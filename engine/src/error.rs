//! Everything that can go wrong in the engine, and the plain-language message the UI shows.

use std::path::PathBuf;

use thiserror::Error;

/// Why a video can't be downloaded at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnsupportedReason {
    AgeRestricted,
    Private,
    MembersOnly,
    Live,
    Playlist,
}

/// An engine error. `Display` (from `#[error]`) is the technical text for logs;
/// `user_message()` is what the user sees.
#[derive(Debug, Error)]
pub enum EngineError {
    #[error("{0} is not installed or not on PATH")]
    ToolMissing(String),
    #[error("unsupported video: {0:?}")]
    Unsupported(UnsupportedReason),
    #[error("yt-dlp failed: {0}")]
    YtDlp(String),
    #[error("no suitable {0} stream")]
    NoStream(&'static str),
    #[error("download failed: {0}")]
    Http(String),
    #[error("ffmpeg failed: {0}")]
    Ffmpeg(String),
    #[error("folder not found: {}", .0.display())]
    Folder(PathBuf),
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("cancelled")]
    Cancelled,
}

impl EngineError {
    /// Plain-language message with a suggested next step.
    pub fn user_message(&self) -> String {
        match self {
            EngineError::ToolMissing(tool) => format!(
                "{tool} isn't installed or isn't on PATH. Install it, then restart the app."
            ),
            EngineError::Unsupported(reason) => match reason {
                UnsupportedReason::AgeRestricted => {
                    "This video is age-restricted, so it can't be downloaded.".into()
                }
                UnsupportedReason::Private => {
                    "This video is private, so it can't be downloaded.".into()
                }
                UnsupportedReason::MembersOnly => {
                    "This video is for channel members only, so it can't be downloaded.".into()
                }
                UnsupportedReason::Live => {
                    "Live streams can't be downloaded. Try again after the stream has ended."
                        .into()
                }
                UnsupportedReason::Playlist => {
                    "That's a playlist link. Paste the link to a single video instead.".into()
                }
            },
            EngineError::YtDlp(_) => "yt-dlp couldn't read this video. Check the link, then \
                 update yt-dlp by running `yt-dlp -U` and try again."
                .into(),
            EngineError::NoStream(what) => {
                format!("This video has no {what} stream the app can download.")
            }
            EngineError::Http(detail) if detail == "HTTP 403" => {
                "YouTube refused the download. Try again in a moment; if it keeps happening, \
                 update yt-dlp by running `yt-dlp -U`."
                    .into()
            }
            EngineError::Http(_) => {
                "The download stopped before it finished. Check your connection and try again."
                    .into()
            }
            EngineError::Ffmpeg(_) => {
                "ffmpeg couldn't build the final file. Try again, or update ffmpeg.".into()
            }
            EngineError::Folder(path) => format!(
                "The folder {} no longer exists. Choose another folder.",
                path.display()
            ),
            EngineError::Io(e) => {
                format!("The app couldn't write the file ({e}). Check the folder and free space, then try again.")
            }
            EngineError::Cancelled => "Download cancelled.".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_403_message_is_not_about_the_connection() {
        let m = EngineError::Http("HTTP 403".into()).user_message();
        assert!(m.contains("YouTube refused"), "{m}");
        assert!(m.contains("yt-dlp -U"), "{m}");
        assert!(EngineError::Http("connection ended early".into()).user_message().contains("connection"));
    }

    #[test]
    fn network_error_mentioning_403_is_still_a_connection_problem() {
        // A dropped connection's text can contain digits like "403" (e.g. from a stream URL).
        let m = EngineError::Http("error decoding response body: connection reset, clen=4031".into()).user_message();
        assert!(m.contains("connection"), "{m}");
        assert!(!m.contains("YouTube refused"), "{m}");
    }

    #[test]
    fn ytdlp_message_suggests_update() {
        assert!(EngineError::YtDlp("x".into()).user_message().contains("yt-dlp -U"));
    }

    #[test]
    fn folder_message_suggests_new_folder() {
        assert!(EngineError::Folder("D:\\gone".into()).user_message().contains("Choose another folder"));
    }

    #[test]
    fn age_message_is_specific() {
        assert!(EngineError::Unsupported(UnsupportedReason::AgeRestricted)
            .user_message()
            .to_lowercase()
            .contains("age-restricted"));
    }
}
