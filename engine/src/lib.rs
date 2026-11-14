//! Download engine: probes videos with yt-dlp, downloads streams, runs ffmpeg.
//! No Tauri dependency — the UI talks to it through the app crate.

pub mod backend;
pub mod download;
pub mod error;
pub mod job;
pub mod naming;
pub mod select;
pub mod tools;
pub mod types;
pub mod ytdlp;

pub use backend::{Backend, SystemBackend};
pub use error::*;
pub use tools::missing_tools;
pub use types::*;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;

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
        select::to_probed(&ytdlp::parse_info(&json)?)
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
