//! Test doubles shared by the engine's unit tests.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::backend::Backend;
use crate::download::ProgressFn;
use crate::error::{EngineError, UnsupportedReason};
use crate::ytdlp::RawFormat;

pub(crate) const SYNTH: &str = include_str!("../tests/fixtures/synthetic.json");

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Stage {
    Download,
    Ffmpeg,
}

/// Stands in for yt-dlp, the network, and ffmpeg.
pub(crate) struct FakeBackend {
    pub(crate) json: &'static str,
    /// Write the file, then wait for Cancel.
    pub(crate) block_in: Option<Stage>,
    /// Fail with HTTP 403.
    pub(crate) fail_in: Option<Stage>,
    /// Probing one of these URLs fails as a private video; every other URL returns `json`.
    pub(crate) private_urls: HashSet<String>,
    pub(crate) download_calls: AtomicUsize,
}

impl FakeBackend {
    pub(crate) fn new() -> Self {
        Self {
            json: SYNTH,
            block_in: None,
            fail_in: None,
            private_urls: HashSet::new(),
            download_calls: AtomicUsize::new(0),
        }
    }
    pub(crate) fn blocking(stage: Stage) -> Self {
        Self { block_in: Some(stage), ..Self::new() }
    }
    pub(crate) fn failing(stage: Stage) -> Self {
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
    async fn probe_json(&self, url: &str) -> Result<String, EngineError> {
        if self.private_urls.contains(url) {
            return Err(EngineError::Unsupported(UnsupportedReason::Private));
        }
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
