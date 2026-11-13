//! The remembered download folder.

use std::path::PathBuf;

/// File the store plugin keeps settings in, and the key for the last folder.
pub const STORE_FILE: &str = "settings.json";
pub const LAST_FOLDER: &str = "lastFolder";

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
