import { expect, test } from 'vitest';
import { formatDuration, formatSize, phaseText } from './format';

test('duration as m:ss under an hour', () => {
  expect(formatDuration(754)).toBe('12:34');
  expect(formatDuration(5)).toBe('0:05');
  expect(formatDuration(59.6)).toBe('1:00');
});

test('duration as h:mm:ss from an hour', () => {
  expect(formatDuration(3723)).toBe('1:02:03');
});

test('unknown duration is empty', () => {
  expect(formatDuration(null)).toBe('');
});

test('size in MB, or GB from 1000 MB', () => {
  expect(formatSize(79_200_000)).toBe('~79 MB');
  expect(formatSize(400_000)).toBe('~1 MB');
  expect(formatSize(1_450_000_000)).toBe('~1.5 GB');
  expect(formatSize(null)).toBe('');
});

test('phase text', () => {
  expect(phaseText('probing')).toBe('Getting video info');
  expect(phaseText('downloadingVideo')).toBe('Downloading video');
  expect(phaseText('downloadingAudio')).toBe('Downloading audio');
  expect(phaseText('merging')).toBe('Merging');
  expect(phaseText('converting')).toBe('Converting to MP3');
});
