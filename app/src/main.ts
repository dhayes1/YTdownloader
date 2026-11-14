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
    // The box clears after a successful add, unless the user already typed something else.
    const box = root.querySelector<HTMLInputElement>('#url')!;
    if (box.value.trim() === url) box.value = '';
  },
});
const submitUrl = link.submit;

function setChoice(choice: Choice): void {
  dispatch({ type: 'choiceChanged', choice });
  api.setChoice(choice).catch(quiet);
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
    if (state.choice.type !== 'video') setChoice({ type: 'video', quality: QUALITY_CHOICES[0] });
  });
  root.querySelector('#kind-mp3')!.addEventListener('click', () => {
    if (state.choice.type !== 'mp3') setChoice({ type: 'mp3' });
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
  const [folder, missing, choice] = await Promise.all([api.getFolder(), api.missingTools(), api.getChoice()]);
  dispatch({ type: 'folderChanged', folder });
  dispatch({ type: 'toolsChecked', missing });
  dispatch({ type: 'choiceChanged', choice });
}

void init();
