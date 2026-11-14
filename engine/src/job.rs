//! Runs one download from start to finish: probe, download, merge or convert, clean up.
//! Sends exactly one terminal event (Done, Failed, or Cancelled), always last.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

use crate::backend::Backend;
use crate::error::EngineError;
use crate::naming::{sanitize_filename, unique_path};
use crate::select::{container_for, fps_class, select_audio, select_video};
use crate::tools::{merge_args, mp3_args};
use crate::types::{DownloadJob, DownloadKind, JobEvent, JobEventKind, JobId, Phase};
use crate::ytdlp::{parse_info, RawFormat};

type Emit<'a> = &'a (dyn Fn(JobEventKind) + Send + Sync);

/// Runs `job` and reports through `events`. Never returns an error: every outcome is an event.
pub async fn run_job<B: Backend>(
    backend: &B,
    id: JobId,
    job: DownloadJob,
    cancel: CancellationToken,
    events: &UnboundedSender<JobEvent>,
) {
    // A closed channel just means nobody is listening any more; the job still finishes.
    let emit = |kind: JobEventKind| {
        let _ = events.send(JobEvent { job_id: id, kind });
    };
    // Files this job created. Removed if the job doesn't succeed.
    let mut created: Vec<PathBuf> = Vec::new();

    let terminal = match run_steps(backend, &job, &cancel, &emit, &mut created).await {
        Ok(path) => JobEventKind::Done { path },
        Err(e) => {
            for p in &created {
                let _ = tokio::fs::remove_file(p).await;
            }
            match e {
                EngineError::Cancelled => JobEventKind::Cancelled,
                other => JobEventKind::Failed { msg: other.user_message() },
            }
        }
    };
    emit(terminal);
}

/// The job's steps. Returns the final file's path.
async fn run_steps<B: Backend>(
    backend: &B,
    job: &DownloadJob,
    cancel: &CancellationToken,
    emit: Emit<'_>,
    created: &mut Vec<PathBuf>,
) -> Result<PathBuf, EngineError> {
    // Probe again: stream URLs expire, so the UI's earlier probe may be stale.
    emit(JobEventKind::Phase { phase: Phase::Probing });
    let json = tokio::select! {
        _ = cancel.cancelled() => return Err(EngineError::Cancelled),
        r = backend.probe_json(&job.url) => r?,
    };
    let info = parse_info(&json)?;

    if !job.folder.is_dir() {
        return Err(EngineError::Folder(job.folder.clone()));
    }
    let stem = sanitize_filename(&info.title);
    let progress = |done: u64, total: Option<u64>| emit(JobEventKind::Progress { done, total });

    match &job.kind {
        DownloadKind::Video { option } => {
            let video = select_video(&info, option.height, fps_class(Some(option.fps as f64)))
                .ok_or(EngineError::NoStream("video"))?;
            let audio = select_audio(&info).ok_or(EngineError::NoStream("audio"))?;
            let container = container_for(
                video.vcodec.as_deref().unwrap_or(""),
                audio.acodec.as_deref().unwrap_or(""),
            );
            let out = unique_path(&job.folder, &stem, container.ext());
            let video_part = part_path(&out, "video");
            let audio_part = part_path(&out, "audio");

            wait_for_streams(&[video, audio], cancel).await?;
            emit(JobEventKind::Phase { phase: Phase::DownloadingVideo });
            created.push(video_part.clone());
            backend.download(video, &video_part, cancel, &progress).await?;
            check(cancel)?;

            emit(JobEventKind::Phase { phase: Phase::DownloadingAudio });
            created.push(audio_part.clone());
            backend.download(audio, &audio_part, cancel, &progress).await?;
            check(cancel)?;

            emit(JobEventKind::Phase { phase: Phase::Merging });
            created.push(out.clone());
            backend.ffmpeg(merge_args(&video_part, &audio_part, &out), cancel).await?;
            check(cancel)?;

            remove_parts(&[video_part, audio_part]).await;
            Ok(out)
        }
        DownloadKind::Mp3 => {
            let audio = select_audio(&info).ok_or(EngineError::NoStream("audio"))?;
            let out = unique_path(&job.folder, &stem, "mp3");
            let audio_part = part_path(&out, "audio");

            wait_for_streams(&[audio], cancel).await?;
            emit(JobEventKind::Phase { phase: Phase::DownloadingAudio });
            created.push(audio_part.clone());
            backend.download(audio, &audio_part, cancel, &progress).await?;
            check(cancel)?;

            emit(JobEventKind::Phase { phase: Phase::Converting });
            created.push(out.clone());
            backend.ffmpeg(mp3_args(&audio_part, &out), cancel).await?;
            check(cancel)?;

            remove_parts(&[audio_part]).await;
            Ok(out)
        }
    }
}

