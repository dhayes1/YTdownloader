// Draws a State onto the elements in index.html and rebuilds the queue list.
// No logic beyond "what to show"; clicks are handled by delegation in main.ts via data-* attributes.

import { QUALITY_CHOICES, phaseText, qualityLabel } from './format';
import { canAdd, canStart, totals, waitingCount, type Row, type State } from './state';

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

function removeButton(attr: 'data-remove-entry' | 'data-remove-group', id: number, label: string): HTMLButtonElement {
  const b = node('button', 'row-x', '✕');
  b.type = 'button';
  b.title = label;
  b.setAttribute('aria-label', label);
  b.setAttribute(attr, String(id));
  return b;
}

function rowItem(r: Row, s: State): HTMLLIElement {
  const li = node('li', `item${r.group !== null ? ' in-group' : ''} ${r.state.type}`);
  const line = node('div', 'item-line');
  line.append(node('span', 'item-title', `${r.prefix ? `${r.prefix} · ` : ''}${r.title}`));

  switch (r.state.type) {
    case 'waiting':
      line.append(node('span', 'item-state', 'Waiting'));
      break;
    case 'running': {
      const bar = node('div', 'bar');
      bar.setAttribute('role', 'progressbar');
      bar.setAttribute('aria-valuemin', '0');
      bar.setAttribute('aria-valuemax', '100');
      const fill = node('div', 'bar-fill');
      if (r.percent === null) bar.classList.add('indeterminate');
      else {
        fill.style.width = `${r.percent}%`;
        bar.setAttribute('aria-valuenow', String(r.percent));
      }
      bar.append(fill);
      line.append(
        node('span', 'item-state', r.phase ? phaseText(r.phase) : ''),
        bar,
        node('span', 'item-percent', r.percent === null ? '' : `${r.percent}%`),
      );
      break;
    }
    case 'done':
      line.append(node('span', 'item-state ok-text', '✓ Done'));
      break;
    case 'failed':
      line.append(node('span', 'item-state error-text reason', `Failed: ${r.state.msg}`));
      li.classList.add('clickable');
      li.setAttribute('data-toggle-failure', String(r.id));
      break;
  }
  line.append(removeButton('data-remove-entry', r.id, 'Remove from queue'));
  li.append(line);

  if (r.state.type === 'failed' && s.openFailure === r.id) {
    li.append(node('div', 'failure-full', r.state.msg));
  }
  return li;
}

function buildQueue(s: State): HTMLLIElement[] {
  const items: HTMLLIElement[] = [];
  const groupIds = new Set(s.groups.map((g) => g.id));
  for (const g of s.groups) {
    const members = s.rows.filter((r) => r.group === g.id);
    const head = node('li', 'item group');
    const line = node('div', 'item-line');
    line.append(node('span', 'item-title', `📂 ${g.title}`), node('span', 'item-state', `(${members.length})`));
    line.append(removeButton('data-remove-group', g.id, 'Remove playlist from queue'));
    head.append(line);
    items.push(head, ...members.map((r) => rowItem(r, s)));
  }
  // Single videos (and anything whose group is unknown) after the groups, in order.
  for (const r of s.rows) {
    if (r.group === null || !groupIds.has(r.group)) items.push(rowItem(r, s));
  }
  return items;
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
  el(root, 'queue').replaceChildren(...buildQueue(s));
  el(root, 'empty').hidden = s.rows.length > 0;

  // Footer
  const t = totals(s);
  el(root, 'totals').textContent = `${t.done} done · ${t.failed} failed · ${t.waiting} waiting`;
  el(root, 'stop').hidden = !running;
  el<HTMLButtonElement>(root, 'clear-finished').disabled = t.done + t.failed === 0;
  el<HTMLButtonElement>(root, 'open-folder').disabled = !s.lastDonePath;
}
