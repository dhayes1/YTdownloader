//! Download engine: probes videos with yt-dlp, downloads streams, runs ffmpeg.
//! No Tauri dependency — the UI talks to it through the app crate.

pub mod backend;
pub mod download;
pub mod error;
pub mod job;
pub mod naming;
pub mod queue;
pub mod select;
#[cfg(test)]
pub(crate) mod testing;
pub mod tools;
pub mod types;
pub mod ytdlp;

pub use backend::{Backend, SystemBackend};
pub use error::*;
pub use queue::*;
pub use tools::missing_tools;
pub use types::*;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

/// The video id and `list` parameter of a URL, when it has both and both are non-empty.
/// The video id is the `v` parameter, or for `youtu.be/<id>` and `/shorts/<id>` links the path.
fn video_and_list(url: &str) -> Option<(String, String)> {
    let no_fragment = url.split('#').next()?;
    let (before_query, query) = no_fragment.split_once('?')?;
    let after_scheme = before_query.split_once("://").map_or(before_query, |(_, rest)| rest);
    let (host, path) = after_scheme.split_once('/').unwrap_or((after_scheme, ""));
    let mut segments = path.split('/');
    let path_id = match (host.trim_start_matches("www."), segments.next()) {
        ("youtu.be", Some(id)) => Some(id),
        (_, Some("shorts")) => segments.next(),
        _ => None,
    }
    .filter(|id| !id.is_empty());

    let (mut video, mut list) = (path_id.map(str::to_string), None);
    for pair in query.split('&') {
        match pair.split_once('=') {
            Some(("v", val)) if !val.is_empty() && path_id.is_none() => video = Some(val.to_string()),
            Some(("list", val)) if !val.is_empty() => list = Some(val.to_string()),
            _ => {}
        }
    }
    Some((video?, list?))
}

/// The engine API the app uses: probe a URL, start a job, cancel a job.
/// Every job gets its own ID, so a V2 queue can run several through the same `start`.
pub struct Engine<B: Backend = SystemBackend> {
    backend: Arc<B>,
    next_id: AtomicU64,
    running: Arc<Mutex<HashMap<JobId, CancellationToken>>>,
}

impl Engine<SystemBackend> {
    pub fn new() -> Self {
        Self::with_backend(SystemBackend::default())
    }
}

impl Default for Engine<SystemBackend> {
    fn default() -> Self {
        Self::new()
    }
}

