import { expect, test, vi } from 'vitest';
import { createLinkSubmitter } from './link';
import { initialState, reduce, type Action, type State } from './state';
import type { PlaylistInfo, PlaylistLookup, Probed } from './types';

const video = (title: string): Probed => ({
  type: 'video',
  info: { title, durationSecs: null, thumbnailUrl: null, videoOptions: [], hasAudio: true },
});

function setup() {
  let state: State = initialState;
  const pending: Array<{ url: string; resolve: (p: Probed) => void; reject: (e: unknown) => void }> = [];
  const api = {
    probeLink: vi.fn(
      (url: string) => new Promise<Probed>((resolve, reject) => pending.push({ url, resolve, reject })),
    ),
    addVideo: vi.fn(async (_url: string, _title: string) => 1),
    addPlaylist: vi.fn(async () => 1),
  };
  const dispatch = (a: Action) => {
    state = reduce(state, a);
  };
  const link = createLinkSubmitter({ getState: () => state, dispatch, api, onAdded: () => {} });
  return { link, api, pending, get: () => state };
}

const flush = () => new Promise((r) => setTimeout(r, 0));

test('second submit while reading makes no second probe', async () => {
  const t = setup();
  t.link.submit('https://a');
  t.link.submit('https://a');
  t.pending[0].resolve(video('A'));
  await flush();
  expect(t.api.probeLink).toHaveBeenCalledTimes(1);
  expect(t.api.addVideo).toHaveBeenCalledTimes(1);
  expect(t.get().link).toBe('idle');
});

test('a different link pasted while reading does not re-add the first', async () => {
  const t = setup();
  t.link.submit('https://a');
  t.link.submit('https://b');
  expect(t.api.probeLink).toHaveBeenCalledTimes(1);
  t.pending[0].resolve(video('A'));
  await flush();
  expect(t.api.addVideo).toHaveBeenCalledTimes(1);
  expect(t.api.addVideo).toHaveBeenCalledWith('https://a', 'A');
});

test('clearing during a pending probe means nothing is added', async () => {
  const t = setup();
  t.link.submit('https://a');
  t.link.clear();
  t.pending[0].resolve(video('A'));
  await flush();
  expect(t.api.addVideo).not.toHaveBeenCalled();
  expect(t.api.addPlaylist).not.toHaveBeenCalled();
});

test('same link resubmitted after clear is not added twice by the old probe', async () => {
  const t = setup();
  t.link.submit('https://a');
  t.link.clear();
  t.link.submit('https://a');
  t.pending[0].resolve(video('A'));
  await flush();
  expect(t.api.addVideo).not.toHaveBeenCalled();
  t.pending[1].resolve(video('A'));
  await flush();
  expect(t.api.addVideo).toHaveBeenCalledTimes(1);
});

test('probe failure shows the message', async () => {
  const t = setup();
  t.link.submit('https://a');
  t.pending[0].reject('bad link');
  await flush();
  expect(t.get().linkError).toBe('bad link');
  expect(t.get().link).toBe('idle');
});

const pl: PlaylistInfo = { title: 'My “list”', entries: [{ url: 'e1', title: 'E1', index: 1 }] };
const inPlaylist = (lookup: PlaylistLookup): Probed => ({
  type: 'videoInPlaylist',
  info: { title: 'Vid', durationSecs: null, thumbnailUrl: null, videoOptions: [], hasAudio: true },
  lookup,
});
const watch = 'https://www.youtube.com/watch?v=abc123&list=PLx&index=2';

async function asked(lookup: PlaylistLookup) {
  const t = setup();
  t.link.submit(watch);
  t.pending[0].resolve(inPlaylist(lookup));
  await flush();
  return t;
}

test('a video in a playlist adds nothing and asks', async () => {
  const t = await asked({ type: 'found', playlist: pl });
  expect(t.api.addVideo).not.toHaveBeenCalled();
  expect(t.api.addPlaylist).not.toHaveBeenCalled();
  expect(t.get().ask?.lookup).toEqual({ type: 'found', playlist: pl });
  expect(t.get().link).toBe('idle');
});

