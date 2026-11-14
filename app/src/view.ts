// Draws a State onto the existing elements in index.html. No logic beyond "what to show".

import { formatDuration, formatSize, phaseText } from './format';
import { canDownload, type State } from './state';

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

/** The file name from a Windows or POSIX path. */
function baseName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

export function render(s: State, root: HTMLElement): void {
  const downloading = s.status === 'downloading';
  const info = s.info;

  // URL box: locked during a download, so a paste can't start a second job.
  const url = el<HTMLInputElement>(root, 'url');
  url.disabled = downloading;

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

  el(root, 'probing').hidden = s.status !== 'probing';

  // Video info
  el(root, 'video').hidden = !info;
  const thumb = el<HTMLImageElement>(root, 'thumb');
  if (info?.thumbnailUrl && thumb.getAttribute('src') !== info.thumbnailUrl) {
    thumb.src = info.thumbnailUrl;
  }
  thumb.hidden = !info?.thumbnailUrl;
  el(root, 'title').textContent = info?.title ?? '';
  el(root, 'duration').textContent = formatDuration(info?.durationSecs ?? null);

  // Video | MP3
  const videoBtn = el<HTMLButtonElement>(root, 'kind-video');
  const mp3Btn = el<HTMLButtonElement>(root, 'kind-mp3');
  videoBtn.setAttribute('aria-pressed', String(s.kind === 'video'));
  mp3Btn.setAttribute('aria-pressed', String(s.kind === 'mp3'));
  videoBtn.disabled = downloading || !info || info.videoOptions.length === 0;
  mp3Btn.disabled = downloading || !info || !info.hasAudio;

  // Resolution list (video only)
  const quality = el<HTMLSelectElement>(root, 'quality');
  const options = info?.videoOptions ?? [];
  const wanted = options.map((o, i) => `${i}|${o.label}|${o.estSize}`).join(',');
  if (quality.dataset.options !== wanted) {
    quality.dataset.options = wanted;
    quality.replaceChildren(
      ...options.map((o, i) => {
        const size = formatSize(o.estSize);
        return new Option(size ? `${o.label}   ${size}` : o.label, String(i));
      }),
    );
  }
  quality.value = String(s.optionIndex);
  quality.hidden = s.kind !== 'video';
  quality.disabled = downloading || options.length === 0;

  // Folder
  el<HTMLButtonElement>(root, 'folder-btn').disabled = downloading;
  const folder = el(root, 'folder');
  folder.textContent = s.folder;
  folder.title = s.folder;

  // Download
  const download = el<HTMLButtonElement>(root, 'download');
  download.disabled = !canDownload(s);
  download.hidden = downloading;

  // Progress
  el(root, 'progress').hidden = !downloading;
  const bar = root.querySelector<HTMLElement>('.bar')!;
  const fill = el(root, 'bar-fill');
  const indeterminate = s.percent === null;
  bar.classList.toggle('indeterminate', indeterminate);
  fill.style.width = indeterminate ? '' : `${s.percent}%`;
  if (indeterminate) bar.removeAttribute('aria-valuenow');
  else bar.setAttribute('aria-valuenow', String(s.percent));
  el(root, 'phase').textContent = s.phase ? phaseText(s.phase) : '';
  el(root, 'percent').textContent = indeterminate ? '' : `${s.percent}%`;

  // Done
  el(root, 'done').hidden = s.status !== 'done';
  el(root, 'done-name').textContent = s.donePath ? baseName(s.donePath) : '';

  // Error
  const error = el(root, 'error');
  error.hidden = !s.error;
  error.textContent = s.error ?? '';
}
