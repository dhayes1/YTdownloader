// The whole UI's logic as one pure function: (state, action) → new state.
// No DOM and no Tauri calls here, so it can be tested without a window.

import type {
  Choice,
  Phase,
  PlaylistLookup,
  Quality,
  QueueEntry,
  QueueEvent,
  QueueGroup,
  QueueStatus,
  VideoInfo,
} from './types';

/** A queue entry plus what the view needs while it runs. */
export interface Row extends QueueEntry {
  phase?: Phase;
  percent: number | null;
}

export interface State {
  link: 'idle' | 'reading';
  linkUrl: string;
  linkError?: string;
  choice: Choice;
  /** Last video quality picked; MP3 -> Video restores it. */
  videoQuality: Quality;
  /** The "whole playlist or just this video?" question, while one is open. */
  ask?: { url: string; info: VideoInfo; lookup: PlaylistLookup };
  folder: string;
  missingTools: string[];
  rows: Row[];
  groups: QueueGroup[];
  status: QueueStatus;
  lastDonePath?: string;
  /** Id of the failed row whose message is expanded. */
  openFailure?: number;
}

export type Action =
  | { type: 'toolsChecked'; missing: string[] }
  | { type: 'folderChanged'; folder: string }
  | { type: 'choiceChanged'; choice: Choice }
  | { type: 'linkSubmitted'; url: string }
  | { type: 'kindChanged'; kind: 'video' | 'mp3' }
  | { type: 'videoQualityLoaded'; quality: Quality }
  | { type: 'linkAsk'; url: string; info: VideoInfo; lookup: PlaylistLookup }
  | { type: 'linkRead'; url: string }
  | { type: 'linkFailed'; url: string; message: string }
  | { type: 'linkCleared' }
  | { type: 'failureToggled'; id: number }
  | { type: 'queueEvent'; event: QueueEvent };

export const initialState: State = {
  link: 'idle',
  linkUrl: '',
  choice: { type: 'video', quality: { type: 'best' } },
  videoQuality: { type: 'best' },
  folder: '',
  missingTools: [],
  rows: [],
  groups: [],
  status: 'idle',
};

export function reduce(s: State, a: Action): State {
  switch (a.type) {
    case 'toolsChecked':
      return { ...s, missingTools: a.missing };

    case 'folderChanged':
      return { ...s, folder: a.folder };

    case 'choiceChanged':
      return { ...s, choice: a.choice, videoQuality: a.choice.type === 'video' ? a.choice.quality : s.videoQuality };

    case 'kindChanged':
      return { ...s, choice: a.kind === 'video' ? { type: 'video', quality: s.videoQuality } : { type: 'mp3' } };

    case 'videoQualityLoaded':
      return { ...s, videoQuality: a.quality };

    case 'linkSubmitted': {
      if (s.link === 'reading') return s; // Add can't fire twice
      const url = a.url.trim();
      if (!url) return { ...s, link: 'idle', linkUrl: '', linkError: undefined, ask: undefined };
      return { ...s, link: 'reading', linkUrl: url, linkError: undefined, ask: undefined };
    }

    case 'linkAsk':
      if (s.link !== 'reading' || a.url !== s.linkUrl) return s; // stale
      return { ...s, link: 'idle', ask: { url: a.url, info: a.info, lookup: a.lookup } };

    case 'linkRead':
      if (!isCurrent(s, a.url)) return s; // stale
      return { ...s, link: 'idle', linkUrl: '', linkError: undefined, ask: undefined };

    case 'linkFailed':
      if (!isCurrent(s, a.url)) return s; // stale
      return { ...s, link: 'idle', linkError: a.message, ask: undefined };

    case 'linkCleared':
      return { ...s, link: 'idle', linkUrl: '', linkError: undefined, ask: undefined };

    case 'failureToggled':
      return { ...s, openFailure: s.openFailure === a.id ? undefined : a.id };

    case 'queueEvent':
      return reduceQueue(s, a.event);
  }
}

/** True when a link result belongs to what is being read now, or to the open question. */
function isCurrent(s: State, url: string): boolean {
  return (s.link === 'reading' && url === s.linkUrl) || s.ask?.url === url;
}

function mapRow(s: State, id: number, f: (r: Row) => Row): State {
  return { ...s, rows: s.rows.map((r) => (r.id === id ? f(r) : r)) };
}

function reduceQueue(s: State, e: QueueEvent): State {
  switch (e.type) {
    case 'added': {
      const rows: Row[] = e.entries.map((entry) => ({ ...entry, percent: null }));
      const groups = e.group && !s.groups.some((g) => g.id === e.group!.id) ? [...s.groups, e.group] : s.groups;
      return { ...s, rows: [...s.rows, ...rows], groups };
    }

    case 'removed': {
      const gone = new Set(e.ids);
      return {
        ...s,
        rows: s.rows.filter((r) => !gone.has(r.id)),
        groups: e.group === null ? s.groups : s.groups.filter((g) => g.id !== e.group),
        openFailure: s.openFailure !== undefined && gone.has(s.openFailure) ? undefined : s.openFailure,
      };
    }

    case 'state': {
      const next = mapRow(s, e.id, (r) => ({ ...r, state: e.state, phase: undefined, percent: null }));
      return e.state.type === 'done' ? { ...next, lastDonePath: e.state.path } : next;
    }

    case 'phase':
      return mapRow(s, e.id, (r) => ({ ...r, phase: e.phase, percent: 0 }));

    case 'progress':
      return mapRow(s, e.id, (r) => ({
        ...r,
        percent: e.total ? Math.min(100, Math.round((e.done / e.total) * 100)) : null,
      }));

    case 'status':
      return { ...s, status: e.status };
  }
}

/** Entries still waiting to download. */
export function waitingCount(s: State): number {
  return s.rows.filter((r) => r.state.type === 'waiting').length;
}

/** True when Start should be enabled. */
export function canStart(s: State): boolean {
  return s.status !== 'running' && waitingCount(s) > 0 && s.folder !== '' && s.missingTools.length === 0;
}

/** True when a link can be added. */
export function canAdd(s: State): boolean {
  return s.link === 'idle' && s.missingTools.length === 0;
}

export function totals(s: State): { done: number; failed: number; waiting: number } {
  let done = 0;
  let failed = 0;
  for (const r of s.rows) {
    if (r.state.type === 'done') done++;
    else if (r.state.type === 'failed') failed++;
  }
  return { done, failed, waiting: waitingCount(s) };
}

/** One line of the queue list: a playlist header or a video row. */
export type LayoutItem = { kind: 'group'; group: QueueGroup; count: number } | { kind: 'row'; row: Row };

/**
 * The list in the order the engine runs it: rows as added, with each playlist's header just
 * before its first member. Rows whose group is unknown are shown as single videos.
 */
export function queueLayout(s: State): LayoutItem[] {
  const groups = new Map(s.groups.map((g) => [g.id, g]));
  const counts = new Map<number, number>();
  for (const r of s.rows) if (r.group !== null) counts.set(r.group, (counts.get(r.group) ?? 0) + 1);
  const items: LayoutItem[] = [];
  const headed = new Set<number>();
  for (const r of s.rows) {
    const g = r.group === null ? undefined : groups.get(r.group);
    if (g && !headed.has(g.id)) {
      headed.add(g.id);
      items.push({ kind: 'group', group: g, count: counts.get(g.id) ?? 0 });
    }
    items.push({ kind: 'row', row: r });
  }
  // A playlist added with no entries is still listed.
  for (const g of s.groups) if (!headed.has(g.id)) items.push({ kind: 'group', group: g, count: 0 });
  return items;
}
