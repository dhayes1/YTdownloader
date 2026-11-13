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
