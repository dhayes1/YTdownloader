//! Builds the resolution list and picks which streams to download.
//!
//! Only separate video-only and audio-only streams over plain https are used: combined
//! low-quality streams, storyboards, and HLS playlists are skipped. yt-dlp lists formats
//! worst → best, so "best" always means the last match.

use crate::error::EngineError;
use crate::types::{VideoInfo, VideoOption};
use crate::ytdlp::{RawFormat, RawInfo};

/// The file type of the merged video.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    Mp4,
    Mkv,
}

impl Container {
    pub fn ext(&self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::Mkv => "mkv",
        }
    }
}

/// Groups frame rates: high frame rates (over 30) keep their rounded value, everything
/// else is 0. So 59.94 → 60, and 24, 25, and 30 all count as "normal".
pub fn fps_class(fps: Option<f64>) -> u32 {
    match fps {
        Some(f) if f > 30.0 => f.round() as u32,
        _ => 0,
    }
}

fn usable(f: &RawFormat) -> bool {
    f.protocol.as_deref() == Some("https") && f.url.is_some()
}

fn is_none_codec(codec: &Option<String>) -> bool {
    codec.as_deref() == Some("none")
}

fn has_codec(codec: &Option<String>) -> bool {
    matches!(codec.as_deref(), Some(c) if c != "none")
}

fn video_only(info: &RawInfo) -> impl Iterator<Item = &RawFormat> {
    info.formats
        .iter()
        .filter(|f| usable(f) && is_none_codec(&f.acodec) && has_codec(&f.vcodec) && f.height.is_some())
}

fn size(f: &RawFormat) -> Option<u64> {
    f.filesize.or(f.filesize_approx)
}

/// One option per distinct (height, fps class), highest first.
pub fn video_options(info: &RawInfo) -> Vec<VideoOption> {
    // (height, fps class) → best format for it. Later formats overwrite earlier ones,
    // which leaves the best one, because yt-dlp lists worst → best.
    let mut best: Vec<((u32, u32), &RawFormat)> = Vec::new();
    for f in video_only(info) {
        let key = (f.height.unwrap_or(0), fps_class(f.fps));
        match best.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = f,
            None => best.push((key, f)),
        }
    }
    best.sort_by(|a, b| b.0.cmp(&a.0)); // height desc, then fps class desc

    let audio_size = select_audio(info).and_then(size);
    best.into_iter()
        .map(|((height, class), f)| VideoOption {
            label: if class > 0 { format!("{height}p{class}") } else { format!("{height}p") },
            height,
            fps: f.fps.map(|x| x.round() as u32).unwrap_or(0),
            codec: f.vcodec.clone().unwrap_or_default(),
            est_size: match (size(f), audio_size) {
                (Some(v), Some(a)) => Some(v + a),
                _ => None,
            },
        })
        .collect()
}

/// The best video-only stream at this height and fps class.
pub fn select_video(info: &RawInfo, height: u32, fps: u32) -> Option<&RawFormat> {
    video_only(info)
        .filter(|f| f.height == Some(height) && fps_class(f.fps) == fps)
        .last()
}

/// The best audio-only stream.
pub fn select_audio(info: &RawInfo) -> Option<&RawFormat> {
    info.formats
        .iter()
        .filter(|f| usable(f) && is_none_codec(&f.vcodec) && has_codec(&f.acodec))
        .last()
}

/// MP4 only when the video is H.264 and the audio is AAC; MKV for everything else.
pub fn container_for(vcodec: &str, acodec: &str) -> Container {
    if vcodec.starts_with("avc1") && acodec.starts_with("mp4a") {
        Container::Mp4
    } else {
        Container::Mkv
    }
}

/// What the UI shows after a probe.
pub fn to_video_info(info: &RawInfo) -> Result<VideoInfo, EngineError> {
    let video_options = video_options(info);
    let has_audio = select_audio(info).is_some();
    if video_options.is_empty() && !has_audio {
        return Err(EngineError::NoStream("video or audio"));
    }
    Ok(VideoInfo {
        title: info.title.clone(),
        duration_secs: info.duration,
        thumbnail_url: info.thumbnail.clone(),
        video_options,
        has_audio,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::EngineError;
    use crate::ytdlp::{parse_info, RawInfo};

    const SYNTH: &str = include_str!("../tests/fixtures/synthetic.json");
    const COMBINED: &str = include_str!("../tests/fixtures/combined_only.json");

    fn info(s: &str) -> RawInfo {
        parse_info(s).unwrap()
    }

    #[test]
    fn options_from_synthetic() {
        let o = video_options(&info(SYNTH));
        let labels: Vec<_> = o.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, ["1080p60", "1080p", "720p60", "720p"]);
        assert_eq!(o[0].fps, 60);
        assert_eq!(o[0].codec, "vp9");
        assert_eq!(o[0].est_size, Some(79_200_000)); // 303 approx + 251
        assert_eq!(o[1].est_size, Some(49_200_000)); // 248 + 251
    }

    #[test]
    fn picks_best_video_per_option() {
        let i = info(SYNTH);
        assert_eq!(select_video(&i, 1080, 60).unwrap().format_id, "303");
        assert_eq!(select_video(&i, 720, 0).unwrap().format_id, "247");
        assert!(select_video(&i, 480, 0).is_none());
    }

    #[test]
    fn picks_best_audio() {
        assert_eq!(select_audio(&info(SYNTH)).unwrap().format_id, "251");
    }

    #[test]
    fn ignores_combined_storyboard_and_hls() {
        assert!(video_options(&info(SYNTH)).iter().all(|o| o.height != 360));
    }

    #[test]
    fn fps_classes() {
        assert_eq!(fps_class(Some(59.94)), 60);
        assert_eq!(fps_class(Some(30.0)), 0);
        assert_eq!(fps_class(Some(25.0)), 0);
        assert_eq!(fps_class(None), 0);
    }

    #[test]
    fn container_rules() {
        assert_eq!(container_for("avc1.640028", "mp4a.40.2"), Container::Mp4);
        assert_eq!(container_for("avc1.640028", "opus"), Container::Mkv);
        assert_eq!(container_for("vp9", "mp4a.40.2"), Container::Mkv);
        assert_eq!(container_for("av01.0.08M.08", "opus"), Container::Mkv);
    }

    #[test]
    fn video_info_from_synthetic() {
        let v = to_video_info(&info(SYNTH)).unwrap();
        assert_eq!(v.title, "Synthetic Video");
        assert_eq!(v.duration_secs, Some(754.0));
        assert!(v.has_audio);
        assert_eq!(v.video_options.len(), 4);
    }

    #[test]
    fn combined_only_has_nothing_usable() {
        assert!(matches!(to_video_info(&info(COMBINED)), Err(EngineError::NoStream(_))));
    }
}