/// `Song.mkv` → `Song.video.part` in the same folder.
fn part_path(out: &Path, which: &str) -> PathBuf {
    let stem = out.file_stem().unwrap_or_default().to_string_lossy();
    out.with_file_name(format!("{stem}.{which}.part"))
}

/// Extra wait after `available_at`, in case our clock is slightly ahead of YouTube's.
const AVAILABLE_MARGIN_SECS: f64 = 1.0;

/// How long to wait before the streams can be requested: YouTube answers 403 to requests made
/// before a format's `available_at` (yt-dlp sleeps for the same reason). `now` is Unix seconds.
fn stream_wait(formats: &[&RawFormat], now: f64) -> Duration {
    let latest = formats.iter().filter_map(|f| f.available_at).fold(f64::MIN, f64::max);
    if latest <= now {
        return Duration::ZERO;
    }
    Duration::from_secs_f64(latest - now + AVAILABLE_MARGIN_SECS)
}

/// Waits until every format is available, or returns `Cancelled`.
async fn wait_for_streams(formats: &[&RawFormat], cancel: &CancellationToken) -> Result<(), EngineError> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64();
    let wait = stream_wait(formats, now);
    if wait.is_zero() {
        return Ok(());
    }
    tokio::select! {
        _ = cancel.cancelled() => Err(EngineError::Cancelled),
        _ = tokio::time::sleep(wait) => Ok(()),
    }
}

fn check(cancel: &CancellationToken) -> Result<(), EngineError> {
    if cancel.is_cancelled() {
        Err(EngineError::Cancelled)
    } else {
        Ok(())
    }
}

