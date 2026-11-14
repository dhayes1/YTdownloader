//! Real-video tests. They need the network, yt-dlp, ffmpeg, and Deno, so they're ignored by
//! default. Run with:
//!   $env:YTDL_TEST_URL="<url>"; cargo test -p engine --test network -- --ignored

use std::time::Duration;

use engine::{DownloadJob, DownloadKind, Engine, JobEventKind};
use tokio::sync::mpsc::unbounded_channel;

fn test_url() -> String {
    std::env::var("YTDL_TEST_URL").expect("set YTDL_TEST_URL to a public video you may download")
}

#[tokio::test]
#[ignore = "needs network, yt-dlp, ffmpeg, and Deno"]
async fn real_probe_lists_options() {
    let info = Engine::new().probe(&test_url()).await.unwrap();
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
    let info = engine.probe(&test_url()).await.unwrap();
    let job = DownloadJob {
        url: test_url(),
        kind: DownloadKind::Video { option: info.video_options[0].clone() },
        folder: dir.path().to_path_buf(),
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
        DownloadJob { url: test_url(), kind: DownloadKind::Mp3, folder: dir.path().to_path_buf() },
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
