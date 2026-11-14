// The link-submit flow (read the link, add it to the queue) as a unit with its collaborators
// passed in, so it can be tested without a window or Tauri.

import type { Action, State } from './state';
import type { PlaylistInfo, Probed } from './types';

export interface LinkApi {
  probeLink: (url: string) => Promise<Probed>;
  addVideo: (url: string, title: string) => Promise<unknown>;
  addPlaylist: (playlist: PlaylistInfo) => Promise<unknown>;
}

export interface LinkDeps {
  getState: () => State;
  dispatch: (a: Action) => void;
  api: LinkApi;
  /** Called after a successful add (main.ts empties the box). */
  onAdded: (url: string) => void;
}

/** The video's own link (no playlist): watch?v=<id> from the submitted URL; unchanged if it has no v. */
export function videoOnlyUrl(raw: string): string {
  try {
    const id = new URL(raw).searchParams.get('v');
    if (id) return `https://www.youtube.com/watch?v=${encodeURIComponent(id)}`;
  } catch {
    // not a URL; fall through
  }
  return raw;
}

const message = (err: unknown) => (typeof err === 'string' ? err : String(err));

export function createLinkSubmitter(deps: LinkDeps) {
  const { getState, dispatch, api } = deps;
  // Bumped by every real submit and every clear, so an older in-flight read can tell it is stale.
  let token = 0;

  // The submit token the open question belongs to; answers made after a clear or a new link are stale.
  let askToken = 0;
  // True from the moment an answer is claimed until its add finishes, so a second click adds nothing.
  let answering = false;

  function submit(raw: string): void {
    const before = getState();
    if (before.link === 'reading' || before.missingTools.length > 0) return;
    if (before.ask) token++; // a new link replaces the question; its answers go stale
    dispatch({ type: 'linkSubmitted', url: raw });
    const after = getState();
    if (after.link !== 'reading') return; // blank link
    const url = after.linkUrl;
    const mine = ++token;
    const current = () => token === mine && getState().link === 'reading';

    void (async () => {
      try {
        const probed = await api.probeLink(url);
        if (!current()) return; // cleared or superseded while reading
        if (probed.type === 'videoInPlaylist') {
          askToken = mine;
          answering = false;
          dispatch({ type: 'linkAsk', url, info: probed.info, lookup: probed.lookup });
          return; // nothing is added until the user answers
        }
        if (probed.type === 'video') await api.addVideo(url, probed.info.title);
        else await api.addPlaylist(probed.playlist);
        if (token !== mine) return;
        deps.onAdded(url);
        dispatch({ type: 'linkRead', url });
      } catch (err) {
        if (token === mine) dispatch({ type: 'linkFailed', url, message: message(err) });
      }
    })();
  }

  /** Run one answer to the open question, if it is still the current one. */
  async function answer(add: (ask: NonNullable<State['ask']>) => Promise<unknown>, only?: (ask: NonNullable<State['ask']>) => boolean): Promise<void> {
    const ask = getState().ask;
    if (!ask || answering || token !== askToken || (only && !only(ask))) return;
    answering = true; // claimed before the first await
    const mine = token;
    try {
      await add(ask);
      if (token !== mine) return;
      deps.onAdded(ask.url);
      dispatch({ type: 'linkRead', url: ask.url });
    } catch (err) {
      if (token === mine) dispatch({ type: 'linkFailed', url: ask.url, message: message(err) });
    } finally {
      if (token === mine) answering = false;
    }
  }

  const answerWhole = () =>
    answer(
      (ask) => (ask.lookup.type === 'found' ? api.addPlaylist(ask.lookup.playlist) : Promise.resolve()),
      (ask) => ask.lookup.type === 'found',
    );

  const answerJustVideo = () => answer((ask) => api.addVideo(videoOnlyUrl(ask.url), ask.info.title));

  function clear(): void {
    token++;
    dispatch({ type: 'linkCleared' });
  }

  return { submit, clear, answerWhole, answerJustVideo };
}
