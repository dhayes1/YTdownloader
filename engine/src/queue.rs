//! The download queue: videos and playlist entries run one at a time, in order.
//!
//! All state sits behind one `std::sync::Mutex`, held only for short changes and never across
//! an `.await`. Events are sent while the lock is held, so their order always matches the order
//! of the state changes they describe.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio_util::sync::CancellationToken;

use crate::backend::{Backend, SystemBackend};
use crate::error::EngineError;
use crate::job::{run_job, JobOutcome};
use crate::types::{DownloadJob, DownloadKind, JobEventKind, JobId, Phase, PlaylistInfo, Quality};

/// Identifies one queue row. Serializes as a plain number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntryId(pub u64);

/// Identifies one playlist added to the queue. Serializes as a plain number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GroupId(pub u64);

/// Where a queue row is.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EntryState {
    Waiting,
    Running,
    Done { path: PathBuf },
    Failed { msg: String },
}

/// One video in the queue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueEntry {
    pub id: EntryId,
    /// The playlist it came from, if any.
    pub group: Option<GroupId>,
    pub url: String,
    pub title: String,
    /// The playlist position, zero-padded ("01"), put before the file name.
    pub prefix: Option<String>,
    pub state: EntryState,
}

/// A playlist in the queue. Its title is the subfolder its entries go into.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueGroup {
    pub id: GroupId,
    pub title: String,
}

/// Whether the queue is working through its entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QueueStatus {
    Idle,
    Running,
    Paused,
}

/// What changed in the queue.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum QueueEvent {
    Added { group: Option<QueueGroup>, entries: Vec<QueueEntry> },
    Removed { ids: Vec<EntryId>, group: Option<GroupId> },
    State { id: EntryId, state: EntryState },
    Phase { id: EntryId, phase: Phase },
    Progress { id: EntryId, done: u64, total: Option<u64> },
    Status { status: QueueStatus },
}

struct State {
    entries: Vec<QueueEntry>,
    groups: HashMap<GroupId, QueueGroup>,
    status: QueueStatus,
    choice: DownloadKind,
    folder: PathBuf,
    next_id: u64,
    /// True from `start` spawning the runner until the runner decides to exit.
    runner_alive: bool,
    /// The entry the runner is working on, and the token that cancels its job.
    current: Option<(EntryId, CancellationToken)>,
    stop_requested: bool,
}

struct Inner<B> {
    backend: Arc<B>,
    events: UnboundedSender<QueueEvent>,
    state: Mutex<State>,
}

impl<B> Inner<B> {
    fn lock(&self) -> MutexGuard<'_, State> {
        // A panic elsewhere must not take the whole queue down with it.
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A closed channel just means nobody is listening any more.
    fn emit(&self, event: QueueEvent) {
        let _ = self.events.send(event);
    }

    /// Changes a listed entry's state. Does nothing for an entry that was removed.
    fn set_state(&self, s: &mut State, id: EntryId, state: EntryState) {
        if let Some(e) = s.entries.iter_mut().find(|e| e.id == id) {
            e.state = state.clone();
            self.emit(QueueEvent::State { id, state });
        }
    }

    fn set_status(&self, s: &mut State, status: QueueStatus) {
        s.status = status;
        self.emit(QueueEvent::Status { status });
    }

    /// Removes the entries `which` selects, cancelling the running one. A group left with no
    /// entries is removed too, and its entries go out in their own `Removed` event that names it;
    /// every other removed entry goes out in one `Removed` event with no group.
    fn remove_where(&self, s: &mut State, which: impl Fn(&QueueEntry) -> bool) {
        let (removed, kept): (Vec<QueueEntry>, Vec<QueueEntry>) =
            std::mem::take(&mut s.entries).into_iter().partition(|e| which(e));
        s.entries = kept;
        if removed.is_empty() {
            return;
        }
        if let Some((id, token)) = &s.current {
            if removed.iter().any(|e| e.id == *id) {
                token.cancel();
            }
        }
        let mut emptied: Vec<GroupId> = Vec::new();
        for g in removed.iter().filter_map(|e| e.group) {
            if !emptied.contains(&g) && !s.entries.iter().any(|e| e.group == Some(g)) {
                emptied.push(g);
            }
        }
        let rest: Vec<EntryId> =
            removed.iter().filter(|e| !e.group.is_some_and(|g| emptied.contains(&g))).map(|e| e.id).collect();
        if !rest.is_empty() {
            self.emit(QueueEvent::Removed { ids: rest, group: None });
        }
        for g in emptied {
            s.groups.remove(&g);
            let ids = removed.iter().filter(|e| e.group == Some(g)).map(|e| e.id).collect();
            self.emit(QueueEvent::Removed { ids, group: Some(g) });
        }
    }
}

/// The download queue. Cheap to clone: clones share the same queue.
pub struct Queue<B: Backend = SystemBackend> {
    inner: Arc<Inner<B>>,
}

impl<B: Backend> Clone for Queue<B> {
    fn clone(&self) -> Self {
        Self { inner: Arc::clone(&self.inner) }
    }
}

