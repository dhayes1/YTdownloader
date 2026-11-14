// Wiring: every user action and Rust event becomes an Action for the reducer, then we redraw.

import * as api from './api';
import { QUALITY_CHOICES } from './format';
import { createLinkSubmitter } from './link';
import { initialState, reduce, type Action, type State } from './state';
import type { Choice } from './types';
import { render } from './view';

const root = document.querySelector<HTMLElement>('main.app')!;
let state: State = initialState;

function dispatch(action: Action): void {
  state = reduce(state, action);
  render(state, root);
}

const quiet = () => {};

const link = createLinkSubmitter({
  getState: () => state,
  dispatch,
  api,
  onAdded: (url) => {
    // The added link leaves the box; anything else in it (a link pasted meanwhile) stays.
    const box = root.querySelector<HTMLInputElement>('#url')!;
    box.value = box.value.replace(url, '').trim();
    root.querySelector<HTMLElement>('#clear-url')!.hidden = box.value === '';
  },
});
const submitUrl = link.submit;

// Set once the user picks Video/MP3 or a quality, so the startup load doesn't overwrite it.
let choiceTouched = false;

function setChoice(choice: Choice): void {
  choiceTouched = true;
  dispatch({ type: 'choiceChanged', choice });
  api.setChoice(choice).catch(quiet);
}

/** Switch Video | MP3; going back to Video restores the remembered quality. */
function setKind(kind: 'video' | 'mp3'): void {
  choiceTouched = true;
  dispatch({ type: 'kindChanged', kind });
  api.setChoice(state.choice).catch(quiet);
}

function wire(): void {
  const url = root.querySelector<HTMLInputElement>('#url')!;
  // The paste event fires before the text lands in the box, so read it on the next tick.
  url.addEventListener('paste', () => setTimeout(() => submitUrl(url.value)));
  url.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') submitUrl(url.value);
    if (e.key === 'Escape') clearUrl();
  });
  // Show or hide the ✕ as you type, without redrawing the list.
  url.addEventListener('input', () => {
    root.querySelector<HTMLElement>('#clear-url')!.hidden = url.value === '';
  });

  function clearUrl(): void {
    url.value = '';
    link.clear();
    url.focus();
  }
  root.querySelector('#clear-url')!.addEventListener('click', clearUrl);
  root.querySelector('#add')!.addEventListener('click', () => submitUrl(url.value));

  root.querySelector('#kind-video')!.addEventListener('click', () => {
    if (state.choice.type !== 'video') setKind('video');
  });
  root.querySelector('#kind-mp3')!.addEventListener('click', () => {
    if (state.choice.type !== 'mp3') setKind('mp3');
  });

  root.querySelector('#ask-whole')!.addEventListener('click', () => void link.answerWhole());
  root.querySelector('#ask-video')!.addEventListener('click', () => void link.answerJustVideo());
  root.querySelector('#ask-close')!.addEventListener('click', clearUrl);
  // Esc cancels the question wherever the focus is.
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && state.ask && e.target !== url) clearUrl();
  });

  const quality = root.querySelector<HTMLSelectElement>('#quality')!;
  quality.addEventListener('change', () =>
    setChoice({ type: 'video', quality: QUALITY_CHOICES[Number(quality.value)] }),
  );

  root.querySelector('#folder-btn')!.addEventListener('click', async () => {
    const folder = await api.pickFolder();
    if (folder) dispatch({ type: 'folderChanged', folder });
  });

  root.querySelector('#start')!.addEventListener('click', () => void api.startQueue().catch(quiet));
  root.querySelector('#stop')!.addEventListener('click', () => void api.stopQueue().catch(quiet));
  root.querySelector('#clear-finished')!.addEventListener('click', () => void api.clearFinished().catch(quiet));
  root.querySelector('#open-folder')!.addEventListener('click', () => {
    if (state.lastDonePath) api.reveal(state.lastDonePath).catch(quiet);
  });

  // Row elements are kept between renders, but one listener on the list is still simplest.
  root.querySelector('#queue')!.addEventListener('click', (e) => {
    const target = e.target as HTMLElement;
    const entry = target.closest<HTMLElement>('[data-remove-entry]');
    if (entry) {
      void api.removeEntry(Number(entry.dataset.removeEntry)).catch(quiet);
      return;
    }
    const group = target.closest<HTMLElement>('[data-remove-group]');
    if (group) {
      void api.removeGroup(Number(group.dataset.removeGroup)).catch(quiet);
      return;
    }
    const failed = target.closest<HTMLElement>('[data-toggle-failure]');
    if (failed) dispatch({ type: 'failureToggled', id: Number(failed.dataset.toggleFailure) });
  });
}

async function init(): Promise<void> {
  wire();
  render(state, root);
  await api.onQueueEvent((event) => dispatch({ type: 'queueEvent', event }));
  const [folder, missing, choice, quality] = await Promise.all([
    api.getFolder(),
    api.missingTools(),
    api.getChoice(),
    api.getVideoQuality(),
  ]);
  dispatch({ type: 'folderChanged', folder });
  dispatch({ type: 'toolsChecked', missing });
  if (!choiceTouched) {
    dispatch({ type: 'videoQualityLoaded', quality });
    dispatch({ type: 'choiceChanged', choice });
  }
}

void init();