test('whole playlist adds the playlist', async () => {
  const t = await asked({ type: 'found', playlist: pl });
  await t.link.answerWhole();
  expect(t.api.addPlaylist).toHaveBeenCalledWith(pl);
  expect(t.api.addVideo).not.toHaveBeenCalled();
  expect(t.get().ask).toBeUndefined();
  expect(t.get().linkUrl).toBe('');
});

test('just this video adds the plain watch URL with the video title', async () => {
  const t = await asked({ type: 'found', playlist: pl });
  await t.link.answerJustVideo();
  expect(t.api.addVideo).toHaveBeenCalledWith('https://www.youtube.com/watch?v=abc123', 'Vid');
  expect(t.api.addPlaylist).not.toHaveBeenCalled();
  expect(t.get().ask).toBeUndefined();
});

test('clearing during an ask adds nothing, even if answered late', async () => {
  const t = await asked({ type: 'found', playlist: pl });
  t.link.clear();
  await t.link.answerWhole();
  await t.link.answerJustVideo();
  expect(t.api.addVideo).not.toHaveBeenCalled();
  expect(t.api.addPlaylist).not.toHaveBeenCalled();
  expect(t.get().ask).toBeUndefined();
});

test('Mix and unavailable have no whole-playlist path', async () => {
  for (const lookup of [{ type: 'mix' }, { type: 'unavailable' }] as PlaylistLookup[]) {
    const t = await asked(lookup);
    await t.link.answerWhole();
    expect(t.api.addPlaylist).not.toHaveBeenCalled();
    expect(t.get().ask).toBeDefined();
    await t.link.answerJustVideo();
    expect(t.api.addVideo).toHaveBeenCalledWith('https://www.youtube.com/watch?v=abc123', 'Vid');
  }
});

test('a new link replaces the question and the old answer is stale', async () => {
  const t = await asked({ type: 'found', playlist: pl });
  t.link.submit('https://b');
  expect(t.get().ask).toBeUndefined();
  expect(t.api.probeLink).toHaveBeenCalledTimes(2);
  await t.link.answerJustVideo();
  expect(t.api.addVideo).not.toHaveBeenCalled();
  t.pending[1].resolve(video('B'));
  await flush();
  expect(t.api.addVideo).toHaveBeenCalledWith('https://b', 'B');
});

test('Add works again after the question is answered', async () => {
  const t = await asked({ type: 'mix' });
  await t.link.answerJustVideo();
  t.link.submit('https://b');
  expect(t.api.probeLink).toHaveBeenCalledTimes(2);
});

test('a failed add after an answer shows the error', async () => {
  const t = await asked({ type: 'found', playlist: pl });
  t.api.addPlaylist.mockRejectedValueOnce('nope');
  await t.link.answerWhole();
  expect(t.get().linkError).toBe('nope');
  expect(t.get().ask).toBeUndefined();
});

test('two Whole playlist answers while the first add is pending add once', async () => {
  const t = await asked({ type: 'found', playlist: pl });
  let release: () => void = () => {};
  t.api.addPlaylist.mockImplementationOnce(() => new Promise<number>((r) => (release = () => r(1))));
  const first = t.link.answerWhole();
  const second = t.link.answerWhole();
  release();
  await Promise.all([first, second]);
  expect(t.api.addPlaylist).toHaveBeenCalledTimes(1);
  expect(t.api.addVideo).not.toHaveBeenCalled();
});

test('Whole then Just this video while pending adds only the playlist', async () => {
  const t = await asked({ type: 'found', playlist: pl });
  let release: () => void = () => {};
  t.api.addPlaylist.mockImplementationOnce(() => new Promise<number>((r) => (release = () => r(1))));
  const first = t.link.answerWhole();
  const second = t.link.answerJustVideo();
  release();
  await Promise.all([first, second]);
  expect(t.api.addPlaylist).toHaveBeenCalledTimes(1);
  expect(t.api.addVideo).not.toHaveBeenCalled();
  expect(t.get().ask).toBeUndefined();
});
