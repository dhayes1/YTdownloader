// Wiring: every user action and Rust event becomes an Action for the reducer, then we redraw.

import * as api from './api';
import { buildJob, canDownload, initialState, reduce, type Action, type State } from './state';
import type { JobEvent } from './types';
import { render } from './view';

const root = document.querySelector<HTMLElement>('main.app')!;
let state: State = initialState;

function dispatch(action: Action): void {
  state = reduce(state, action);
  render(state, root);
}

const message = (err: unknown) => (typeof err === 'string' ? err : String(err));

// Job events can arrive before start_download's reply tells us the job's ID.
// Hold them until then, so a very quick failure isn't dropped.
let starting = false;
let early: JobEvent[] = [];

function submitUrl(raw: string): void {
  // Same link already being shown (paste is followed by a change event on blur): nothing to do.
  // After an error, the same link is allowed again so the user can retry.
  if (raw.trim() === state.url && state.status !== 'empty' && state.status !== 'error') return;
  dispatch({ type: 'urlPasted', url: raw });
  if (state.status !== 'probing') return; // blank, or ignored during a download
  const url = state.url;
  api
    .probe(url)
    .then((info) => dispatch({ type: 'probeOk', url, info }))
    .catch((err) => dispatch({ type: 'probeFailed', url, message: message(err) }));
}

async function startDownload(): Promise<void> {
  if (!canDownload(state)) return;
  starting = true;
  try {
    const jobId = await api.startDownload(buildJob(state));
    dispatch({ type: 'started', jobId });
  } catch (err) {
    dispatch({ type: 'startFailed', message: message(err) });
  } finally {
    starting = false;
    const held = early;
    early = [];
    held.forEach((event) => dispatch({ type: 'jobEvent', event }));
  }
}

function wire(): void {
  const url = root.querySelector<HTMLInputElement>('#url')!;
  // The paste event fires before the text lands in the box, so read it on the next tick.
  url.addEventListener('paste', () => setTimeout(() => submitUrl(url.value)));
  url.addEventListener('change', () => submitUrl(url.value));
  url.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') submitUrl(url.value);
  });

  root.querySelector('#kind-video')!.addEventListener('click', () => dispatch({ type: 'kindChanged', kind: 'video' }));
  root.querySelector('#kind-mp3')!.addEventListener('click', () => dispatch({ type: 'kindChanged', kind: 'mp3' }));

  const quality = root.querySelector<HTMLSelectElement>('#quality')!;
  quality.addEventListener('change', () => dispatch({ type: 'optionChanged', index: Number(quality.value) }));

  root.querySelector('#folder-btn')!.addEventListener('click', async () => {
    const folder = await api.pickFolder();
    if (folder) dispatch({ type: 'folderChanged', folder });
  });

  root.querySelector('#download')!.addEventListener('click', () => void startDownload());

  root.querySelector('#cancel')!.addEventListener('click', () => {
    if (state.jobId !== undefined) void api.cancelDownload(state.jobId);
  });

  root.querySelector('#open-folder')!.addEventListener('click', () => {
    if (state.donePath) api.reveal(state.donePath).catch(() => {});
  });
}

async function init(): Promise<void> {
  wire();
  render(state, root);
  await api.onJobEvent((event) => {
    if (starting) early.push(event);
    else dispatch({ type: 'jobEvent', event });
  });
  const [folder, missing] = await Promise.all([api.getFolder(), api.missingTools()]);
  dispatch({ type: 'folderChanged', folder });
  dispatch({ type: 'toolsChecked', missing });
}

void init();
