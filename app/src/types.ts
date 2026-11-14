// TypeScript mirror of the engine's JSON (engine/src/types.rs and engine/src/queue.rs). Field
// names match serde's camelCase output; Rust `Option<T>` arrives as `T | null`; ids are plain
// numbers. The Rust tests `quality_json_shape`, `probed_json_shape` and `queue_event_json_shape`
// pin this contract.

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

export type Quality = { type: 'best' } | { type: 'upTo'; height: number } | { type: 'smallest' };

/** What to download (Rust `DownloadKind`): video at a quality, or MP3 audio. */
export type Choice = { type: 'video'; quality: Quality } | { type: 'mp3' };

export interface PlaylistEntry {
  url: string;
  title: string;
  index: number;
}

export interface PlaylistInfo {
  title: string;
  entries: PlaylistEntry[];
}

/** What looking up the playlist behind a watch?v=...&list=... link found. */
export type PlaylistLookup = { type: 'found'; playlist: PlaylistInfo } | { type: 'mix' } | { type: 'unavailable' };

export type Probed =
  | { type: 'video'; info: VideoInfo }
  | { type: 'playlist'; playlist: PlaylistInfo }
  | { type: 'videoInPlaylist'; info: VideoInfo; lookup: PlaylistLookup };

export type Phase = 'probing' | 'downloadingVideo' | 'downloadingAudio' | 'merging' | 'converting';

export type EntryState =
  | { type: 'waiting' }
  | { type: 'running' }
  | { type: 'done'; path: string }
  | { type: 'failed'; msg: string };

export interface QueueEntry {
  id: number;
  group: number | null;
  url: string;
  title: string;
  prefix: string | null;
  state: EntryState;
}

export interface QueueGroup {
  id: number;
  title: string;
}

export type QueueStatus = 'idle' | 'running' | 'paused';

export type QueueEvent =
  | { type: 'added'; group: QueueGroup | null; entries: QueueEntry[] }
  | { type: 'removed'; ids: number[]; group: number | null }
  | { type: 'state'; id: number; state: EntryState }
  | { type: 'phase'; id: number; phase: Phase }
  | { type: 'progress'; id: number; done: number; total: number | null }
  | { type: 'status'; status: QueueStatus };
