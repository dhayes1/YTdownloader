//! Data the engine shares with the app and frontend. The serde attributes fix the JSON shape
//! the TypeScript side mirrors (camelCase fields, `type` tags on enums).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// What the UI shows after probing a URL.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoInfo {
    pub title: String,
    pub duration_secs: Option<f64>,
    pub thumbnail_url: Option<String>,
    pub video_options: Vec<VideoOption>,
    pub has_audio: bool,
}

/// One entry in the resolution list, for example "1080p60".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoOption {
    pub label: String,
    pub height: u32,
    pub fps: u32,
    pub codec: String,
    pub est_size: Option<u64>,
}

/// Video at a chosen resolution, or audio converted to MP3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DownloadKind {
    Video { option: VideoOption },
    Mp3,
}

/// Which video to pick: the best available, the best at or below a height, or the smallest.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Quality {
    Best,
    UpTo { height: u32 },
    Smallest,
}

/// Everything the engine needs to run one download.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadJob {
    pub url: String,
    pub kind: DownloadKind,
    pub folder: PathBuf,
}

/// Identifies one download. Serializes as a plain number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobId(pub u64);

/// The step a job is on.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    Probing,
    DownloadingVideo,
    DownloadingAudio,
    Merging,
    Converting,
}

/// What happened in a job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum JobEventKind {
    Phase { phase: Phase },
    Progress { done: u64, total: Option<u64> },
    Done { path: PathBuf },
    Failed { msg: String },
    Cancelled,
}

/// An event from a running job. Every event carries its job's ID so V2 can run several jobs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobEvent {
    pub job_id: JobId,
    pub kind: JobEventKind,
}

impl JobEvent {
    /// True for the last event a job sends: Done, Failed, or Cancelled.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.kind,
            JobEventKind::Done { .. } | JobEventKind::Failed { .. } | JobEventKind::Cancelled
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pins the JSON contract the TypeScript side mirrors.
    #[test]
    fn job_event_json_shape() {
        let e = JobEvent { job_id: JobId(7), kind: JobEventKind::Progress { done: 5, total: Some(10) } };
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({"jobId":7,"kind":{"type":"progress","done":5,"total":10}})
        );
        let p = JobEvent { job_id: JobId(7), kind: JobEventKind::Phase { phase: Phase::DownloadingVideo } };
        assert_eq!(serde_json::to_value(&p).unwrap()["kind"]["phase"], "downloadingVideo");
    }

    #[test]
    fn download_kind_json_shape() {
        assert_eq!(serde_json::to_value(DownloadKind::Mp3).unwrap(), serde_json::json!({"type":"mp3"}));
    }

    #[test]
    fn quality_json_shape() {
        assert_eq!(serde_json::to_value(Quality::UpTo { height: 720 }).unwrap(), serde_json::json!({"type":"upTo","height":720}));
        assert_eq!(serde_json::to_value(Quality::Best).unwrap(), serde_json::json!({"type":"best"}));
    }
}
