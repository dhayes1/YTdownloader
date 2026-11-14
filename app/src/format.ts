// Small display helpers. Pure functions, tested in format.test.ts.

import type { Phase, Quality } from './types';

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