impl<B: Backend> Engine<B> {
    pub fn with_backend(backend: B) -> Self {
        Self {
            backend: Arc::new(backend),
            next_id: AtomicU64::new(1),
            running: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Reads a video's title, duration, thumbnail, and resolution list, or a playlist's entries.
    pub async fn probe(&self, url: &str) -> Result<Probed, EngineError> {
        let json = self.backend.probe_json(url).await?;
        let probed = select::to_probed(&ytdlp::parse_info(&json)?)?;
        let (Some((_, list)), Probed::Video { info }) = (video_and_list(url), &probed) else {
            return Ok(probed);
        };
        let lookup = if list.starts_with("RD") {
            PlaylistLookup::Mix
        } else {
            // The video itself was readable, so any trouble with the playlist just means the user
            // is offered "Just this video".
            let found = match self.backend.probe_playlist_json(url).await {
                Ok(json) => ytdlp::parse_info(&json).and_then(|i| select::to_probed(&i)),
                Err(e) => Err(e),
            };
            match found {
                Ok(Probed::Playlist { playlist }) => PlaylistLookup::Found { playlist },
                _ => PlaylistLookup::Unavailable,
            }
        };
        Ok(Probed::VideoInPlaylist { info: info.clone(), lookup })
    }

    /// Starts `job` in the background and returns its ID. Events arrive on `events`.
    /// Must be called inside a tokio runtime.
    pub fn start(&self, job: DownloadJob, events: UnboundedSender<JobEvent>) -> JobId {
        let id = JobId(self.next_id.fetch_add(1, Ordering::Relaxed));
        let cancel = CancellationToken::new();
        self.running.lock().unwrap().insert(id, cancel.clone());

        let backend = Arc::clone(&self.backend);
        let running = Arc::clone(&self.running);
        tokio::spawn(async move {
            job::run_job(&*backend, id, job, cancel, &events).await;
            running.lock().unwrap().remove(&id);
        });
        id
    }

    /// Cancels the job with this ID. Does nothing if it isn't running.
    pub fn cancel(&self, id: JobId) {
        if let Some(token) = self.running.lock().unwrap().get(&id) {
            token.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeBackend, SYNTH};
    use std::sync::atomic::Ordering;

    const FLAT: &str = include_str!("../tests/fixtures/flat_playlist.json");

    const REGULAR: &str = "https://www.youtube.com/watch?v=x3RK6U8nvPs&list=PL4kXoZZASRgs8_19aGso98561fkl8Slmk";
    const PRIVATE: &str = "https://www.youtube.com/watch?v=mwcOKX7PaRg&list=PLH0glPyGJz0EOpz04H0cQ4l_xIBaW042K&pp=sAgC";
    const MIX: &str = "https://www.youtube.com/watch?v=VWYdlnKqByQ&list=RDVWYdlnKqByQ&start_radio=1";

    #[test]
    fn video_and_list_ids() {
        assert_eq!(video_and_list(REGULAR), Some(("x3RK6U8nvPs".into(), "PL4kXoZZASRgs8_19aGso98561fkl8Slmk".into())));
        assert_eq!(video_and_list(PRIVATE), Some(("mwcOKX7PaRg".into(), "PLH0glPyGJz0EOpz04H0cQ4l_xIBaW042K".into())));
        assert_eq!(video_and_list(MIX), Some(("VWYdlnKqByQ".into(), "RDVWYdlnKqByQ".into())));
        assert_eq!(
            video_and_list("https://youtu.be/x3RK6U8nvPs?list=PL4kXoZZASRgs8_19aGso98561fkl8Slmk&si=abc"),
            Some(("x3RK6U8nvPs".into(), "PL4kXoZZASRgs8_19aGso98561fkl8Slmk".into()))
        );
        assert_eq!(
            video_and_list("https://www.youtube.com/shorts/abc123?list=PLx"),
            Some(("abc123".into(), "PLx".into()))
        );
        assert_eq!(video_and_list("https://youtu.be/abc?si=zzz"), None);
        assert_eq!(video_and_list("https://youtu.be/?list=PLx"), None);
        assert_eq!(video_and_list("https://www.youtube.com/watch?v=abc"), None);
        assert_eq!(video_and_list("https://www.youtube.com/playlist?list=PLx"), None);
        assert_eq!(video_and_list("https://www.youtube.com/watch?pv=abc&xlist=PLx"), None);
    }

    #[tokio::test]
    async fn probe_video_in_playlist_found() {
        let fake = FakeBackend { json: SYNTH, playlist_json: Some(FLAT), ..FakeBackend::new() };
        match Engine::with_backend(fake).probe(REGULAR).await.unwrap() {
            Probed::VideoInPlaylist { lookup: PlaylistLookup::Found { playlist }, .. } => {
                assert!(!playlist.entries.is_empty())
            }
            other => panic!("got {other:?}"),
        }
    }

    #[tokio::test]
    async fn probe_video_in_playlist_mix() {
        let fake = FakeBackend { json: SYNTH, playlist_json: Some(FLAT), ..FakeBackend::new() };
        let engine = Engine::with_backend(fake);
        let r = engine.probe(MIX).await.unwrap();
        assert!(matches!(r, Probed::VideoInPlaylist { lookup: PlaylistLookup::Mix, .. }), "got {r:?}");
        assert_eq!(engine.backend.playlist_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn probe_video_in_playlist_unavailable() {
        // A video result for the playlist probe means the list isn't a real playlist.
        let fake = FakeBackend { json: SYNTH, playlist_json: Some(SYNTH), ..FakeBackend::new() };
        let r = Engine::with_backend(fake).probe(PRIVATE).await.unwrap();
        assert!(matches!(r, Probed::VideoInPlaylist { lookup: PlaylistLookup::Unavailable, .. }), "got {r:?}");
    }

    #[tokio::test]
    async fn probe_video_in_playlist_any_playlist_error_is_unavailable() {
        let fake = FakeBackend { json: SYNTH, playlist_fails: true, ..FakeBackend::new() };
        let r = Engine::with_backend(fake).probe(REGULAR).await.unwrap();
        assert!(matches!(r, Probed::VideoInPlaylist { lookup: PlaylistLookup::Unavailable, .. }), "got {r:?}");
        // An unparseable playlist result is also just "unavailable".
        let fake = FakeBackend { json: SYNTH, playlist_json: Some("not json"), ..FakeBackend::new() };
        let r = Engine::with_backend(fake).probe(REGULAR).await.unwrap();
        assert!(matches!(r, Probed::VideoInPlaylist { lookup: PlaylistLookup::Unavailable, .. }), "got {r:?}");
    }

    #[tokio::test]
    async fn probe_plain_video_skips_playlist_probe() {
        let fake = FakeBackend { json: SYNTH, playlist_json: Some(FLAT), ..FakeBackend::new() };
        let engine = Engine::with_backend(fake);
        let r = engine.probe("https://www.youtube.com/watch?v=abc").await.unwrap();
        assert!(matches!(r, Probed::Video { .. }));
        assert_eq!(engine.backend.playlist_calls.load(Ordering::SeqCst), 0);
    }
}
