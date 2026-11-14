//! Real-video tests. They need the network, yt-dlp, ffmpeg, and Deno, so they're ignored by
//! default. Run with:
//!   $env:YTDL_TEST_URL="<url>"; cargo test -p engine --test network -- --ignored
//! The playlist tests read `YTDL_TEST_PLAYLIST` instead:
//!   $env:YTDL_TEST_PLAYLIST="<playlist url>"; cargo test -p engine --test network -- --ignored real_playlist real_two

use std::sync::Arc;
use std::time::Duration;

use engine::{
    DownloadJob, DownloadKind, Engine, EntryState, JobEventKind, PlaylistInfo, Probed, Quality, Queue, QueueEvent,
    QueueStatus, SystemBackend,
};
use tokio::sync::mpsc::unbounded_channel;

fn test_url() -> String {
    std::env::var("YTDL_TEST_URL").expect("set YTDL_TEST_URL to a public video you may download")
}

fn test_playlist() -> String {
    std::env::var("YTDL_TEST_PLAYLIST").expect("set YTDL_TEST_PLAYLIST to a public playlist you may download")
}

async fn probe_playlist() -> PlaylistInfo {
    match Engine::new().probe(&test_playlist()).await.unwrap() {
        Probed::Playlist { playlist } => playlist,
        other => panic!("expected a playlist, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs network and yt-dlp"]
async fn real_playlist_listing() {
    let playlist = probe_playlist().await;
    assert!(!playlist.entries.is_empty());
}

/// Queues the first two entries of a real playlist as MP3s.
#[tokio::test]
#[ignore = "needs network, yt-dlp, ffmpeg, and Deno"]
async fn real_two_mp3_queue() {
    let dir = tempfile::tempdir().unwrap();
    let mut playlist = probe_playlist().await;
    assert!(playlist.entries.len() >= 2, "the test playlist needs at least two entries");
    playlist.entries.truncate(2);

    let (tx, mut rx) = unbounded_channel();
    let queue = Queue::new(Arc::new(SystemBackend::default()), tx);
    queue.set_folder(dir.path().to_path_buf());
    queue.set_choice(DownloadKind::Mp3);
    queue.add_playlist(&playlist);
    queue.start();

    tokio::time::timeout(Duration::from_secs(600), async {
        loop {
            match rx.recv().await.expect("queue events ended") {
                QueueEvent::Status { status: QueueStatus::Idle | QueueStatus::Paused } => return,
                QueueEvent::State { id, state } => println!("{id:?}: {state:?}"),
                _ => {}
            }
        }
    })
    .await
    .expect("queue took over 10 minutes");

    let entries = queue.entries();
    assert_eq!(entries.len(), 2);
    let sub = dir.path().join(engine::naming::sanitize_filename(&playlist.title));
    for e in &entries {
        let EntryState::Done { path } = &e.state else { panic!("expected Done, got {:?}", e.state) };
        assert_eq!(path.parent().unwrap(), sub);
        assert_eq!(path.extension().unwrap(), "mp3");
        assert!(path.file_name().unwrap().to_string_lossy().starts_with(e.prefix.as_deref().unwrap()));
        assert!(std::fs::metadata(path).unwrap().len() > 0);
    }
    let mp3s = std::fs::read_dir(&sub).unwrap().filter(|f| f.as_ref().unwrap().path().extension().is_some_and(|x| x == "mp3")).count();
    assert_eq!(mp3s, 2);
}

#[tokio::test]
#[ignore = "needs network, yt-dlp, ffmpeg, and Deno"]
async fn real_probe_lists_options() {
    let Probed::Video { info } = Engine::new().probe(&test_url()).await.unwrap() else { panic!("expected a video") };
    assert!(!info.title.is_empty());
    assert!(!info.video_options.is_empty());
}

/// Cancel a real video download partway, then start the same download again (Donny's
/// manual-test report: Download after Cancel showed an error).
#[tokio::test]
#[ignore = "needs network, yt-dlp, ffmpeg, and Deno"]
async fn real_cancel_then_restart() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new();
    let job = DownloadJob {
        url: test_url(),
        kind: DownloadKind::Video { quality: Quality::Best },
        folder: dir.path().to_path_buf(),
        subfolder: None,
        prefix: None,
    };

    // First run: cancel partway through the video stream.
    let (tx, mut rx) = unbounded_channel();
    let id = engine.start(job.clone(), tx);
    let mut first = Vec::new();
    while let Some(e) = rx.recv().await {
        if matches!(e.kind, JobEventKind::Progress { done, .. } if done > 12_000_000) {
            engine.cancel(id);
        }
        let end = e.is_terminal();
        first.push(e);
        if end {
            break;
        }
    }
    println!("first run ended with: {:?}", first.last().unwrap().kind);
    println!("files after cancel: {:?}", std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect::<Vec<_>>());

    // Second run, same engine, same job.
    let (tx, mut rx) = unbounded_channel();
    engine.start(job, tx);
    let mut second = Vec::new();
    while let Some(e) = rx.recv().await {
        let end = e.is_terminal();
        second.push(e);
        if end {
            break;
        }
    }
    println!("second run events: {:?}", second.iter().map(|e| &e.kind).collect::<Vec<_>>());
    assert!(matches!(second.last().unwrap().kind, JobEventKind::Done { .. }), "second run: {:?}", second.last().unwrap().kind);
}

#[tokio::test]
#[ignore = "needs network, yt-dlp, ffmpeg, and Deno"]
async fn real_mp3_download() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::new();
    let (tx, mut rx) = unbounded_channel();
    engine.start(
        DownloadJob { url: test_url(), kind: DownloadKind::Mp3, folder: dir.path().to_path_buf(), subfolder: None, prefix: None },
        tx,
    );

    let last = tokio::time::timeout(Duration::from_secs(300), async {
        loop {
            let e = rx.recv().await.expect("job ended without a terminal event");
            if e.is_terminal() {
                return e;
            }
        }
    })
    .await
    .expect("download took over 5 minutes");

    match last.kind {
        JobEventKind::Done { path } => {
            assert_eq!(path.extension().unwrap(), "mp3");
            assert!(std::fs::metadata(&path).unwrap().len() > 0);
        }
        other => panic!("expected Done, got {other:?}"),
    }
}
