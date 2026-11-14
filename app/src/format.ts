// Small display helpers. Pure functions, tested in format.test.ts.

import type { Phase, Quality } from './types';

/** 754 → "12:34", 3723 → "1:02:03", null → "". */
export function formatDuration(secs: number | null): string {
  if (secs === null) return '';
  const total = Math.round(secs);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = String(total % 60).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${s}` : `${m}:${s}`;
}

/** Estimated size: "~79 MB", or "~1.5 GB" from 1000 MB; "" when unknown. */
export function formatSize(bytes: number | null): string {
  if (bytes === null) return '';
  const mb = bytes / 1_000_000;
  // Round in tenths of a GB with Math.round (toFixed rounds 1.45 down: binary floats).
  if (mb >= 1000) return `~${(Math.round(mb / 100) / 10).toFixed(1)} GB`;
  return `~${Math.max(1, Math.round(mb))} MB`;
}

const PHASE_TEXT: Record<Phase, string> = {
  probing: 'Getting video info',
  downloadingVideo: 'Downloading video',
  downloadingAudio: 'Downloading audio',
  merging: 'Merging',
  converting: 'Converting to MP3',
};

export function phaseText(phase: Phase): string {
  return PHASE_TEXT[phase];
}

/** The quality choices offered in the dropdown, best first. */
export const QUALITY_CHOICES: Quality[] = [
  { type: 'best' },
  { type: 'upTo', height: 1080 },
  { type: 'upTo', height: 720 },
  { type: 'smallest' },
];

export function qualityLabel(q: Quality): string {
  switch (q.type) {
    case 'best':
      return 'Best';
    case 'upTo':
      return `Up to ${q.height}p`;
    case 'smallest':
      return 'Smallest';
  }
}
