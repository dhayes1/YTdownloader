# Manual test checklist (V2, download queue)

Run the app with `cd app; npm run tauri dev`. Use public videos and playlists you're allowed to download.
Tick each box as it passes; note anything odd underneath it.

Tip: to queue a whole playlist, paste the playlist's own link (`youtube.com/playlist?list=…`). A link to a video inside a playlist (`watch?v=…&list=…`) asks whether you want the whole playlist or just that video (see Playlists).

## Screens and choices

- [ ] **Empty state:** on launch, the link box with Add, the Video/MP3 toggle, the quality list, the folder, a disabled "Start (0 waiting)" button, and the hint "Paste a playlist or video link to start." show.
- [ ] **Paste a single video link:** "Getting video info…" shows, then one waiting row with the video's title is added and the link box clears.
- [ ] **Quality list:** Best, Up to 1080p, Up to 720p, Smallest; the choice sticks, including after closing and reopening the app.
- [ ] **Switch to MP3:** the quality list hides; switching back to Video restores it, with the same quality you had chosen.
- [ ] **Remembered video quality:** choose Up to 720p, switch to MP3 and back to Video (still Up to 720p); close and reopen the app (still Up to 720p).
- [ ] **Paste adds right away:** pasting a video or playlist link adds it without pressing Enter or Add, and the box clears. Pasting a second link while the first is still being read adds it after the first.
- [ ] **Clear the link:** the ✕ appears inside the link box once it has text. Clicking it (or pressing Esc in the box) empties the box, hides any error, and leaves the cursor in the box.
- [ ] **Enter and Add:** typing or pasting a link then pressing Enter, or clicking Add, adds it. Double-clicking Add adds it only once.
- [ ] **Installed app:** after running the installer, YouTube Downloader is in the Start menu, opens with no console window, finds yt-dlp/ffmpeg/Deno (no missing-tools panel), and remembers the folder.
- [ ] **Folder remembered:** choose a different folder, close the app, start it again — the new folder is shown.

## Playlists

- [ ] **Playlist download:** paste a real playlist link. A 📂 group row appears with its count and one numbered row (`01 · …`) per video. Start downloads them into `<folder>\<playlist title>\` as `NN - Title.ext`, in order.
- [ ] **Video inside a playlist:** paste a `watch?v=…&list=…` link for a regular playlist (or a `youtu.be/…?list=…` link). The link box shows "This video is part of the playlist “<title>” (<N> videos)." with **Whole playlist** and **Just this video**. Nothing is added until you choose. **Whole playlist** adds the 📂 group; **Just this video** adds one row.
- [ ] **Cancel the question:** with the question showing, ✕ (or Esc) closes it and adds nothing.
- [ ] **Mix inside a link:** a `watch?v=…&list=RD…` link shows "That's a YouTube Mix, which changes every time. Paste a regular playlist or single videos instead." with only **Just this video**, which adds the one video.
- [ ] **Private playlist inside a link:** a `watch?v=…&list=…` link to a private or missing playlist shows "This playlist is private or doesn't exist." with only **Just this video**, which adds the one video.
- [ ] **Mix link:** a YouTube Mix link shows the Mix message (no rows added).
- [ ] **Channel link:** a channel link shows the channel message (no rows added).
- [ ] **Private entry:** a private or deleted playlist entry fails with a plain message and the queue continues with the next video.
- [ ] **Private playlist:** a private playlist shows a plain message (not a raw yt-dlp error).
- [ ] **✕ on a group:** removes every row in it; if one is running, it is cancelled and the queue continues with the next row outside the group.

## Queue

- [ ] **Start:** runs waiting rows top to bottom, one at a time; running rows show the phase, a progress bar, and a percent; done rows show "✓ Done".
- [ ] **Totals:** the footer shows `N done · N failed · N waiting` and matches the rows.
- [ ] **Stop then Start:** Stop cancels the running video (it goes back to Waiting) and pauses the queue; Start resumes from the top waiting row.
- [ ] **✕ on the running row:** it is cancelled and removed, and the queue continues with the next video.
- [ ] **✕ on waiting, done, and failed rows:** each is removed; files already on disk are untouched.
- [ ] **Failed row:** shows "Failed" with a short reason; clicking it shows the full message, and clicking again hides it.
- [ ] **Clear finished:** removes done and failed rows and leaves the rest.
- [ ] **Open folder:** enabled after a video finishes; opens File Explorer at that file. The file plays in VLC with sound.
- [ ] **Quality change mid-queue:** change the quality while rows are running; only rows that haven't started use the new choice.
- [ ] **Same video twice:** the second file is named `… (1).mp4` / `.mkv` / `.mp3`; the first is untouched.
- [ ] **MP3:** an MP3 queue completes and the files play.
- [ ] **Long list:** with many rows, the list scrolls on its own and the rest of the window stays put.

## Cancel (after each: no `.part` file and no partial output file in the folder)

- [ ] Stop or ✕ while **Downloading video**
- [ ] Stop or ✕ while **Downloading audio**
- [ ] Stop or ✕ while **Merging** (use a long video so the phase lasts long enough to click)
- [ ] Stop or ✕ while **Converting to MP3**

## Edge cases

- [ ] **Private or age-restricted video link:** a specific plain message under the link box, not a raw yt-dlp error; no row is added.
- [ ] **Garbage link** (e.g. `hello`): a plain error suggesting `yt-dlp -U` / checking the link.
- [ ] **Missing tool:** start the app with `deno` off PATH (e.g. temporarily rename `deno.exe`). The missing-tools panel lists Deno with its install command, and Add and Start stay disabled. Restore it afterwards.
- [ ] **Dark mode:** switch Windows to dark mode — the app follows.
- [ ] **Resize:** make the window larger and back to its minimum — the layout holds, nothing overlaps or scrolls sideways.
- [ ] **Close with nothing to do:** with an empty queue, or only done and failed rows, closing the window exits at once with no prompt.
- [ ] **Close asks first:** with a video downloading or waiting, closing the window shows "Downloads are in progress. Close anyway?" with **Close** and **Keep downloading**. **Keep downloading** leaves the window and the queue running; clicking X again asks again.
- [ ] **Close mid-queue:** start a long queue, then close the window and choose **Close**. The window disappears, the app takes a moment to exit, and afterwards no `.part` file or partial output is left in the folder, and no `ffmpeg.exe` is left running in Task Manager.
- [ ] **Keep awake:** while the queue runs, `powercfg /requests` (in an admin PowerShell) lists the app under SYSTEM; after the queue finishes or is stopped, it no longer does.
