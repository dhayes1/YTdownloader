# YouTube Downloader

A small Windows desktop app (Tauri 2 + Rust) for personal use. Paste a YouTube video or playlist
link, choose **Video** (Best, up to 1080p, up to 720p, or Smallest) or **MP3**, pick a folder, and
download. Video is never re-encoded: the best video and audio streams are merged with ffmpeg into
MP4 when they fit, MKV otherwise.

## The queue

Every link you add goes into a queue. A single video becomes one row; a playlist link becomes a
folder row with one numbered row per video, saved into `<folder>\<playlist title>\` as
`NN - Title.ext`. **Start** downloads the waiting rows one at a time, top to bottom, with the quality
choice read as each video starts. **Stop** pauses after cancelling the running video, **✕** removes
a row or a whole playlist (cancelling it if it's running), and **Clear finished** tidies up done and
failed rows. A link to a video inside a playlist asks whether to add the whole playlist or just that
video. Pasting a link adds it straight away, and your last video quality is remembered. Closing the
window while downloads are running asks "Downloads are in progress. Close anyway?" (**Close** or
**Keep downloading**). The PC stays awake while the queue runs. The queue isn't saved between runs.

## Prerequisites

Windows 10 or 11, with these installed and on `PATH`:

- **Rust** (stable) — `rustc` and `cargo`
- **Node.js** (LTS) — for the frontend build
- **Visual Studio C++ build tools** — the "Desktop development with C++" workload (Tauri needs it on Windows)
- **yt-dlp** — keep it current with `yt-dlp -U`; YouTube changes often
- **ffmpeg** — merges video and audio, converts to MP3
- **Deno** — yt-dlp needs a JavaScript runtime for full YouTube support
- **Git**

The app never installs, bundles, or updates these tools for you.

## Install and launch

Build the installer (a few minutes; from the `app` folder):

```powershell
cd app
npm run tauri build
```

Then run `target\release\bundle\nsis\YouTube Downloader_<version>_x64-setup.exe` (in the repo root's
`target` folder). It installs for your user only (no admin needed) and adds **YouTube Downloader**
to the Start menu, with an uninstall entry in **Settings → Apps**. After changing the code, build
again and rerun the installer to update.

For development with live reload, use `npm run tauri dev` from the `app` folder instead.

## Personal use only

Downloading from YouTube generally violates its Terms of Service unless the content is yours or
you are licensed to download it. This project is for personal learning and is not distributed.

## Spike (stream-download check)

```powershell
cargo run -p engine --example spike -- "<youtube-url>"
```
