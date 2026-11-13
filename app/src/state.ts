// The whole UI's logic as one pure function: (state, action) → new state.
// No DOM and no Tauri calls here, so it can be tested without a window.

import type { DownloadJob, JobEvent, Phase, VideoInfo } from './types';

export type Status = 'empty' | 'probing' | 'ready' | 'downloading' | 'done' | 'error' | 'toolsMissing';

export interface State {
  status: Status;
  url: string;
  info?: VideoInfo;
  kind: 'video' | 'mp3';
  optionIndex: number;
  folder: string;
  jobId?: number;
  phase?: Phase;
  percent: number | null;
  donePath?: string;
  error?: string;
  missingTools: string[];
}

export type Action =
  | { type: 'toolsChecked'; missing: string[] }
  | { type: 'folderChanged'; folder: string }
  | { type: 'urlPasted'; url: string }
  | { type: 'probeOk'; url: string; info: VideoInfo }
  | { type: 'probeFailed'; url: string; message: string }
  | { type: 'kindChanged'; kind: 'video' | 'mp3' }
  | { type: 'optionChanged'; index: number }
  | { type: 'started'; jobId: number }
  | { type: 'startFailed'; message: string }
  | { type: 'jobEvent'; event: JobEvent };

export const initialState: State = {
  status: 'empty',
  url: '',
  kind: 'video',
  optionIndex: 0,
  folder: '',
  percent: null,
  missingTools: [],
};

function kindAvailable(info: VideoInfo, kind: 'video' | 'mp3'): boolean {
  return kind === 'video' ? info.videoOptions.length > 0 : info.hasAudio;
}

/** Fields that describe a finished or running job, cleared when a new one starts. */
const NO_JOB = { jobId: undefined, phase: undefined, percent: null, donePath: undefined, error: undefined };

export function reduce(s: State, a: Action): State {
  switch (a.type) {
    case 'toolsChecked': {
      const missingTools = a.missing;
      if (missingTools.length > 0 && s.status === 'empty') return { ...s, missingTools, status: 'toolsMissing' };
      if (missingTools.length === 0 && s.status === 'toolsMissing') return { ...s, missingTools, status: 'empty' };
      return { ...s, missingTools };
    }

    case 'folderChanged':
      return { ...s, folder: a.folder };

    case 'urlPasted': {
      if (s.status === 'downloading') return s; // one job at a time; finish or cancel first
      const url = a.url.trim();
      if (!url) return { ...s, ...NO_JOB, status: 'empty', url: '', info: undefined };
      return { ...s, ...NO_JOB, status: 'probing', url, info: undefined };
    }

    case 'probeOk': {
      if (s.status !== 'probing' || a.url !== s.url) return s; // a newer paste won
      const kind = a.info.videoOptions.length > 0 ? 'video' : 'mp3';
      return { ...s, status: 'ready', info: a.info, kind, optionIndex: 0 };
    }

    case 'probeFailed':
      if (s.status !== 'probing' || a.url !== s.url) return s;
      return { ...s, status: 'error', error: a.message, info: undefined };

    case 'kindChanged':
      if (s.status === 'downloading' || !s.info || !kindAvailable(s.info, a.kind)) return s;
      return { ...s, kind: a.kind };

    case 'optionChanged': {
      const count = s.info?.videoOptions.length ?? 0;
      if (s.status === 'downloading' || a.index < 0 || a.index >= count) return s;
      return { ...s, optionIndex: a.index };
    }

    case 'started':
      return { ...s, ...NO_JOB, status: 'downloading', jobId: a.jobId, phase: 'probing' };

    case 'startFailed':
      return { ...s, ...NO_JOB, status: 'error', error: a.message };

    case 'jobEvent': {
      const { jobId, kind } = a.event;
      if (s.status !== 'downloading' || jobId !== s.jobId) return s;
      switch (kind.type) {
        case 'phase':
          return { ...s, phase: kind.phase, percent: 0 };
        case 'progress':
          return {
            ...s,
            percent: kind.total ? Math.min(100, Math.round((kind.done / kind.total) * 100)) : null,
          };
        case 'done':
          return { ...s, ...NO_JOB, status: 'done', donePath: kind.path };
        case 'cancelled':
          return { ...s, ...NO_JOB, status: 'ready' };
        case 'failed':
          return { ...s, ...NO_JOB, status: 'error', error: kind.msg };
      }
    }
  }
}

/** True when the Download button should be enabled. */
export function canDownload(s: State): boolean {
  const idle = s.status === 'ready' || s.status === 'done' || s.status === 'error';
  return (
    idle &&
    s.info !== undefined &&
    s.folder !== '' &&
    s.missingTools.length === 0 &&
    kindAvailable(s.info, s.kind)
  );
}

/** The job to send to `start_download`. Call only when `canDownload(s)` is true. */
export function buildJob(s: State): DownloadJob {
  const info = s.info!;
  return {
    url: s.url,
    kind: s.kind === 'video' ? { type: 'video', option: info.videoOptions[s.optionIndex] } : { type: 'mp3' },
    folder: s.folder,
  };
}
