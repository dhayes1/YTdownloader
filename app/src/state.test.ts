import { describe, expect, test } from 'vitest';
import { buildJob, canDownload, initialState, reduce, type State } from './state';
import type { JobEvent, JobEventKind, VideoInfo } from './types';

// Matches the engine's output for engine/tests/fixtures/synthetic.json (Task 3).
const SYNTH_INFO: VideoInfo = {
  title: 'Synthetic Video',
  durationSecs: 754,
  thumbnailUrl: 'https://i.ytimg.com/vi/x/maxresdefault.jpg',
  videoOptions: [
    { label: '1080p60', height: 1080, fps: 60, codec: 'vp9', estSize: 79_200_000 },
    { label: '1080p', height: 1080, fps: 30, codec: 'vp9', estSize: 49_200_000 },
    { label: '720p60', height: 720, fps: 60, codec: 'avc1.4d4020', estSize: 34_200_000 },
    { label: '720p', height: 720, fps: 30, codec: 'vp9', estSize: 22_200_000 },
  ],
  hasAudio: true,
};

const AUDIO_ONLY: VideoInfo = { ...SYNTH_INFO, videoOptions: [] };

/** Pasted u1, probe succeeded, folder chosen. */
function ready(info: VideoInfo = SYNTH_INFO): State {
  let s = reduce(initialState, { type: 'folderChanged', folder: 'C:\\dl' });
  s = reduce(s, { type: 'urlPasted', url: 'u1' });
  return reduce(s, { type: 'probeOk', url: 'u1', info });
}

function downloading(jobId = 1): State {
  return reduce(ready(), { type: 'started', jobId });
}

function ev(jobId: number, kind: JobEventKind): JobEvent {
  return { jobId, kind };
}

test('missing tools block downloads', () => {
  let s = reduce(initialState, { type: 'toolsChecked', missing: ['deno'] });
  s = reduce(s, { type: 'folderChanged', folder: 'C:\\dl' });
  s = reduce(s, { type: 'urlPasted', url: 'u1' });
  s = reduce(s, { type: 'probeOk', url: 'u1', info: SYNTH_INFO });
  expect(s.missingTools).toEqual(['deno']);
  expect(canDownload(s)).toBe(false);
});

test('paste probes, result makes ready with highest option', () => {
  let s = reduce(initialState, { type: 'urlPasted', url: 'u1' });
  expect(s.status).toBe('probing');
  s = reduce(s, { type: 'probeOk', url: 'u1', info: SYNTH_INFO });
  expect(s.status).toBe('ready');
  expect(s.optionIndex).toBe(0);
  expect(s.kind).toBe('video');
  expect(s.info).toEqual(SYNTH_INFO);
});

test('stale_probe_result_is_ignored', () => {
  let s = reduce(initialState, { type: 'urlPasted', url: 'u1' });
  s = reduce(s, { type: 'urlPasted', url: 'u2' });
  s = reduce(s, { type: 'probeOk', url: 'u1', info: SYNTH_INFO });
  expect(s.status).toBe('probing');
  expect(s.url).toBe('u2');
  s = reduce(s, { type: 'probeFailed', url: 'u1', message: 'old' });
  expect(s.status).toBe('probing');
});

test('probe failure shows message', () => {
  let s = reduce(initialState, { type: 'urlPasted', url: 'u1' });
  s = reduce(s, { type: 'probeFailed', url: 'u1', message: 'This video is private.' });
  expect(s.status).toBe('error');
  expect(s.error).toBe('This video is private.');
  expect(canDownload(s)).toBe(false);
});

test('blank paste clears to empty', () => {
  const s = reduce(ready(), { type: 'urlPasted', url: '   ' });
  expect(s.status).toBe('empty');
  expect(s.info).toBeUndefined();
});

test('no video options defaults to mp3', () => {
  const s = ready(AUDIO_ONLY);
  expect(s.kind).toBe('mp3');
  expect(canDownload(s)).toBe(true);
});

test('kind cannot switch to an unavailable choice', () => {
  const s = reduce(ready(AUDIO_ONLY), { type: 'kindChanged', kind: 'video' });
  expect(s.kind).toBe('mp3');
});

test('option change stays in range', () => {
  let s = reduce(ready(), { type: 'optionChanged', index: 2 });
  expect(s.optionIndex).toBe(2);
  s = reduce(s, { type: 'optionChanged', index: 9 });
  expect(s.optionIndex).toBe(2);
});

test('paste_during_download_is_ignored', () => {
  const s = reduce(downloading(), { type: 'urlPasted', url: 'u9' });
  expect(s.status).toBe('downloading');
  expect(s.url).toBe('u1');
});

