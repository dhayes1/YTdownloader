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

const message = (err: unknown) => (typeof err === 'string' ? err : String(err));

export function createLinkSubmitter(deps: LinkDeps) {
  const { getState, dispatch, api } = deps;
  // Bumped by every real submit and every clear, so an older in-flight read can tell it is stale.
  let token = 0;

  function submit(raw: string): void {
    const before = getState();
    if (before.link === 'reading' || before.missingTools.length > 0) return;
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

  function clear(): void {
    token++;
    dispatch({ type: 'linkCleared' });
  }

  return { submit, clear };
}
