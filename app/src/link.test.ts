import { expect, test, vi } from 'vitest';
import { createLinkSubmitter } from './link';
import { initialState, reduce, type Action, type State } from './state';
import type { Probed } from './types';

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
