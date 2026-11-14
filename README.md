# YouTube Downloader

A small Windows desktop app (Tauri 2 + Rust) for personal use. Paste a YouTube link, choose
**Video** (at a resolution the video actually offers) or **MP3**, pick a folder, and download one
video at a time with progress and a Cancel button. Video is never re-encoded: the best video and
audio streams are merged with ffmpeg into MP4 when they fit, MKV otherwise.

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
