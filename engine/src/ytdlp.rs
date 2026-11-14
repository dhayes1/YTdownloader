//! The raw JSON that `yt-dlp -J` prints, and how to turn yt-dlp failures into `EngineError`s.

use std::collections::HashMap;

use serde::Deserialize;

use crate::error::{EngineError, UnsupportedReason};

/// The parts of yt-dlp's JSON the engine uses. Unknown fields are ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct RawInfo {
    #[serde(rename = "_type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub title: String,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
    #[serde(default)]
    pub formats: Vec<RawFormat>,
    pub is_live: Option<bool>,
    pub live_status: Option<String>,
    pub id: Option<String>,
    pub webpage_url: Option<String>,
    #[serde(default)]
    pub entries: Vec<RawEntry>,
}

/// One item of a flat playlist listing.
#[derive(Debug, Clone, Deserialize)]
pub struct RawEntry {
    pub id: Option<String>,
    pub url: Option<String>,
    pub title: Option<String>,
    pub playlist_index: Option<u32>,
}

/// One stream yt-dlp found. yt-dlp lists formats from worst to best.
#[derive(Debug, Clone, Deserialize)]
pub struct RawFormat {
    pub format_id: String,
    pub url: Option<String>,
    pub protocol: Option<String>,
    pub ext: Option<String>,
    pub vcodec: Option<String>,
    pub acodec: Option<String>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub filesize: Option<u64>,
    pub filesize_approx: Option<u64>,
    #[serde(default)]
    pub http_headers: HashMap<String, String>,
    /// Unix time (seconds) before which YouTube refuses the stream with 403.
    pub available_at: Option<f64>,
}

/// Parses `yt-dlp -J` output. Live streams, channels, and Mixes are rejected as `Unsupported`.
pub fn parse_info(json: &str) -> Result<RawInfo, EngineError> {
    let info: RawInfo = serde_json::from_str(json)
        .map_err(|e| EngineError::YtDlp(format!("could not read yt-dlp output: {e}")))?;

    let url = info.webpage_url.as_deref().unwrap_or("");
    if ["/@", "/channel/", "/c/", "/user/"].iter().any(|p| url.contains(p)) {
        return Err(EngineError::Unsupported(UnsupportedReason::Channel));
    }
    if info.kind.as_deref() == Some("playlist")
        && info.id.as_deref().is_some_and(|id| id.starts_with("RD"))
    {
        return Err(EngineError::Unsupported(UnsupportedReason::Mix));
    }
    let live = info.is_live == Some(true)
        || matches!(info.live_status.as_deref(), Some("is_live" | "is_upcoming"));
    if live {
        return Err(EngineError::Unsupported(UnsupportedReason::Live));
    }
    Ok(info)
}

/// Turns yt-dlp's error output into a specific error when it recognizes the cause;
/// otherwise keeps the last line of output for the technical message.
pub fn classify_ytdlp_error(stderr: &str) -> EngineError {
    let lower = stderr.to_lowercase();
    if lower.contains("confirm your age") || lower.contains("age-restricted") {
        return EngineError::Unsupported(UnsupportedReason::AgeRestricted);
    }
    if lower.contains("private video") {
        return EngineError::Unsupported(UnsupportedReason::Private);
    }
    if lower.contains("members-only") || lower.contains("join this channel") {
        return EngineError::Unsupported(UnsupportedReason::MembersOnly);
    }
    let last = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .last()
        .unwrap_or("unknown error");
    EngineError::YtDlp(last.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::{EngineError, UnsupportedReason};

    const SYNTH: &str = include_str!("../tests/fixtures/synthetic.json");
    const LIVE: &str = include_str!("../tests/fixtures/live.json");
    const FLAT: &str = include_str!("../tests/fixtures/flat_playlist.json");
    const CHANNEL: &str = include_str!("../tests/fixtures/channel.json");
    const MIX: &str = include_str!("../tests/fixtures/mix.json");

    #[test]
    fn parses_synthetic() {
        let i = parse_info(SYNTH).unwrap();
        assert_eq!(i.title, "Synthetic Video");
        assert_eq!(i.formats.len(), 13);
        assert_eq!(i.formats[3].http_headers["User-Agent"], "UA");
        assert_eq!(i.formats[3].available_at, None);
    }

    #[test]
    fn parses_available_at() {
        let json = r#"{"title":"t","formats":[{"format_id":"251","available_at":1794622619}]}"#;
        assert_eq!(parse_info(json).unwrap().formats[0].available_at, Some(1794622619.0));
    }

    #[test]
    fn rejects_live() {
        assert!(matches!(parse_info(LIVE), Err(EngineError::Unsupported(UnsupportedReason::Live))));
    }

    #[test] fn accepts_playlist() { let i = parse_info(FLAT).unwrap(); assert_eq!(i.kind.as_deref(), Some("playlist")); assert_eq!(i.entries.len(), 3); }
    #[test] fn rejects_channel() { assert!(matches!(parse_info(CHANNEL), Err(EngineError::Unsupported(UnsupportedReason::Channel)))); }
    #[test] fn rejects_mix() { assert!(matches!(parse_info(MIX), Err(EngineError::Unsupported(UnsupportedReason::Mix)))); }

    #[test]
    fn invalid_json_is_ytdlp_error() {
        assert!(matches!(parse_info("not json"), Err(EngineError::YtDlp(_))));
    }

    #[test]
    fn classify_age() {
        assert!(matches!(
            classify_ytdlp_error("ERROR: [youtube] abc: Sign in to confirm your age. This video may be inappropriate for some users."),
            EngineError::Unsupported(UnsupportedReason::AgeRestricted)
        ));
    }

    #[test]
    fn classify_private() {
        assert!(matches!(
            classify_ytdlp_error("ERROR: [youtube] abc: Private video. Sign in if you've been granted access to this video"),
            EngineError::Unsupported(UnsupportedReason::Private)
        ));
    }

    #[test]
    fn classify_members() {
        assert!(matches!(
            classify_ytdlp_error("ERROR: [youtube] abc: Join this channel to get access to members-only content like this video"),
            EngineError::Unsupported(UnsupportedReason::MembersOnly)
        ));
    }

    #[test]
    fn classify_other_keeps_last_line() {
        assert_eq!(
            classify_ytdlp_error("WARNING: x\nERROR: something broke\n").to_string(),
            "yt-dlp failed: ERROR: something broke"
        );
    }
}
