//! Download engine: probes videos with yt-dlp, downloads streams, runs ffmpeg.
//! No Tauri dependency — the UI talks to it through the app crate.

pub mod error;
pub mod types;
pub mod ytdlp;

pub use error::*;
pub use types::*;