impl<B: Backend> Queue<B> {
    pub fn new(backend: Arc<B>, events: UnboundedSender<QueueEvent>) -> Self {
        let state = State {
            entries: Vec::new(),
            groups: HashMap::new(),
            status: QueueStatus::Idle,
            choice: DownloadKind::Video { quality: Quality::Best },
            folder: PathBuf::new(),
            next_id: 1,
            runner_alive: false,
            current: None,
            stop_requested: false,
        };
        Self { inner: Arc::new(Inner { backend, events, state: Mutex::new(state) }) }
    }

    /// What each entry downloads as; read when the entry starts.
    pub fn set_choice(&self, kind: DownloadKind) {
        self.inner.lock().choice = kind;
    }

    /// Where each entry is saved; read when the entry starts.
    pub fn set_folder(&self, folder: PathBuf) {
        self.inner.lock().folder = folder;
    }

    pub fn add_video(&self, url: String, title: String) -> EntryId {
        let mut s = self.inner.lock();
        let id = EntryId(s.next_id);
        s.next_id += 1;
        let entry = QueueEntry { id, group: None, url, title, prefix: None, state: EntryState::Waiting };
        s.entries.push(entry.clone());
        self.inner.emit(QueueEvent::Added { group: None, entries: vec![entry] });
        id
    }

    /// Adds every entry of `playlist`, numbered by its playlist position, into a subfolder
    /// named after the playlist.
    pub fn add_playlist(&self, playlist: &PlaylistInfo) -> GroupId {
        // Wide enough for the highest position too: unavailable entries may be missing from the list.
        let highest = playlist.entries.iter().map(|p| p.index as usize).max().unwrap_or(0);
        let width = playlist.entries.len().max(highest).to_string().len().max(2);
        let mut s = self.inner.lock();
        let group = QueueGroup { id: GroupId(s.next_id), title: playlist.title.clone() };
        s.next_id += 1;
        let mut added = Vec::with_capacity(playlist.entries.len());
        for p in &playlist.entries {
            added.push(QueueEntry {
                id: EntryId(s.next_id),
                group: Some(group.id),
                url: p.url.clone(),
                title: p.title.clone(),
                prefix: Some(format!("{:0width$}", p.index)),
                state: EntryState::Waiting,
            });
            s.next_id += 1;
        }
        s.entries.extend(added.iter().cloned());
        s.groups.insert(group.id, group.clone());
        self.inner.emit(QueueEvent::Added { group: Some(group.clone()), entries: added });
        group.id
    }

    /// Drops the entry. If it's running, its job is cancelled (and cleans up its files) and the
    /// queue moves on. Files of finished entries are never touched.
    pub fn remove(&self, id: EntryId) {
        let mut s = self.inner.lock();
        self.inner.remove_where(&mut s, |e| e.id == id);
    }

    /// Drops every entry of the playlist in one event, cancelling the running one if it's a member.
    /// Does nothing for an unknown or already removed group.
    pub fn remove_group(&self, id: GroupId) {
        let mut s = self.inner.lock();
        if !s.groups.contains_key(&id) {
            return;
        }
        self.inner.remove_where(&mut s, |e| e.group == Some(id));
        // A playlist added with no entries is still listed; drop it on its own.
        if s.groups.remove(&id).is_some() {
            self.inner.emit(QueueEvent::Removed { ids: Vec::new(), group: Some(id) });
        }
    }

    /// Starts working through Waiting entries. Must be called inside a tokio runtime.
    /// Called after `stop` but before the runner has exited, it cancels the stop: the stopped
    /// entry goes back to Waiting and the runner picks it up again.
    pub fn start(&self) {
        {
            let mut s = self.inner.lock();
            if s.runner_alive {
                s.stop_requested = false;
                return;
            }
            if !s.entries.iter().any(|e| e.state == EntryState::Waiting) {
                return;
            }
            s.runner_alive = true;
            s.stop_requested = false;
            self.inner.set_status(&mut s, QueueStatus::Running);
        }
        tokio::spawn(run_guarded(Arc::clone(&self.inner)));
    }

    /// Cancels the running entry (it goes back to Waiting) and pauses the queue.
    pub fn stop(&self) {
        let mut s = self.inner.lock();
        if !s.runner_alive {
            return;
        }
        s.stop_requested = true;
        if let Some((_, token)) = &s.current {
            token.cancel();
        }
    }

    /// Drops Done and Failed entries.
    pub fn clear_finished(&self) {
        let finished = |e: &QueueEntry| matches!(e.state, EntryState::Done { .. } | EntryState::Failed { .. });
        let mut s = self.inner.lock();
        self.inner.remove_where(&mut s, finished);
    }

    pub fn status(&self) -> QueueStatus {
        self.inner.lock().status
    }

    pub fn entries(&self) -> Vec<QueueEntry> {
        self.inner.lock().entries.clone()
    }

    /// True while the runner task is working through the queue.
    pub fn is_running(&self) -> bool {
        self.inner.lock().runner_alive
    }
}

/// Ends the runner and sets the final status. Called with the lock held, so a `start` racing
/// with the exit either sees the runner alive (and its entry already handled) or fully gone.
fn finish<B>(inner: &Inner<B>, s: &mut State, status: QueueStatus) {
    s.runner_alive = false;
    s.stop_requested = false;
    s.current = None;
    inner.set_status(s, status);
}

