//! Turns a video title into a safe Windows file name, and finds a name that isn't taken.

use std::path::{Path, PathBuf};

const MAX_CHARS: usize = 200;
const ILLEGAL: [char; 9] = ['<', '>', ':', '"', '/', '\\', '|', '?', '*'];
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Removes characters Windows doesn't allow in file names, trims leading/trailing spaces and
/// trailing dots, avoids reserved device names, and caps the length at 200 characters.
pub fn sanitize_filename(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .filter(|c| !ILLEGAL.contains(c) && !c.is_control())
        .take(MAX_CHARS)
        .collect();
    let trimmed = cleaned.trim_start().trim_end_matches(['.', ' ']);

    if trimmed.is_empty() {
        return "download".to_string();
    }
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(trimmed)) {
        return format!("{trimmed}_");
    }
    trimmed.to_string()
}

/// `folder/stem.ext`, or `stem (1).ext`, `stem (2).ext`, … if that already exists.
pub fn unique_path(folder: &Path, stem: &str, ext: &str) -> PathBuf {
    let first = folder.join(format!("{stem}.{ext}"));
    if !first.exists() {
        return first;
    }
    (1..)
        .map(|n| folder.join(format!("{stem} ({n}).{ext}")))
        .find(|p| !p.exists())
        .expect("an unused name always exists")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_illegal() {
        assert_eq!(sanitize_filename(r#"a<b>c:d"e/f\g|h?i*j"#), "abcdefghij");
    }

    #[test]
    fn strips_controls() {
        assert_eq!(sanitize_filename("a\u{7}b"), "ab");
    }

    #[test]
    fn trims_dots_and_spaces() {
        assert_eq!(sanitize_filename("  Song. . "), "Song");
    }

    #[test]
    fn reserved_names() {
        assert_eq!(sanitize_filename("CON"), "CON_");
        assert_eq!(sanitize_filename("lpt1"), "lpt1_");
    }

    #[test]
    fn empty_becomes_download() {
        assert_eq!(sanitize_filename("???"), "download");
    }

    #[test]
    fn keeps_unicode() {
        assert_eq!(sanitize_filename("🎵 Café Mix"), "🎵 Café Mix");
    }

    #[test]
    fn truncates_long() {
        assert!(sanitize_filename(&"é".repeat(300)).chars().count() <= 200);
    }

    #[test]
    fn unique_paths() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(unique_path(d.path(), "Song", "mp4"), d.path().join("Song.mp4"));
        std::fs::write(d.path().join("Song.mp4"), b"").unwrap();
        assert_eq!(unique_path(d.path(), "Song", "mp4"), d.path().join("Song (1).mp4"));
        std::fs::write(d.path().join("Song (1).mp4"), b"").unwrap();
        assert_eq!(unique_path(d.path(), "Song", "mp4"), d.path().join("Song (2).mp4"));
    }
}
