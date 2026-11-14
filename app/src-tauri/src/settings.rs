//! The remembered download folder.

use std::path::PathBuf;

use engine::{DownloadKind, Quality};

/// File the store plugin keeps settings in, and the key for the last folder.
pub const STORE_FILE: &str = "settings.json";
pub const LAST_FOLDER: &str = "lastFolder";
/// Store key for the remembered download choice (video quality or MP3).
pub const CHOICE: &str = "choice";
/// Store key for the last video quality picked, so MP3 -> Video can restore it.
pub const VIDEO_QUALITY: &str = "videoQuality";

/// The saved video quality if it deserializes; otherwise Best.
pub fn resolve_video_quality(saved: Option<serde_json::Value>) -> Quality {
    saved.and_then(|v| serde_json::from_value(v).ok()).unwrap_or(Quality::Best)
}

/// The saved choice if it deserializes; otherwise best-quality video.
pub fn resolve_choice(saved: Option<serde_json::Value>) -> DownloadKind {
    saved
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or(DownloadKind::Video { quality: Quality::Best })
}

/// The saved folder if it still exists (a drive may be unplugged or the folder deleted);
/// otherwise the Downloads folder.
pub fn resolve_folder(saved: Option<String>, downloads: PathBuf) -> PathBuf {
    match saved.map(PathBuf::from) {
        Some(p) if p.is_dir() => p,
        _ => downloads,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn resolve_folder_uses_saved() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(resolve_folder(Some(d.path().to_string_lossy().into()), "C:\\dl".into()), d.path());
    }

    #[test]
    fn resolve_folder_defaults_to_downloads() {
        assert_eq!(resolve_folder(None, "C:\\dl".into()), PathBuf::from("C:\\dl"));
    }

    #[test]
    fn resolve_folder_falls_back_when_missing() {
        assert_eq!(resolve_folder(Some("Z:\\no\\such\\dir".into()), "C:\\dl".into()), PathBuf::from("C:\\dl"));
    }
}

#[cfg(test)]
mod choice_tests {
    use super::*;
    use engine::{DownloadKind, Quality};

    #[test] fn resolve_choice_defaults_to_video_best() { assert_eq!(resolve_choice(None), DownloadKind::Video { quality: Quality::Best }); }
    #[test] fn resolve_choice_uses_saved() { assert_eq!(resolve_choice(Some(serde_json::json!({"type":"mp3"}))), DownloadKind::Mp3); }
    #[test] fn resolve_choice_ignores_garbage() { assert_eq!(resolve_choice(Some(serde_json::json!("nonsense"))), DownloadKind::Video { quality: Quality::Best }); }
}

#[cfg(test)]
mod video_quality_tests {
    use super::*;

    #[test] fn resolve_video_quality_defaults_to_best() { assert_eq!(resolve_video_quality(None), Quality::Best); }
    #[test] fn resolve_video_quality_uses_saved() { assert_eq!(resolve_video_quality(Some(serde_json::json!({"type":"upTo","height":720}))), Quality::UpTo { height: 720 }); }
    #[test] fn resolve_video_quality_ignores_garbage() { assert_eq!(resolve_video_quality(Some(serde_json::json!(5))), Quality::Best); }
}