/// If the runner panics, this puts the queue back in a usable state: the entry it was running
/// fails, the runner is marked gone and the status is Paused, so `start` works again.
struct RunnerGuard<B> {
    inner: Arc<Inner<B>>,
    armed: bool,
}

impl<B> Drop for RunnerGuard<B> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut s = self.inner.lock();
        if let Some((id, _)) = s.current.take() {
            let msg = "Something went wrong while downloading this video.".to_string();
            self.inner.set_state(&mut s, id, EntryState::Failed { msg });
        }
        finish(&self.inner, &mut s, QueueStatus::Paused);
    }
}

async fn run_guarded<B: Backend>(inner: Arc<Inner<B>>) {
    let mut guard = RunnerGuard { inner: Arc::clone(&inner), armed: true };
    run(inner).await;
    guard.armed = false;
}

/// The runner task: one entry at a time until none is Waiting, a stop, or a missing folder.
async fn run<B: Backend>(inner: Arc<Inner<B>>) {
    loop {
        let (id, job, cancel) = {
            let mut s = inner.lock();
            if s.stop_requested {
                let waiting = s.entries.iter().any(|e| e.state == EntryState::Waiting);
                let status = if waiting { QueueStatus::Paused } else { QueueStatus::Idle };
                return finish(&inner, &mut s, status);
            }
            let Some(entry) = s.entries.iter().find(|e| e.state == EntryState::Waiting).cloned() else {
                return finish(&inner, &mut s, QueueStatus::Idle);
            };
            let subfolder = entry.group.and_then(|g| s.groups.get(&g)).map(|g| g.title.clone());
            let job = DownloadJob {
                url: entry.url,
                kind: s.choice.clone(),
                folder: s.folder.clone(),
                subfolder,
                prefix: entry.prefix,
            };
            let cancel = CancellationToken::new();
            s.current = Some((entry.id, cancel.clone()));
            inner.set_state(&mut s, entry.id, EntryState::Running);
            (entry.id, job, cancel)
        };

        let outcome = run_forwarding(&inner, id, job, cancel).await;

        let mut s = inner.lock();
        s.current = None;
        // `set_state` ignores an entry removed while it ran (or just as it finished).
        let removed = !s.entries.iter().any(|e| e.id == id);
        match outcome {
            JobOutcome::Done(path) => inner.set_state(&mut s, id, EntryState::Done { path }),
            JobOutcome::Failed(e @ EngineError::Folder(_)) => {
                inner.set_state(&mut s, id, EntryState::Failed { msg: e.user_message() });
                return finish(&inner, &mut s, QueueStatus::Paused);
            }
            JobOutcome::Failed(e) => inner.set_state(&mut s, id, EntryState::Failed { msg: e.user_message() }),
            JobOutcome::Cancelled if removed => {}
            JobOutcome::Cancelled => {
                // Only `stop` and `remove` cancel, so a listed entry was stopped. If `start` has
                // cancelled the stop since, the loop picks this entry up again.
                inner.set_state(&mut s, id, EntryState::Waiting);
            }
        }
        // A pending stop is handled at the top of the loop, under the same lock as picking.
    }
}