async fn remove_parts(parts: &[PathBuf]) {
    for p in parts {
        let _ = tokio::fs::remove_file(p).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ffi::OsString;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use async_trait::async_trait;
    use tokio::sync::mpsc::unbounded_channel;
    use tokio_util::sync::CancellationToken;

    use crate::backend::Backend;
    use crate::download::ProgressFn;
    use crate::error::EngineError;
    use crate::select::to_video_info;
    use crate::types::{DownloadJob, DownloadKind, JobEvent, JobEventKind, JobId, Phase, VideoOption};
    use crate::ytdlp::{parse_info, RawFormat};
    use crate::Engine;

    const SYNTH: &str = include_str!("../tests/fixtures/synthetic.json");

    #[derive(Clone, Copy, PartialEq)]
    enum Stage {
        Download,
        Ffmpeg,
    }

    /// Stands in for yt-dlp, the network, and ffmpeg.
    struct FakeBackend {
        json: &'static str,
        /// Write the file, then wait for Cancel.
        block_in: Option<Stage>,
        /// Fail with HTTP 403.
        fail_in: Option<Stage>,
        download_calls: AtomicUsize,
    }

    impl FakeBackend {
        fn new() -> Self {
            Self { json: SYNTH, block_in: None, fail_in: None, download_calls: AtomicUsize::new(0) }
        }
        fn blocking(stage: Stage) -> Self {
            Self { block_in: Some(stage), ..Self::new() }
        }
        fn failing(stage: Stage) -> Self {
            Self { fail_in: Some(stage), ..Self::new() }
        }

        async fn finish(&self, stage: Stage, cancel: &CancellationToken) -> Result<(), EngineError> {
            if self.block_in == Some(stage) {
                cancel.cancelled().await;
                return Err(EngineError::Cancelled);
            }
            Ok(())
        }
    }

    #[async_trait]
    impl Backend for FakeBackend {
        async fn probe_json(&self, _url: &str) -> Result<String, EngineError> {
            Ok(self.json.to_string())
        }

        async fn download(
            &self,
            _fmt: &RawFormat,
            dest: &Path,
            cancel: &CancellationToken,
            progress: ProgressFn<'_>,
        ) -> Result<(), EngineError> {
            self.download_calls.fetch_add(1, Ordering::SeqCst);
            if self.fail_in == Some(Stage::Download) {
                return Err(EngineError::Http("HTTP 403".into()));
            }
            std::fs::write(dest, [0u8; 100])?;
            progress(100, Some(100));
            self.finish(Stage::Download, cancel).await
        }

        async fn ffmpeg(&self, args: Vec<OsString>, cancel: &CancellationToken) -> Result<(), EngineError> {
            if self.fail_in == Some(Stage::Ffmpeg) {
                return Err(EngineError::Ffmpeg("bad".into()));
            }
            std::fs::write(args.last().unwrap(), b"out")?;
            self.finish(Stage::Ffmpeg, cancel).await
        }
    }

    fn best_option() -> VideoOption {
        to_video_info(&parse_info(SYNTH).unwrap()).unwrap().video_options[0].clone() // 1080p60, vp9 → mkv
    }

    fn video() -> DownloadKind {
        DownloadKind::Video { option: best_option() }
    }

    /// Runs one job to completion (cancelling after `cancel_after` if given) and returns its events.
    async fn run(fake: &FakeBackend, kind: DownloadKind, folder: &Path, cancel_after: Option<Duration>) -> Vec<JobEvent> {
        let (tx, mut rx) = unbounded_channel();
        let cancel = CancellationToken::new();
        if let Some(d) = cancel_after {
            let c = cancel.clone();
            tokio::spawn(async move {
                tokio::time::sleep(d).await;
                c.cancel();
            });
        }
        let job = DownloadJob { url: "u".into(), kind, folder: folder.to_path_buf() };
        run_job(fake, JobId(1), job, cancel, &tx).await;
        drop(tx);
        let mut events = Vec::new();
        while let Some(e) = rx.recv().await {
            events.push(e);
        }
        events
    }

    fn phases(events: &[JobEvent]) -> Vec<Phase> {
        events
            .iter()
            .filter_map(|e| match e.kind {
                JobEventKind::Phase { phase } => Some(phase),
                _ => None,
            })
            .collect()
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn done_path(events: &[JobEvent]) -> PathBuf {
        match &events.last().unwrap().kind {
            JobEventKind::Done { path } => path.clone(),
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn video_job_event_order() {
        let dir = tempfile::tempdir().unwrap();
        let ev = run(&FakeBackend::new(), video(), dir.path(), None).await;
        assert_eq!(phases(&ev), [Phase::Probing, Phase::DownloadingVideo, Phase::DownloadingAudio, Phase::Merging]);
        assert!(done_path(&ev).ends_with("Synthetic Video.mkv"));
        assert_eq!(ev.iter().filter(|e| e.is_terminal()).count(), 1);
        assert!(ev.iter().all(|e| e.job_id == JobId(1)));
        assert!(ev.iter().any(|e| e.kind == JobEventKind::Progress { done: 100, total: Some(100) }));
    }

    #[tokio::test]
    async fn mp3_job_event_order() {
        let dir = tempfile::tempdir().unwrap();
        let ev = run(&FakeBackend::new(), DownloadKind::Mp3, dir.path(), None).await;
        assert_eq!(phases(&ev), [Phase::Probing, Phase::DownloadingAudio, Phase::Converting]);
        assert!(done_path(&ev).ends_with("Synthetic Video.mp3"));
    }

    #[tokio::test]
    async fn parts_removed_after_success() {
        let dir = tempfile::tempdir().unwrap();
        run(&FakeBackend::new(), video(), dir.path(), None).await;
        assert_eq!(files(dir.path()), ["Synthetic Video.mkv"]);
    }

    #[tokio::test]
    async fn existing_file_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Synthetic Video.mkv"), b"mine").unwrap();
        let ev = run(&FakeBackend::new(), video(), dir.path(), None).await;
        assert!(done_path(&ev).ends_with("Synthetic Video (1).mkv"));
        assert_eq!(std::fs::read(dir.path().join("Synthetic Video.mkv")).unwrap(), b"mine");
    }

    #[tokio::test]
    async fn cancel_during_download_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let ev = run(&FakeBackend::blocking(Stage::Download), video(), dir.path(), Some(Duration::from_millis(50))).await;
        assert_eq!(ev.last().unwrap().kind, JobEventKind::Cancelled);
        assert!(files(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn cancel_during_merge_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let ev = run(&FakeBackend::blocking(Stage::Ffmpeg), video(), dir.path(), Some(Duration::from_millis(50))).await;
        assert_eq!(ev.last().unwrap().kind, JobEventKind::Cancelled);
        assert_eq!(ev.iter().filter(|e| e.is_terminal()).count(), 1);
        assert!(files(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn failure_reports_message_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let ev = run(&FakeBackend::failing(Stage::Download), video(), dir.path(), None).await;
        assert!(matches!(&ev.last().unwrap().kind, JobEventKind::Failed { msg } if !msg.is_empty()));
        assert!(files(dir.path()).is_empty());
    }

    #[tokio::test]
    async fn ffmpeg_failure_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let ev = run(&FakeBackend::failing(Stage::Ffmpeg), DownloadKind::Mp3, dir.path(), None).await;
        assert!(matches!(&ev.last().unwrap().kind, JobEventKind::Failed { .. }));
        assert!(files(dir.path()).is_empty());
    }

    /// SYNTH with the best audio (251) not available for a long time.
    fn synth_not_available_yet() -> &'static str {
        Box::leak(
            SYNTH
                .replace(r#""format_id":"251""#, r#""format_id":"251","available_at":9999999999"#)
                .into_boxed_str(),
        )
    }

    #[test]
    fn wait_until_latest_available_at_plus_margin() {
        let info = parse_info(synth_not_available_yet()).unwrap();
        let audio = select_audio(&info).unwrap();
        let video = select_video(&info, 1080, 60).unwrap(); // no available_at
        assert_eq!(stream_wait(&[video], 100.0), Duration::ZERO);
        assert_eq!(stream_wait(&[video, audio], 9999999999.0 - 3.0), Duration::from_secs(4)); // 3 s + 1 s margin
        assert_eq!(stream_wait(&[audio], 9999999999.0 + 10.0), Duration::ZERO); // already available
    }

    #[tokio::test]
    async fn waits_for_streams_and_cancel_works_during_wait() {
        let dir = tempfile::tempdir().unwrap();
        let fake = FakeBackend { json: synth_not_available_yet(), ..FakeBackend::new() };
        let ev = run(&fake, DownloadKind::Mp3, dir.path(), Some(Duration::from_millis(50))).await;
        assert_eq!(ev.last().unwrap().kind, JobEventKind::Cancelled);
        assert_eq!(fake.download_calls.load(Ordering::SeqCst), 0, "must not request a stream before available_at");
    }

    #[tokio::test]
    async fn missing_folder_fails_before_download() {
        let dir = tempfile::tempdir().unwrap();
        let fake = FakeBackend::new();
        let ev = run(&fake, video(), &dir.path().join("gone"), None).await;
        assert!(matches!(&ev.last().unwrap().kind, JobEventKind::Failed { msg } if msg.contains("Choose another folder")));
        assert_eq!(fake.download_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn engine_start_returns_unique_ids_and_cancel_unknown_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::with_backend(FakeBackend::new());
        let job = DownloadJob { url: "u".into(), kind: DownloadKind::Mp3, folder: dir.path().to_path_buf() };
        let (tx, _rx) = unbounded_channel();
        let a = engine.start(job.clone(), tx.clone());
        let b = engine.start(job, tx);
        assert_ne!(a, b);
        engine.cancel(JobId(999));
    }

    #[tokio::test]
    async fn engine_cancel_stops_its_job() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::with_backend(FakeBackend::blocking(Stage::Download));
        let job = DownloadJob { url: "u".into(), kind: DownloadKind::Mp3, folder: dir.path().to_path_buf() };
        let (tx, mut rx) = unbounded_channel();
        let id = engine.start(job, tx);
        tokio::time::sleep(Duration::from_millis(50)).await;
        engine.cancel(id);
        let last = tokio::time::timeout(Duration::from_secs(2), async {
            let mut last = None;
            while let Some(e) = rx.recv().await {
                last = Some(e);
            }
            last
        })
        .await
        .expect("job should end after cancel");
        assert_eq!(last.unwrap().kind, JobEventKind::Cancelled);
    }

    #[tokio::test]
    async fn engine_probe_returns_video_info() {
        let engine = Engine::with_backend(FakeBackend::new());
        let info = engine.probe("u").await.unwrap();
        assert_eq!(info.title, "Synthetic Video");
        assert_eq!(info.video_options.len(), 4);
    }
}
