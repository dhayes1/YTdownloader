//! Download engine: probes videos with yt-dlp, downloads streams, runs ffmpeg.
//! No Tauri dependency — the UI talks to it through the app crate.

pub mod download;
pub mod error;
pub mod naming;
pub mod select;
pub mod tools;
pub mod types;
pub mod ytdlp;

pub use error::*;
pub use types::*;
