import { expect, test } from 'vitest';
import { QUALITY_CHOICES, phaseText, qualityLabel } from './format';

test('phase text', () => {
  expect(phaseText('probing')).toBe('Getting video info');
  expect(phaseText('downloadingVideo')).toBe('Downloading video');
  expect(phaseText('downloadingAudio')).toBe('Downloading audio');
  expect(phaseText('merging')).toBe('Merging');
  expect(phaseText('converting')).toBe('Converting to MP3');
});

test('quality labels', () => {
  expect(qualityLabel({ type: 'best' })).toBe('Best');
  expect(qualityLabel({ type: 'upTo', height: 1080 })).toBe('Up to 1080p');
  expect(qualityLabel({ type: 'upTo', height: 720 })).toBe('Up to 720p');
  expect(qualityLabel({ type: 'smallest' })).toBe('Smallest');
});

test('quality choices in order', () => {
  expect(QUALITY_CHOICES).toEqual([
    { type: 'best' },
    { type: 'upTo', height: 1080 },
    { type: 'upTo', height: 720 },
    { type: 'smallest' },
  ]);
});
