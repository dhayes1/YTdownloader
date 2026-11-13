// TypeScript mirror of the engine's JSON (engine/src/types.rs). Field names match serde's
// camelCase output; Rust `Option<T>` arrives as `T | null`. The Rust tests
// `job_event_json_shape` and `download_kind_json_shape` pin this contract.

export interface VideoOption {
  label: string;
  height: number;
  fps: number;
  codec: string;
  estSize: number | null;
}

export interface VideoInfo {
  title: string;
  durationSecs: number | null;
  thumbnailUrl: string | null;
  videoOptions: VideoOption[];
  hasAudio: boolean;
}

export type DownloadKind = { type: 'video'; option: VideoOption } | { type: 'mp3' };

export interface DownloadJob {
  url: string;
  kind: DownloadKind;
  folder: string;
}

export type Phase = 'probing' | 'downloadingVideo' | 'downloadingAudio' | 'merging' | 'converting';

export type JobEventKind =
  | { type: 'phase'; phase: Phase }
  | { type: 'progress'; done: number; total: number | null }
  | { type: 'done'; path: string }
  | { type: 'failed'; msg: string }
  | { type: 'cancelled' };

export interface JobEvent {
  jobId: number;
  kind: JobEventKind;
}