/// Runs the entry's job, forwarding its Phase and Progress events while the entry is listed.
async fn run_forwarding<B: Backend>(inner: &Inner<B>, id: EntryId, job: DownloadJob, cancel: CancellationToken) -> JobOutcome {
    let (tx, mut rx) = unbounded_channel();
    let backend = &*inner.backend;
    let job_run = async move {
        let outcome = run_job(backend, JobId(id.0), job, cancel, &tx).await;
        drop(tx); // ends the forwarding loop
        outcome
    };
    let forward = async {
        while let Some(e) = rx.recv().await {
            let event = match e.kind {
                JobEventKind::Phase { phase } => QueueEvent::Phase { id, phase },
                JobEventKind::Progress { done, total } => QueueEvent::Progress { id, done, total },
                // The outcome is reported by the runner as a State change.
                _ => continue,
            };
            let s = inner.lock();
            if s.entries.iter().any(|e| e.id == id) {
                inner.emit(event);
            }
        }
    };
    let (outcome, ()) = tokio::join!(job_run, forward);
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashSet;
    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
    use tokio::sync::Semaphore;
    use tokio_util::sync::CancellationToken;

    use crate::backend::Backend;
    use crate::download::ProgressFn;
    use crate::error::EngineError;
    use crate::naming::sanitize_filename;
    use crate::testing::{FakeBackend, Stage};
    use crate::types::{DownloadKind, Phase, PlaylistEntry, PlaylistInfo};
    use crate::ytdlp::RawFormat;

    const WAIT: Duration = Duration::from_secs(5);

    /// A `FakeBackend` that holds every call in `stage` (after writing its file) until the test
    /// opens the gate or the job is cancelled.
    struct Gated {
        inner: FakeBackend,
        stage: Stage,
        gate: Semaphore,
    }

    impl Gated {
        fn new(inner: FakeBackend, stage: Stage) -> Arc<Self> {
            Arc::new(Self { inner, stage, gate: Semaphore::new(0) })
        }
        /// Lets every held and future call through.
        fn open(&self) {
            self.gate.add_permits(1000);
        }
        async fn hold(&self, stage: Stage, cancel: &CancellationToken) -> Result<(), EngineError> {
            if self.stage != stage {
                return Ok(());
            }
            tokio::select! {
                _ = cancel.cancelled() => Err(EngineError::Cancelled),
                p = self.gate.acquire() => { p.unwrap().forget(); Ok(()) }
            }
        }
    }

    #[async_trait]
    impl Backend for Gated {
        async fn probe_json(&self, url: &str) -> Result<String, EngineError> {
            self.inner.probe_json(url).await
        }
        async fn probe_playlist_json(&self, url: &str) -> Result<String, EngineError> {
            self.inner.probe_playlist_json(url).await
        }
        async fn download(
            &self,
            fmt: &RawFormat,
            dest: &Path,
            cancel: &CancellationToken,
            progress: ProgressFn<'_>,
        ) -> Result<(), EngineError> {
            self.inner.download(fmt, dest, cancel, progress).await?;
            self.hold(Stage::Download, cancel).await
        }
        async fn ffmpeg(&self, args: Vec<OsString>, cancel: &CancellationToken) -> Result<(), EngineError> {
            self.inner.ffmpeg(args, cancel).await?;
            self.hold(Stage::Ffmpeg, cancel).await
        }
    }

    fn queue<B: Backend>(backend: Arc<B>, folder: &Path) -> (Queue<B>, UnboundedReceiver<QueueEvent>) {
        let (tx, rx) = unbounded_channel();
        let q = Queue::new(backend, tx);
        q.set_folder(folder.to_path_buf());
        (q, rx)
    }

    async fn next(rx: &mut UnboundedReceiver<QueueEvent>) -> QueueEvent {
        tokio::time::timeout(WAIT, rx.recv()).await.expect("timed out waiting for an event").expect("channel closed")
    }

    /// Gathers events up to and including the first that matches `stop`.
    async fn collect_until(rx: &mut UnboundedReceiver<QueueEvent>, stop: impl Fn(&QueueEvent) -> bool) -> Vec<QueueEvent> {
        let mut events = Vec::new();
        loop {
            let e = next(rx).await;
            let end = stop(&e);
            events.push(e);
            if end {
                return events;
            }
        }
    }

    /// Gathers events until the queue reports Idle or Paused.
    async fn collect_until_idle(rx: &mut UnboundedReceiver<QueueEvent>) -> Vec<QueueEvent> {
        collect_until(rx, |e| {
            matches!(e, QueueEvent::Status { status: QueueStatus::Idle | QueueStatus::Paused })
        })
        .await
    }

    /// Waits until entry `id` reports `phase`.
    async fn wait_phase(rx: &mut UnboundedReceiver<QueueEvent>, id: EntryId, phase: Phase) -> Vec<QueueEvent> {
        collect_until(rx, |e| matches!(e, QueueEvent::Phase { id: i, phase: p } if *i == id && *p == phase)).await
    }

    fn states(events: &[QueueEvent]) -> Vec<(EntryId, EntryState)> {
        events
            .iter()
            .filter_map(|e| match e {
                QueueEvent::State { id, state } => Some((*id, state.clone())),
                _ => None,
            })
            .collect()
    }

    fn done(path: &Path) -> EntryState {
        EntryState::Done { path: path.to_path_buf() }
    }

    fn done_path(events: &[QueueEvent], id: EntryId) -> PathBuf {
        states(events)
            .into_iter()
            .find_map(|(i, s)| match s {
                EntryState::Done { path } if i == id => Some(path),
                _ => None,
            })
            .unwrap_or_else(|| panic!("entry {id:?} never finished: {events:?}"))
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    fn last_status(events: &[QueueEvent]) -> QueueStatus {
        events
            .iter()
            .rev()
            .find_map(|e| match e {
                QueueEvent::Status { status } => Some(*status),
                _ => None,
            })
            .expect("no status event")
    }

    fn state_of<B: Backend>(q: &Queue<B>, id: EntryId) -> EntryState {
        q.entries().into_iter().find(|e| e.id == id).expect("entry missing").state
    }

    fn assert_no_events(rx: &mut UnboundedReceiver<QueueEvent>) {
        if let Ok(e) = rx.try_recv() {
            panic!("unexpected event: {e:?}");
        }
    }

    fn playlist(title: &str, n: u32) -> PlaylistInfo {
        PlaylistInfo {
            title: title.into(),
            entries: (1..=n).map(|i| PlaylistEntry { url: format!("p{i}"), title: format!("Song {i}"), index: i }).collect(),
        }
    }

    #[tokio::test]
    async fn runs_entries_in_order_one_at_a_time() {
        let dir = tempfile::tempdir().unwrap();
        let (q, mut rx) = queue(Arc::new(FakeBackend::new()), dir.path());
        let a = q.add_video("a".into(), "A".into());
        let b = q.add_video("b".into(), "B".into());
        let c = q.add_video("c".into(), "C".into());
        assert_eq!(q.status(), QueueStatus::Idle);
        q.start();
        let ev = collect_until_idle(&mut rx).await;

        assert!(matches!(ev.iter().find(|e| matches!(e, QueueEvent::Status { .. })), Some(QueueEvent::Status { status: QueueStatus::Running })));
        let (pa, pb, pc) = (done_path(&ev, a), done_path(&ev, b), done_path(&ev, c));
        assert_eq!(
            states(&ev),
            [
                (a, EntryState::Running),
                (a, done(&pa)),
                (b, EntryState::Running),
                (b, done(&pb)),
                (c, EntryState::Running),
                (c, done(&pc)),
            ]
        );
        assert_eq!(last_status(&ev), QueueStatus::Idle);
        assert_eq!(q.status(), QueueStatus::Idle);
        assert!(!q.is_running());
        assert!(q.entries().iter().all(|e| matches!(e.state, EntryState::Done { .. })));
    }

    #[tokio::test]
    async fn failed_entry_does_not_stop_queue() {
        let dir = tempfile::tempdir().unwrap();
        let fake = FakeBackend { private_urls: HashSet::from(["bad".to_string()]), ..FakeBackend::new() };
        let (q, mut rx) = queue(Arc::new(fake), dir.path());
        let bad = q.add_video("bad".into(), "Bad".into());
        let good = q.add_video("good".into(), "Good".into());
        q.start();
        let ev = collect_until_idle(&mut rx).await;

        assert!(matches!(&state_of(&q, bad), EntryState::Failed { msg } if msg.contains("private")), "{ev:?}");
        done_path(&ev, good);
        assert_eq!(last_status(&ev), QueueStatus::Idle);
    }

    #[tokio::test]
    async fn missing_folder_pauses_queue() {
        let dir = tempfile::tempdir().unwrap();
        let (q, mut rx) = queue(Arc::new(FakeBackend::new()), &dir.path().join("gone"));
        let first = q.add_video("a".into(), "A".into());
        let second = q.add_video("b".into(), "B".into());
        q.start();
        let ev = collect_until_idle(&mut rx).await;

        assert!(matches!(&state_of(&q, first), EntryState::Failed { msg } if msg.contains("Choose another folder")));
        assert_eq!(last_status(&ev), QueueStatus::Paused);
        assert_eq!(q.status(), QueueStatus::Paused);
        assert_eq!(state_of(&q, second), EntryState::Waiting);
        assert!(!q.is_running());

        q.set_folder(dir.path().to_path_buf());
        q.start();
        let ev = collect_until_idle(&mut rx).await;
        done_path(&ev, second);
        assert_eq!(last_status(&ev), QueueStatus::Idle);
    }

    #[tokio::test]
    async fn stop_puts_running_entry_back_to_waiting() {
        let dir = tempfile::tempdir().unwrap();
        let gated = Gated::new(FakeBackend::new(), Stage::Download);
        let (q, mut rx) = queue(Arc::clone(&gated), dir.path());
        let a = q.add_video("a".into(), "A".into());
        q.start();
        wait_phase(&mut rx, a, Phase::DownloadingVideo).await;
        q.stop();
        let ev = collect_until_idle(&mut rx).await;

        assert_eq!(states(&ev).last(), Some(&(a, EntryState::Waiting)));
        assert_eq!(last_status(&ev), QueueStatus::Paused);
        assert_eq!(state_of(&q, a), EntryState::Waiting);
        assert!(files(dir.path()).is_empty());
        assert!(!q.is_running());

        gated.open();
        q.start();
        let ev = collect_until_idle(&mut rx).await;
        assert_eq!(done_path(&ev, a), dir.path().join("Synthetic Video.mkv"));
        assert_eq!(last_status(&ev), QueueStatus::Idle);
    }

    #[tokio::test]
    async fn removing_running_entry_cancels_cleans_up_and_continues() {
        let dir = tempfile::tempdir().unwrap();
        let gated = Gated::new(FakeBackend::new(), Stage::Ffmpeg);
        let (q, mut rx) = queue(Arc::clone(&gated), dir.path());
        let first = q.add_video("a".into(), "A".into());
        let second = q.add_video("b".into(), "B".into());
        q.start();
        wait_phase(&mut rx, first, Phase::Merging).await;
        q.remove(first);
        gated.open(); // for the second entry
        let ev = collect_until_idle(&mut rx).await;

        let removed = ev.iter().position(|e| *e == QueueEvent::Removed { ids: vec![first], group: None }).expect("no Removed");
        let second_running = ev.iter().position(|e| *e == QueueEvent::State { id: second, state: EntryState::Running }).unwrap();
        assert!(removed < second_running);
        assert!(!states(&ev).iter().any(|(id, _)| *id == first), "removed entry got a state: {ev:?}");
        // If the first entry's files were still there, the second would be "Synthetic Video (1).mkv".
        assert_eq!(done_path(&ev, second), dir.path().join("Synthetic Video.mkv"));
        assert_eq!(files(dir.path()), ["Synthetic Video.mkv"]);
        assert_eq!(q.entries().iter().map(|e| e.id).collect::<Vec<_>>(), [second]);
        assert_eq!(last_status(&ev), QueueStatus::Idle);
    }

    #[tokio::test]
    async fn remove_group_removes_all_and_cancels_running_member() {
        let dir = tempfile::tempdir().unwrap();
        let gated = Gated::new(FakeBackend::new(), Stage::Download);
        let (q, mut rx) = queue(Arc::clone(&gated), dir.path());
        let g = q.add_playlist(&playlist("Mix", 2));
        let single = q.add_video("s".into(), "Single".into());
        let members: Vec<EntryId> = q.entries().iter().filter(|e| e.group == Some(g)).map(|e| e.id).collect();
        assert_eq!(members.len(), 2);
        q.start();
        wait_phase(&mut rx, members[0], Phase::DownloadingVideo).await;
        q.remove_group(g);
        gated.open(); // for the single
        let ev = collect_until_idle(&mut rx).await;

        let removed: Vec<&QueueEvent> = ev.iter().filter(|e| matches!(e, QueueEvent::Removed { .. })).collect();
        assert_eq!(removed, [&QueueEvent::Removed { ids: members.clone(), group: Some(g) }]);
        assert!(!states(&ev).iter().any(|(id, _)| members.contains(id)), "{ev:?}");
        assert_eq!(done_path(&ev, single), dir.path().join("Synthetic Video.mkv"));
        assert_eq!(q.entries().iter().map(|e| e.id).collect::<Vec<_>>(), [single]);
    }

    #[tokio::test]
    async fn added_while_running_joins_the_end() {
        let dir = tempfile::tempdir().unwrap();
        let gated = Gated::new(FakeBackend::new(), Stage::Download);
        let (q, mut rx) = queue(Arc::clone(&gated), dir.path());
        let a = q.add_video("a".into(), "A".into());
        q.start();
        wait_phase(&mut rx, a, Phase::DownloadingVideo).await;
        let b = q.add_video("b".into(), "B".into());
        gated.open();
        let ev = collect_until_idle(&mut rx).await;

        let (pa, pb) = (done_path(&ev, a), done_path(&ev, b));
        assert_eq!(states(&ev), [(a, done(&pa)), (b, EntryState::Running), (b, done(&pb))]);
        assert_eq!(last_status(&ev), QueueStatus::Idle);
    }

    #[tokio::test]
    async fn choice_is_read_when_each_entry_starts() {
        let dir = tempfile::tempdir().unwrap();
        let gated = Gated::new(FakeBackend::new(), Stage::Download);
        let (q, mut rx) = queue(Arc::clone(&gated), dir.path());
        let a = q.add_video("a".into(), "A".into());
        let b = q.add_video("b".into(), "B".into());
        q.start();
        wait_phase(&mut rx, a, Phase::DownloadingVideo).await;
        q.set_choice(DownloadKind::Mp3);
        gated.open();
        let ev = collect_until_idle(&mut rx).await;

        assert_eq!(done_path(&ev, a).extension().unwrap(), "mkv");
        assert_eq!(done_path(&ev, b).extension().unwrap(), "mp3");
    }

    #[tokio::test]
    async fn playlist_entries_get_padded_prefix_and_subfolder() {
        let dir = tempfile::tempdir().unwrap();
        let title = "Road Trip: Mix Vol 1";
        let (q, mut rx) = queue(Arc::new(FakeBackend::new()), dir.path());
        let g = q.add_playlist(&playlist(title, 3));

        let added = next(&mut rx).await;
        let QueueEvent::Added { group: Some(group), entries } = added else { panic!("expected Added with a group: {added:?}") };
        assert_eq!(group, QueueGroup { id: g, title: title.into() });
        assert_eq!(entries, q.entries());
        assert_eq!(entries.iter().map(|e| e.prefix.as_deref()).collect::<Vec<_>>(), [Some("01"), Some("02"), Some("03")]);
        assert!(entries.iter().all(|e| e.group == Some(g) && e.state == EntryState::Waiting));
        assert_eq!(entries.iter().map(|e| e.url.as_str()).collect::<Vec<_>>(), ["p1", "p2", "p3"]);
        assert_eq!(entries[1].title, "Song 2");

        q.start();
        let ev = collect_until_idle(&mut rx).await;
        let sub = dir.path().join(sanitize_filename(title));
        for (i, e) in entries.iter().enumerate() {
            assert_eq!(done_path(&ev, e.id), sub.join(format!("0{} - Synthetic Video.mkv", i + 1)));
        }

        // Width grows with the playlist: 120 entries -> "001" .. "120". The prefix comes from `index`.
        let (big, _rx) = queue(Arc::new(FakeBackend::new()), dir.path());
        let mut info = playlist("Big", 120);
        info.entries.retain(|e| e.index == 7 || e.index == 120);
        info.entries.extend((0..118).map(|i| PlaylistEntry { url: format!("x{i}"), title: "x".into(), index: 200 + i }));
        big.add_playlist(&info);
        let prefixes: Vec<String> = big.entries().into_iter().map(|e| e.prefix.unwrap()).collect();
        assert_eq!(&prefixes[..2], ["007", "120"]);
        assert_eq!(prefixes.len(), 120);
    }

    #[tokio::test]
    async fn prefix_width_covers_highest_playlist_index() {
        let dir = tempfile::tempdir().unwrap();
        let (q, _rx) = queue(Arc::new(FakeBackend::new()), dir.path());
        // 3 entries listed, but numbered up to 105 (the rest were unavailable).
        let info = PlaylistInfo {
            title: "Gaps".into(),
            entries: [1, 99, 105].map(|i| PlaylistEntry { url: format!("p{i}"), title: "x".into(), index: i }).to_vec(),
        };
        q.add_playlist(&info);
        let prefixes: Vec<String> = q.entries().into_iter().map(|e| e.prefix.unwrap()).collect();
        assert_eq!(prefixes, ["001", "099", "105"]);
    }

    /// Panics when asked to probe the URL "boom"; everything else works.
    struct Panicky(FakeBackend);

    #[async_trait]
    impl Backend for Panicky {
        async fn probe_json(&self, url: &str) -> Result<String, EngineError> {
            if url == "boom" {
                panic!("test panic");
            }
            self.0.probe_json(url).await
        }
        async fn probe_playlist_json(&self, url: &str) -> Result<String, EngineError> {
            self.0.probe_playlist_json(url).await
        }
        async fn download(
            &self,
            fmt: &RawFormat,
            dest: &Path,
            cancel: &CancellationToken,
            progress: ProgressFn<'_>,
        ) -> Result<(), EngineError> {
            self.0.download(fmt, dest, cancel, progress).await
        }
        async fn ffmpeg(&self, args: Vec<OsString>, cancel: &CancellationToken) -> Result<(), EngineError> {
            self.0.ffmpeg(args, cancel).await
        }
    }

    #[tokio::test]
    async fn runner_panic_pauses_the_queue_and_start_works_again() {
        let dir = tempfile::tempdir().unwrap();
        let (q, mut rx) = queue(Arc::new(Panicky(FakeBackend::new())), dir.path());
        let boom = q.add_video("boom".into(), "Boom".into());
        let ok = q.add_video("ok".into(), "Ok".into());
        q.start();
        let ev = collect_until_idle(&mut rx).await;
        assert_eq!(last_status(&ev), QueueStatus::Paused);
        assert!(!q.is_running());
        assert!(matches!(state_of(&q, boom), EntryState::Failed { .. }));
        assert_eq!(state_of(&q, ok), EntryState::Waiting);

        q.start();
        let ev = collect_until_idle(&mut rx).await;
        assert_eq!(last_status(&ev), QueueStatus::Idle);
        assert!(matches!(state_of(&q, ok), EntryState::Done { .. }));
    }

    #[tokio::test]
    async fn clear_finished_removes_done_and_failed_only() {
        let dir = tempfile::tempdir().unwrap();
        let fake = FakeBackend { private_urls: HashSet::from(["bad".to_string()]), ..FakeBackend::new() };
        let (q, mut rx) = queue(Arc::new(fake), dir.path());
        let good = q.add_video("good".into(), "Good".into());
        let bad = q.add_video("bad".into(), "Bad".into());
        q.start();
        collect_until_idle(&mut rx).await;
        let waiting = q.add_video("w".into(), "W".into());
        next(&mut rx).await; // Added
        q.clear_finished();

        assert_eq!(next(&mut rx).await, QueueEvent::Removed { ids: vec![good, bad], group: None });
        assert_no_events(&mut rx);
        assert_eq!(q.entries().iter().map(|e| (e.id, e.state.clone())).collect::<Vec<_>>(), [(waiting, EntryState::Waiting)]);
        assert!(files(dir.path()).contains(&"Synthetic Video.mkv".to_string()), "files are never touched");
    }

    #[tokio::test]
    async fn start_and_stop_are_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let gated = Gated::new(FakeBackend::new(), Stage::Download);
        let (q, mut rx) = queue(Arc::clone(&gated), dir.path());

        q.start(); // nothing waiting
        q.stop(); // idle
        tokio::task::yield_now().await;
        assert_no_events(&mut rx);
        assert!(!q.is_running());

        let a = q.add_video("a".into(), "A".into());
        next(&mut rx).await; // Added
        q.start();
        q.start();
        assert!(q.is_running());
        assert_eq!(q.status(), QueueStatus::Running);
        let ev = wait_phase(&mut rx, a, Phase::DownloadingVideo).await;
        q.start();
        gated.open();
        let mut all = ev;
        all.extend(collect_until_idle(&mut rx).await);
        let running = all.iter().filter(|e| **e == QueueEvent::Status { status: QueueStatus::Running }).count();
        assert_eq!(running, 1, "{all:?}");
        assert_eq!(last_status(&all), QueueStatus::Idle);

        q.stop(); // idle again
        q.start(); // nothing waiting
        tokio::task::yield_now().await;
        assert_no_events(&mut rx);
        assert!(!q.is_running());
        assert_eq!(q.status(), QueueStatus::Idle);
    }

    #[test]
    fn queue_event_json_shape() {
        assert_eq!(
            serde_json::to_value(QueueEvent::Status { status: QueueStatus::Running }).unwrap(),
            serde_json::json!({"type":"status","status":"running"})
        );
        assert_eq!(
            serde_json::to_value(QueueEvent::State { id: EntryId(1), state: EntryState::Done { path: "x".into() } }).unwrap(),
            serde_json::json!({"type":"state","id":1,"state":{"type":"done","path":"x"}})
        );
        assert_eq!(
            serde_json::to_value(QueueEvent::Progress { id: EntryId(2), done: 5, total: None }).unwrap(),
            serde_json::json!({"type":"progress","id":2,"done":5,"total":null})
        );
        let entry = QueueEntry {
            id: EntryId(3),
            group: Some(GroupId(4)),
            url: "u".into(),
            title: "t".into(),
            prefix: Some("01".into()),
            state: EntryState::Waiting,
        };
        assert_eq!(
            serde_json::to_value(QueueEvent::Added { group: Some(QueueGroup { id: GroupId(4), title: "g".into() }), entries: vec![entry] }).unwrap(),
            serde_json::json!({"type":"added","group":{"id":4,"title":"g"},"entries":[{"id":3,"group":4,"url":"u","title":"t","prefix":"01","state":{"type":"waiting"}}]})
        );
        assert_eq!(
            serde_json::to_value(QueueEvent::Removed { ids: vec![EntryId(1)], group: None }).unwrap(),
            serde_json::json!({"type":"removed","ids":[1],"group":null})
        );
    }

    /// Stop, then the running (and only) entry is removed before the runner sees the stop:
    /// nothing is left Waiting, so the queue ends Idle rather than Paused.
    #[tokio::test]
    async fn stop_with_nothing_left_waiting_ends_idle() {
        let dir = tempfile::tempdir().unwrap();
        let gated = Gated::new(FakeBackend::new(), Stage::Download);
        let (q, mut rx) = queue(Arc::clone(&gated), dir.path());
        let a = q.add_video("a".into(), "A".into());
        q.start();
        wait_phase(&mut rx, a, Phase::DownloadingVideo).await;
        q.stop();
        q.remove(a);
        let ev = collect_until_idle(&mut rx).await;

        assert_eq!(last_status(&ev), QueueStatus::Idle, "{ev:?}");
        assert_eq!(q.status(), QueueStatus::Idle);
        assert!(!q.is_running());
    }

    /// Start pressed after Stop but before the runner has exited resumes the queue: the stopped
    /// entry goes back to Waiting and is picked up again, and the queue never reports Paused.
    #[tokio::test]
    async fn start_right_after_stop_resumes() {
        let dir = tempfile::tempdir().unwrap();
        let gated = Gated::new(FakeBackend::new(), Stage::Download);
        let (q, mut rx) = queue(Arc::clone(&gated), dir.path());
        let a = q.add_video("a".into(), "A".into());
        let b = q.add_video("b".into(), "B".into());
        q.start();
        wait_phase(&mut rx, a, Phase::DownloadingVideo).await;
        // On the test's single-threaded runtime the runner can't run between these two calls.
        q.stop();
        q.start();
        assert_eq!(q.status(), QueueStatus::Running);
        gated.open();
        let ev = collect_until_idle(&mut rx).await;

        assert!(!ev.iter().any(|e| matches!(e, QueueEvent::Status { .. }) && *e != QueueEvent::Status { status: QueueStatus::Idle }), "{ev:?}");
        let (pa, pb) = (done_path(&ev, a), done_path(&ev, b));
        assert_eq!(
            states(&ev),
            [(a, EntryState::Waiting), (a, EntryState::Running), (a, done(&pa)), (b, EntryState::Running), (b, done(&pb))]
        );
        assert_eq!(pa, dir.path().join("Synthetic Video.mkv"), "the stopped run's files were cleaned up");
        assert_eq!(last_status(&ev), QueueStatus::Idle);
    }

    #[tokio::test]
    async fn removing_last_entry_of_group_removes_group() {
        let dir = tempfile::tempdir().unwrap();
        let (q, mut rx) = queue(Arc::new(FakeBackend::new()), dir.path());

        // Through `remove`.
        let g = q.add_playlist(&playlist("Mix", 2));
        next(&mut rx).await; // Added
        let m: Vec<EntryId> = q.entries().iter().map(|e| e.id).collect();
        q.remove(m[0]);
        assert_eq!(next(&mut rx).await, QueueEvent::Removed { ids: vec![m[0]], group: None });
        q.remove(m[1]);
        assert_eq!(next(&mut rx).await, QueueEvent::Removed { ids: vec![m[1]], group: Some(g) });
        q.remove_group(g); // already gone
        assert_no_events(&mut rx);

        // Through `clear_finished`: the group's entries come in their own event with the group.
        let g = q.add_playlist(&playlist("Mix", 2));
        let single = q.add_video("s".into(), "S".into());
        let m: Vec<EntryId> = q.entries().iter().filter(|e| e.group == Some(g)).map(|e| e.id).collect();
        q.start();
        collect_until_idle(&mut rx).await;
        q.clear_finished();
        assert_eq!(next(&mut rx).await, QueueEvent::Removed { ids: vec![single], group: None });
        assert_eq!(next(&mut rx).await, QueueEvent::Removed { ids: m, group: Some(g) });
        assert_no_events(&mut rx);
        assert!(q.entries().is_empty());
        q.remove_group(g);
        assert_no_events(&mut rx);
    }

    #[tokio::test]
    async fn remove_unknown_group_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let (q, mut rx) = queue(Arc::new(FakeBackend::new()), dir.path());
        q.remove_group(GroupId(999));
        assert_no_events(&mut rx);

        let g = q.add_playlist(&playlist("Mix", 1));
        next(&mut rx).await; // Added
        q.remove_group(g);
        assert!(matches!(next(&mut rx).await, QueueEvent::Removed { group: Some(id), .. } if id == g));
        q.remove_group(g);
        assert_no_events(&mut rx);
    }
}