test('events for other jobs are ignored', () => {
  const before = downloading(1);
  const after = reduce(before, { type: 'jobEvent', event: ev(2, { type: 'progress', done: 5, total: 10 }) });
  expect(after).toEqual(before);
});

test('progress sets percent', () => {
  let s = reduce(downloading(), { type: 'jobEvent', event: ev(1, { type: 'progress', done: 40, total: 100 }) });
  expect(s.percent).toBe(40);
  s = reduce(s, { type: 'jobEvent', event: ev(1, { type: 'progress', done: 50, total: null }) });
  expect(s.percent).toBeNull();
});

test('phase resets percent', () => {
  let s = reduce(downloading(), { type: 'jobEvent', event: ev(1, { type: 'progress', done: 40, total: 100 }) });
  s = reduce(s, { type: 'jobEvent', event: ev(1, { type: 'phase', phase: 'downloadingAudio' }) });
  expect(s.phase).toBe('downloadingAudio');
  expect(s.percent).toBe(0);
});

describe('done, cancelled, failed', () => {
  test('done', () => {
    const s = reduce(downloading(), { type: 'jobEvent', event: ev(1, { type: 'done', path: 'C:\\dl\\x.mkv' }) });
    expect(s.status).toBe('done');
    expect(s.donePath).toBe('C:\\dl\\x.mkv');
    expect(canDownload(s)).toBe(true);
  });
  test('cancelled', () => {
    const s = reduce(downloading(), { type: 'jobEvent', event: ev(1, { type: 'cancelled' }) });
    expect(s.status).toBe('ready');
    expect(s.jobId).toBeUndefined();
  });
  test('failed', () => {
    const s = reduce(downloading(), { type: 'jobEvent', event: ev(1, { type: 'failed', msg: 'Check your connection.' }) });
    expect(s.status).toBe('error');
    expect(s.error).toBe('Check your connection.');
    expect(canDownload(s)).toBe(true); // the video info is still there, so the user can retry
  });
});

test('start failure keeps the video and shows the message', () => {
  const s = reduce(ready(), { type: 'startFailed', message: 'A download is already running.' });
  expect(s.status).toBe('error');
  expect(s.error).toBe('A download is already running.');
  expect(s.info).toEqual(SYNTH_INFO);
});

test('buildJob', () => {
  let s = reduce(ready(), { type: 'optionChanged', index: 1 });
  expect(buildJob(s)).toEqual({
    url: 'u1',
    kind: { type: 'video', option: SYNTH_INFO.videoOptions[1] },
    folder: 'C:\\dl',
  });
  s = reduce(s, { type: 'kindChanged', kind: 'mp3' });
  expect(buildJob(s)).toEqual({ url: 'u1', kind: { type: 'mp3' }, folder: 'C:\\dl' });
});

test('cannot download while probing or downloading', () => {
  const probing = reduce(ready(), { type: 'urlPasted', url: 'u2' });
  expect(canDownload(probing)).toBe(false);
  expect(canDownload(downloading())).toBe(false);
});

describe('while a start request is in flight', () => {
  const starting = () => reduce(ready(), { type: 'startRequested' });

  test('download is disabled, so a double-click sends one request', () => {
    expect(starting().status).toBe('starting');
    expect(canDownload(starting())).toBe(false);
  });

  test('paste is ignored', () => {
    const s = reduce(starting(), { type: 'urlPasted', url: 'u9' });
    expect(s.status).toBe('starting');
    expect(s.url).toBe('u1');
    expect(s.info).toEqual(SYNTH_INFO);
  });

  test('kind and option are locked', () => {
    expect(reduce(starting(), { type: 'kindChanged', kind: 'mp3' }).kind).toBe('video');
    expect(reduce(starting(), { type: 'optionChanged', index: 1 }).optionIndex).toBe(0);
  });

  test('started and startFailed resolve it', () => {
    expect(reduce(starting(), { type: 'started', jobId: 1 }).status).toBe('downloading');
    const failed = reduce(starting(), { type: 'startFailed', message: 'nope' });
    expect(failed.status).toBe('error');
    expect(failed.info).toEqual(SYNTH_INFO);
  });

  test('startRequested is ignored when a download is not allowed', () => {
    expect(reduce(downloading(), { type: 'startRequested' }).status).toBe('downloading');
  });
});

test('cannot download without a folder', () => {
  let s = reduce(initialState, { type: 'urlPasted', url: 'u1' });
  s = reduce(s, { type: 'probeOk', url: 'u1', info: SYNTH_INFO });
  expect(canDownload(s)).toBe(false);
});
