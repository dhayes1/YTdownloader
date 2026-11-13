//! The engine's only contact with the outside world. Jobs call these three methods, so tests
//! swap in a fake and V2 can change how downloads happen without touching the job logic.

use std::ffi::OsString;
use std::path::Path;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::download::{download_ranged, ProgressFn, CHUNK_SIZE};
use crate::error::EngineError;
use crate::tools::{run_ffmpeg, run_ytdlp_probe};
use crate::ytdlp::RawFormat;

#[async_trait]
pub trait Backend: Send + Sync + 'static {
    /// The raw `yt-dlp -J` JSON for `url`.
    async fn probe_json(&self, url: &str) -> Result<String, EngineError>;

    /// Downloads one stream into `dest`.
    async fn download(
        &self,
        fmt: &RawFormat,
        dest: &Path,
        cancel: &CancellationToken,
        progress: ProgressFn<'_>,
    ) -> Result<(), EngineError>;

    /// Runs ffmpeg with `args`; the last argument is the output file.
    async fn ffmpeg(&self, args: Vec<OsString>, cancel: &CancellationToken) -> Result<(), EngineError>;
}

/// The real backend: yt-dlp, reqwest, and ffmpeg.
#[derive(Default)]
pub struct SystemBackend {
    client: reqwest::Client,
}

#[async_trait]
impl Backend for SystemBackend {
    async fn probe_json(&self, url: &str) -> Result<String, EngineError> {
        run_ytdlp_probe(url).await
    }

    async fn download(
        &self,
        fmt: &RawFormat,
        dest: &Path,
        cancel: &CancellationToken,
        progress: ProgressFn<'_>,
    ) -> Result<(), EngineError> {
        let url = fmt.url.as_deref().ok_or(EngineError::NoStream("stream URL"))?;
        download_ranged(&self.client, url, &fmt.http_headers, CHUNK_SIZE, dest, cancel, progress)
            .await
            .map(|_| ())
    }

    async fn ffmpeg(&self, args: Vec<OsString>, cancel: &CancellationToken) -> Result<(), EngineError> {
        run_ffmpeg(args, cancel).await
    }
}
