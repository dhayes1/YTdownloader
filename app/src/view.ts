// Draws a State onto the elements in index.html and keeps the queue list in sync.
// No logic beyond "what to show"; clicks are handled by delegation in main.ts via data-* attributes.

import { QUALITY_CHOICES, phaseText, qualityLabel } from './format';
import { canAdd, canStart, queueLayout, totals, waitingCount, type Row, type State } from './state';

const INSTALL_HINTS: Record<string, string> = {
  'yt-dlp': 'winget install yt-dlp.yt-dlp',
  ffmpeg: 'winget install Gyan.FFmpeg',
  deno: 'winget install DenoLand.Deno',
};

function el<T extends HTMLElement = HTMLElement>(root: HTMLElement, id: string): T {
  const found = root.querySelector<T>(`#${id}`);
  if (!found) throw new Error(`missing #${id} in index.html`);
  return found;
}

function node<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className?: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const n = document.createElement(tag);
  if (className) n.className = className;
  if (text !== undefined) n.textContent = text;
  return n;
}

function setText(e: HTMLElement, text: string): void {
  if (e.textContent !== text) e.textContent = text;
}

function removeButton(attr: 'data-remove-entry' | 'data-remove-group', id: number, label: string): HTMLButtonElement {
  const b = node('button', 'row-x', '✕');
  b.type = 'button';
  b.title = label;
  b.setAttribute('aria-label', label);
  b.setAttribute(attr, String(id));
  return b;
}

// Row elements are created once per id and then updated in place, so a click, keyboard focus on a
// ✕ and the indeterminate animation all survive progress ticks.
interface RowEls {
  li: HTMLLIElement;
  title: HTMLElement;
  state: HTMLElement;
  bar: HTMLElement;
  fill: HTMLElement;
  percent: HTMLElement;
  failure?: HTMLElement;
}
const rowEls = new Map<number, RowEls>();
const groupEls = new Map<number, { li: HTMLLIElement; title: HTMLElement; count: HTMLElement }>();

function createRow(r: Row): RowEls {
  const li = node('li', 'item');
  const line = node('div', 'item-line');
  const title = node('span', 'item-title');
  const state = node('span', 'item-state');
  const bar = node('div', 'bar');
  bar.setAttribute('role', 'progressbar');
  bar.setAttribute('aria-valuemin', '0');
  bar.setAttribute('aria-valuemax', '100');
  const fill = node('div', 'bar-fill');
  bar.append(fill);
  const percent = node('span', 'item-percent');
  line.append(title, state, bar, percent, removeButton('data-remove-entry', r.id, 'Remove from queue'));
  li.append(line);
  return { li, title, state, bar, fill, percent };
}

function updateRow(e: RowEls, r: Row, s: State): void {
  const { li } = e;
  const cls = `item${r.group !== null ? ' in-group' : ''} ${r.state.type}${r.state.type === 'failed' ? ' clickable' : ''}`;
  if (li.className !== cls) li.className = cls;
  setText(e.title, `${r.prefix ? `${r.prefix} · ` : ''}${r.title}`);

  const running = r.state.type === 'running';
  e.bar.hidden = !running;
  e.percent.hidden = !running;
  let stateClass = 'item-state';
  switch (r.state.type) {
    case 'waiting':
      setText(e.state, 'Waiting');
      break;
    case 'running':
      setText(e.state, r.phase ? phaseText(r.phase) : '');
      e.bar.classList.toggle('indeterminate', r.percent === null);
      if (r.percent === null) {
        e.fill.style.width = '';
        e.bar.removeAttribute('aria-valuenow');
      } else {
        e.fill.style.width = `${r.percent}%`;
        e.bar.setAttribute('aria-valuenow', String(r.percent));
      }
      setText(e.percent, r.percent === null ? '' : `${r.percent}%`);
      break;
    case 'done':
      setText(e.state, '✓ Done');
      stateClass += ' ok-text';
      break;
    case 'failed':
      setText(e.state, `Failed: ${r.state.msg}`);
      stateClass += ' error-text reason';
      break;
  }
  if (e.state.className !== stateClass) e.state.className = stateClass;

  if (r.state.type === 'failed') li.setAttribute('data-toggle-failure', String(r.id));
  else li.removeAttribute('data-toggle-failure');

  if (r.state.type === 'failed' && s.openFailure === r.id) {
    if (!e.failure) {
      e.failure = node('div', 'failure-full');
      li.append(e.failure);
    }
    setText(e.failure, r.state.msg);
  } else if (e.failure) {
    e.failure.remove();
    e.failure = undefined;
  }
}

function groupItem(g: { id: number; title: string }, count: number): HTMLLIElement {
  let e = groupEls.get(g.id);
  if (!e) {
    const li = node('li', 'item group');
    const line = node('div', 'item-line');
    const title = node('span', 'item-title');
    const countEl = node('span', 'item-state');
    line.append(title, countEl, removeButton('data-remove-group', g.id, 'Remove playlist from queue'));
    li.append(line);
    e = { li, title, count: countEl };
    groupEls.set(g.id, e);
  }
  setText(e.title, `📂 ${g.title}`);
  setText(e.count, `(${count})`);
  return e.li;
}

