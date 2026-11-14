# Manual test checklist (V1)

Run the app with `cd app; npm run tauri dev`. Use a public video you're allowed to download.
Tick each box as it passes; note anything odd underneath it.

## Screens and choices

- [ ] **Empty state:** on launch, only the link box, the Video/MP3 toggle, the folder, and a disabled Download button show.
- [ ] **Paste a valid link:** "Getting video info…" shows, then the thumbnail, title, and duration appear — no Go button needed.
- [ ] **Resolutions:** the list matches what the video offers, highest first and preselected, with `~N MB` where YouTube gives a size. Choose each one; the selection sticks.
- [ ] **Switch to MP3:** the resolution list hides; switching back to Video restores it.
- [ ] **Clear the link:** the ✕ appears inside the link box once it has text. Clicking it (or pressing Esc in the box) empties the box, hides the video and any error, and leaves the cursor in the box. The ✕ is hidden and Esc does nothing during a download.
- [ ] **Installed app:** after running the installer, YouTube Downloader is in the Start menu, opens with no console window, finds yt-dlp/ffmpeg/Deno (no missing-tools panel), and remembers the folder.
- [ ] **Folder remembered:** choose a different folder, close the app, start it again — the new folder is shown.

## Downloads

- [ ] **Video download:** completes, "Saved …" shows, Open folder opens File Explorer at the file, and the file plays in VLC with sound.
- [ ] **MP3 download:** completes and plays.
- [ ] **Same video twice:** the second file is named `… (1).mp4` / `.mkv` / `.mp3`; the first is untouched.

## Cancel (after each: no `.part` file and no partial output file in the folder)

- [ ] Cancel while **Downloading video**
- [ ] Cancel while **Downloading audio**
- [ ] Cancel while **Merging** (use a long video so the phase lasts long enough to click)
- [ ] Cancel while **Converting to MP3**

## Edge cases

- [ ] **Paste during a download:** the link box is locked; nothing changes.
- [ ] **Private or age-restricted link:** a specific plain message, not a raw yt-dlp error.
- [ ] **Garbage link** (e.g. `hello`): a plain error suggesting `yt-dlp -U` / checking the link.
- [ ] **Missing tool:** start the app with `deno` off PATH (e.g. temporarily rename `deno.exe`). The missing-tools panel lists Deno with its install command and Download stays disabled. Restore it afterwards.
- [ ] **Dark mode:** switch Windows to dark mode — the app follows.
- [ ] **Resize:** make the window larger and back to its minimum — the layout holds, nothing overlaps or scrolls sideways.
- [ ] **Close during a download:** start a long video download, then close the window. The app takes a moment to exit, and afterwards no `.part` file or partial output is left in the folder, and no `ffmpeg.exe` is left running in Task Manager.
- [ ] **Double-click Download:** double-click it quickly — exactly one download starts, with its progress bar and Cancel.