/** Bring the list's children in line with the state, touching only what changed. */
function syncQueue(list: HTMLElement, s: State): void {
  const wanted: HTMLLIElement[] = [];
  const seen = new Set<number>();
  const addRow = (r: Row) => {
    let e = rowEls.get(r.id);
    if (!e) {
      e = createRow(r);
      rowEls.set(r.id, e);
    }
    updateRow(e, r, s);
    seen.add(r.id);
    wanted.push(e.li);
  };
  const groupIds = new Set(s.groups.map((g) => g.id));
  for (const item of queueLayout(s)) {
    if (item.kind === 'group') wanted.push(groupItem(item.group, item.count));
    else addRow(item.row);
  }

  for (const id of [...rowEls.keys()]) if (!seen.has(id)) rowEls.delete(id);
  for (const id of [...groupEls.keys()]) if (!groupIds.has(id)) groupEls.delete(id);

  wanted.forEach((li, i) => {
    if (list.children[i] !== li) list.insertBefore(li, list.children[i] ?? null);
  });
  while (list.children.length > wanted.length) list.lastElementChild!.remove();
}

/** The question that replaces the "Getting video info…" line for a video inside a playlist. */
function askText(ask: NonNullable<State['ask']>): string {
  switch (ask.lookup.type) {
    case 'found': {
      const n = ask.lookup.playlist.entries.length;
      return `This video is part of the playlist “${ask.lookup.playlist.title}” (${n} videos).`;
    }
    case 'mix':
      return "That's a YouTube Mix, which changes every time. Paste a regular playlist or single videos instead.";
    case 'unavailable':
      return "This playlist is private or doesn't exist.";
  }
}

function renderAsk(s: State, root: HTMLElement): void {
  const panel = el(root, 'ask');
  panel.hidden = !s.ask;
  if (!s.ask) return;
  setText(el(root, 'ask-text'), askText(s.ask));
  el(root, 'ask-whole').hidden = s.ask.lookup.type !== 'found';
}

export function render(s: State, root: HTMLElement): void {
  const running = s.status === 'running';

  const url = el<HTMLInputElement>(root, 'url');
  el(root, 'clear-url').hidden = url.value === '';
  el<HTMLButtonElement>(root, 'add').disabled = !canAdd(s);

  // Missing tools
  const tools = el(root, 'tools');
  tools.hidden = s.missingTools.length === 0;
  el(root, 'tools-list').replaceChildren(
    ...s.missingTools.map((t) => {
      const li = document.createElement('li');
      li.append(`${t} — `);
      const code = document.createElement('code');
      code.textContent = INSTALL_HINTS[t] ?? `install ${t}`;
      li.append(code);
      return li;
    }),
  );

  el(root, 'probing').hidden = s.link !== 'reading';
  renderAsk(s, root);
  const linkError = el(root, 'link-error');
  linkError.hidden = !s.linkError;
  linkError.textContent = s.linkError ?? '';

  // Video | MP3
  const isVideo = s.choice.type === 'video';
  el<HTMLButtonElement>(root, 'kind-video').setAttribute('aria-pressed', String(isVideo));
  el<HTMLButtonElement>(root, 'kind-mp3').setAttribute('aria-pressed', String(!isVideo));

  // Quality (video only)
  const quality = el<HTMLSelectElement>(root, 'quality');
  if (quality.options.length === 0) {
    quality.replaceChildren(...QUALITY_CHOICES.map((q, i) => new Option(qualityLabel(q), String(i))));
  }
  if (s.choice.type === 'video') {
    const q = s.choice.quality;
    const i = QUALITY_CHOICES.findIndex(
      (c) => c.type === q.type && (c.type !== 'upTo' || (q.type === 'upTo' && c.height === q.height)),
    );
    quality.value = String(Math.max(0, i));
  }
  quality.hidden = !isVideo;

  // Folder
  const folder = el(root, 'folder');
  folder.textContent = s.folder;
  folder.title = s.folder;

  // Start
  const start = el<HTMLButtonElement>(root, 'start');
  start.textContent = `Start (${waitingCount(s)} waiting)`;
  start.disabled = !canStart(s);

  // Queue list
  syncQueue(el(root, 'queue'), s);
  el(root, 'empty').hidden = s.rows.length > 0;

  // Footer
  const t = totals(s);
  el(root, 'totals').textContent = `${t.done} done · ${t.failed} failed · ${t.waiting} waiting`;
  el(root, 'stop').hidden = !running;
  el<HTMLButtonElement>(root, 'clear-finished').disabled = t.done + t.failed === 0;
  el<HTMLButtonElement>(root, 'open-folder').disabled = !s.lastDonePath;
}
