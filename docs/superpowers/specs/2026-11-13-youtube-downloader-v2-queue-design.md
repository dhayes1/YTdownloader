# YouTube Downloader — Design (V2: download queue)

**Date:** 2026-11-13
**Status:** Draft for approval
**Builds on:** `docs/superpowers/specs/2026-11-12-youtube-downloader-design.md` (V1). Everything in V1
still applies unless this document changes it.

Items marked **(Claude's call)** were decided by Claude, not Donny. They're open to change.

---

## 1. Intent and why

Donny has a playlist he wants to download without copying and pasting each video. He wants to
**line up a few downloads and walk away**. V2 turns the one-video-at-a-time app into a queue:
paste playlist or video links, choose quality once, press Start, and come back to finished files.

The project's goals from V1 still hold: learning AI-assisted development end to end, and learning
Rust. That's why the queue's rules live in the tested Rust engine (§6).

## 2. Goals (V2)

- Paste a **playlist link**: every video in it is added to the queue, in playlist order.
- Paste a **single video link**: one video is added. Playlists and single videos can be mixed.
- **One quality choice for the whole batch:** Video or MP3. For Video, also **Best · Up to 1080p ·
  Up to 720p · Smallest** (§5.2).
- **Start** downloads the queue **one video at a time**. Videos added while it runs join the end.
- Each row shows its state: waiting, downloading (with progress), done, or failed with a short reason.
- **A failure doesn't stop the queue.** The failed row shows its reason and the next video starts.
- **Remove** a waiting video, or a whole playlist group, with ✕. ✕ on the running video cancels it.
- **Stop** cancels the running video and leaves the remaining videos waiting.
- A playlist's files go into **their own folder** named after the playlist, **numbered** in
  playlist order (§5.3).
- **Keep the PC awake** while the queue runs, so Windows doesn't sleep partway through (§5.5).

## 3. Non-goals (V2)

- Saving the queue between app runs. Closing the app drops unfinished videos (cleaned up as in V1).
- Running more than one download at a time.
- A different quality for each video in a batch.
- Channel links (`/@name`, `/channel/…`). They show a clear "paste a playlist or video link"
  message, as playlists did in V1. **(Claude's call)**: a channel can hold thousands of videos,
  which doesn't fit "line up a few".
- **YouTube Mixes** (auto-generated "Mix – …" lists, `list=RD…`). Decided by Donny: they're
  personalized and effectively endless, so they're rejected with a clear message ("That's a YouTube
  Mix, which changes every time. Paste a regular playlist or single videos instead."). A
  `watch?v=…&list=RD…` link still adds just that video, like any video-plus-playlist link.
- A size limit or confirmation for big playlists. Decided by Donny: a regular playlist always adds
  every video. The group row shows the count, and one ✕ removes the whole group.
- Retrying failed videos automatically. (Re-adding the link is the manual retry.)
- Everything V1 already excluded (subtitles, installers beyond the current per-user one,
  macOS/Linux, age-restricted/private/members-only videos, installing the tools).

## 4. Anti-goals

V1's anti-goals stand. These are added:

- **A queue that needs babysitting.** If a single bad video can stall or stop the queue, or the PC
  sleeps mid-queue, the feature failed even if each download works.
- **Queue rules tied to the UI.** The rules (what runs next, what a failure does, what Stop does)
  belong in the engine, testable with the fake backend.

## 5. Behavior

### 5.1 Adding to the queue

- The link box keeps V1's behavior: paste adds right away. Enter, or an **Add** button, also adds.
  **(Claude's call)**: paste-to-add matches V1, and the button covers typed links.
- **Playlist link** (`list=` with no video, or a `playlist?list=` URL):
  - The app reads the playlist's title and video list with yt-dlp's flat listing, which takes
    seconds, not minutes.
  - It adds a group row with the playlist title, and one row per video in playlist order.
  - Unavailable entries (deleted or private, as reported by the listing) are still added. They
    fail when their turn comes, with a specific message.
- **A video link that also carries a playlist** (`watch?v=…&list=…`) adds just that video, as in V1.
  **(Claude's call)**: that's how YouTube shares "this video in a playlist", and V1 already treats
  it this way.
- **Single video link:** **(Claude's call)** read first, then added:
  - The link box shows V1's "Getting video info…" while a probe runs (the existing `Engine::probe`,
    outside the queue). On success, one waiting row is added with the video's title.
  - If the probe finds a problem (private, age-restricted, live, bad link…), no row is added and
    the box shows V1's message, as V1 does today. So the queue only ever holds readable videos and
    playlist entries; it needs no "probing" state.
  - The queue can keep running while a link is being read. Start doesn't wait for it.
- **How a link is classified** **(Claude's call)**: one probe command handles every link
  (`yt-dlp -J --no-playlist --flat-playlist`): `--no-playlist` turns a `watch?v=…&list=…` link into
  just that video, and `--flat-playlist` lists a pure playlist in seconds. The JSON `_type` says
  which it was. A playlist result whose page is a channel (`/@name`, `/channel/`, `/c/`, `/user/`)
  is rejected (§3).
- The box clears after a successful add, so the next link can be pasted. **(Claude's call)**
- The ✕ and Esc clear-link controls from the last change stay.
- Adding the same video twice is allowed. File naming already prevents overwrites, and detecting
  duplicates isn't worth the complexity. **(Claude's call)**

### 5.2 Quality choice

- Video or MP3, applied to every video that hasn't started yet. Changing it mid-queue affects only
  videos still waiting, including after Stop. The queue holds one current choice and reads it as
  each video starts; entries don't keep their own copy. **(Claude's call)**
- **Video:** the choice is applied when each video's turn comes, from that video's own resolution
  list (the job already probes each video fresh when it starts):
  - **Best:** the highest entry in the list.
  - **Up to 1080p / Up to 720p:** the best entry at or below that height. If the video has nothing
    that low, it gets its smallest entry.
  - **Smallest:** the lowest entry.
  - **Frame rate:** at the same height, Best and the caps prefer the higher frame rate (1080p60
    over 1080p). Smallest prefers the lower one.
  - "Height" means V1's height label. Portrait videos (Shorts) keep V1's long-side labeling, a
    known minor item.
- **MP3:** always the best audio, converted as in V1. The resolution choice is hidden.
- Default on first run, or when `settings.json` has no saved choice (as V1's doesn't): **Video,
  Best**. The last choice is remembered like the folder, in `settings.json`. **(Claude's call)**
- **V1's per-video resolution list goes away.** Every video, single or playlist, uses the batch
  choice. **(Claude's call)**: this follows from "one choice for the whole batch", and keeps one
  flow instead of two.

### 5.3 Files and folders

- **Single videos:** into the chosen folder with V1 naming (`Title.mkv`, `Title (1).mkv`).
- **Playlist videos:** into `<chosen folder>\<playlist title>\`, named `NN - Title.ext`.
  - `NN` is the video's position in the flat listing (yt-dlp's `playlist_index`, or the entry's
    order when that's absent), zero-padded to the width of the playlist's size (`01`–`99`, or `001`
    for 100 or more). It's fixed when the playlist is added and travels with the entry. Numbers stay
    as the playlist had them even if some videos are removed from the queue. **(Claude's call)**:
    the files then match the playlist.
  - Name clashes add ` (1)` after the whole name: `01 - Title (1).mkv`.
  - The playlist folder name is cleaned with V1's file-name rules.
  - If the folder already exists (the same playlist downloaded again), it's reused. Files are never
    overwritten: V1's ` (1)` naming applies. **(Claude's call)**
- **Containers and quality-first rules** are unchanged from V1: never re-encode, MP4 only for
  H.264 + AAC, MKV otherwise.
- The chosen folder is checked when **each** video starts, as V1 checks it per job. If it has
  disappeared (an unplugged drive), that video fails with V1's "Choose another folder" message.
  **(Claude's call)**: after one folder failure, the queue **pauses** instead of failing every
  remaining video with the same message. Choosing a folder and pressing Start resumes it. If the
  folder is still missing, the next video fails the same way and the queue pauses again.
  - "Folder failure" means V1's missing-folder error only. Other file errors (no permission, disk
    full, can't create the playlist subfolder) fail that one video and the queue moves on, like any
    other failure.

### 5.4 Running, stopping, removing

- **Start** runs waiting videos top to bottom, one at a time. When the last one finishes, the queue
  is idle. Done and failed rows stay visible until cleared.
- **Clear finished** removes done and failed rows. **(Claude's call)**: otherwise a long session's
  list only grows.
- **Stop** cancels the running video (cleaned up as V1's Cancel), marks it waiting again, and
  pauses the queue. Start resumes from the top waiting row. **(Claude's call)**: Stop keeps your
  list; ✕ is how you remove.
- **✕ on a row:**
  - Waiting → removed.
  - Running → cancelled and removed, and the queue continues with the next video.
  - Done or failed → removed from the list. Files on disk are never deleted by ✕.
- **✕ on a playlist group** removes every row in it at once. If one of them is running, it's
  cancelled, and the queue continues with the next row outside the group. **(Claude's call)**
- Order of operations **(Claude's call)**: removal happens immediately in the list. A cancelled
  video's cleanup finishes before the next video starts. Stop after ✕ simply pauses what's left.
- Clicking a failed row shows its full message (V1's plain-language text).
- The Download button from V1 becomes **Start (N waiting)**. Stop shows while running.
- The footer shows totals, for example "1 done · 1 failed · 2 waiting". **"Open folder"** opens the
  folder of the most recently finished file. **(Claude's call)**

### 5.5 Keeping the PC awake

- While a video is downloading or processing, the app asks Windows to keep the system awake
  (Windows' system-required execution state). It clears the request when the queue goes idle,
  is stopped, or the app exits. The display may still turn off.
- This is the same request media players and installers make. It doesn't change power settings.

### 5.6 Closing the app

- V1's close-cleanup extends to the queue: cancel the running video, wait (up to 10 s) for its
  cleanup, drop waiting videos, release the keep-awake request, then exit. As in V1 this is
  best-effort: if the 10 s pass, the app exits anyway.
- **(Claude's call)**: no "are you sure?" prompt when closing with videos waiting. Per Donny,
  the queue isn't saved between runs, and closing is a deliberate act.

### 5.7 Screen (layout A, chosen by Donny)

```
┌────────────────────────────────────────┐
│ [ Paste a video or playlist link ✕ ][Add]│
│ [   Video   |   MP3   ]                │
│ [ Up to 1080p        ▾ ]  [📁]          │
│ C:\Users\Donny\Downloads               │
│ [        Start (4 waiting)        ]    │
│ ┌────────────────────────────────────┐ │
│ │ 📂 Road Trip Mix (4)             ✕ │ │
│ │   01 · Opening track       ✓ Done  │ │
│ │   02 · Mountain drive ▓▓░░  45%  ✕ │ │
│ │   03 · Private video  Failed: priv │ │
│ │   04 · Night highway     Waiting ✕ │ │
│ │ DH Final Project CSE340  Waiting ✕ │ │
│ └────────────────────────────────────┘ │
│ 1 done · 1 failed · 2 waiting  [Stop]  │
│ [Clear finished]       [Open folder]   │
└────────────────────────────────────────┘
```

- **Top half:** V1's controls. **Below:** the queue list, which scrolls on its own when long.
- **(Claude's call)**: the default window grows to about 460×720 (still resizable, minimum
  420×600) so the list has room. V1's single-video panel (thumbnail, title, duration) is replaced
  by the rows.
- The missing-tools panel and light/dark theming are unchanged.
- An empty queue shows a short hint where the list goes: "Paste a playlist or video link to
  start." **(Claude's call)**

## 6. Approaches considered — where the queue lives

| Option | Summary | Verdict |
|---|---|---|
| **1. Engine queue module** | A pure-Rust queue next to the job runner, using the same `Backend` seam. Tested with the fake backend. Tauri adds commands and forwards events | **Chosen by Donny.** The rules that run unattended are in the tested core; fits V1's "engine has no UI dependency" |
| 2. Tauri app layer | The same logic inside the Tauri crate | Harder to test (needs Tauri around it); mixes rules into glue code |
| 3. Frontend (TypeScript) | The reducer decides what runs next and calls `start_download` per video | Easy to test, but walking away depends on the window's JavaScript; keep-awake and close-cleanup split across two places |

Running several downloads at once was dropped early. It loads YouTube harder, and YouTube already
enforces waits like `available_at`. One at a time is the gentler choice, and it matches "walk away".

## 7. Design

### 7.1 Engine additions

- **Probe results:** a probe returns either a single video (V1's `VideoInfo`) or a playlist: its
  title and its entries (each with a link, a title, and its playlist position). Playlists are no
  longer rejected. Channels still are (§3).
- **Quality rule:** a function choosing a video format from a video's list for Best / Up to H /
  Smallest (§5.2), next to V1's selection code, with fixture tests like V1's.
- **Job input:** a job carries the quality choice instead of V1's exact `VideoOption`, plus where
  its file goes (folder, optional playlist subfolder and number prefix). The job runner otherwise
  stays V1's: fresh probe, `available_at` wait, download, merge or convert, cleanup, exactly one
  terminal event.
- **Queue:** owns the ordered list of entries and runs them one at a time through the existing job
  runner. It has operations for add (a video, or a playlist group), remove (an entry or a group),
  start, stop, and clear finished. It reports queue events (an entry added, an entry's state or
  progress changed, the queue went idle, running, or paused) tagged with entry and group IDs. The
  queue's rules are tested with the fake backend: order, failure moves on, folder failure pauses,
  Stop, ✕ while running, adding while running, and clear finished.

### 7.2 App (Tauri) changes

- Commands for the queue operations, and the quality choice saved in `settings.json`.
- Forward queue events to the window.
- Keep-awake: set while the queue is running, cleared on idle, stop, or exit. Windows only, behind
  `cfg(windows)`.
- The single-job guard (`ActiveJob`) is replaced by the queue's own one-at-a-time rule. The
  close-cleanup hook uses the queue (§5.6).

### 7.3 Frontend changes

- The reducer tracks the queue (entries, groups, states, progress) from queue events, plus the
  quality choice and folder. It stays pure and tested.
- The view renders layout A (§5.7).
- V1's single-job states (`starting`, `downloading`, the start-reply buffering in `main.ts`) are
  replaced by queue state. The double-click protection carries over: Start and Add can't fire twice.

## 8. Testing

- **Engine:** fixture tests for the quality rule. Playlist-probe parsing from a saved flat-playlist
  JSON fixture. Queue rules with the fake backend (§7.1). V1's job tests are updated for the new
  job input.
- **Network (ignored by default):** one real playlist listing, and a short real queue (two MP3s).
- **Frontend:** reducer tests for queue events, removal, quality changes, and Start/Add
  double-click protection.
- **Manual checklist:** V1's checklist, updated, plus: a real playlist downloads into its numbered
  folder, a failed row doesn't stop the queue, Stop and Start resume, ✕ while running, closing
  mid-queue cleans up, and the PC doesn't sleep during a long queue (check with `powercfg
  /requests` while it runs).

## 9. Left to the builder

- Exact Rust types and names for probe results, queue entries, and events, within §7.
- How queue events are shaped for the frontend (one event type with kinds, as V1's `JobEvent`).
- Visual polish within layout A.
- Message wording, in V1's plain-language style with a next step.

## 10. Risks

| Risk | Plan |
|---|---|
| YouTube throttles or blocks a long run of downloads | One at a time, with the `available_at` wait per video. If 403s show up mid-playlist, add a short pause between videos (builder may add a small fixed gap). Approach #1 from V1 (yt-dlp downloads) remains the fallback |
| Flat-playlist output format changes in a yt-dlp update | Parse only what's needed (title, entries' id/url/title). Keep a saved fixture and the ignored network test to catch it |
| Keep-awake left on after a crash | Windows clears an execution-state request when the process exits, so a crash can't leave it on |

## 11. Amendment after manual testing (2026-11-13)

Donny's walk-through showed that the rule "a `watch?v=…&list=…` link adds just that video" (§5.1)
doesn't match how he copies links: YouTube gives that form while a playlist is playing. These
changes supersede the matching parts of §5.1, §5.2 and §5.6.

### 11.1 A link to a video inside a playlist asks

Decided by Donny: a `watch?v=…&list=…` link asks whether to add the whole playlist or just the video.

- **Regular playlist:** the link box shows "This video is part of the playlist “<title>” (<N>
  videos)." with **Whole playlist** and **Just this video** buttons. ✕/Esc cancels without adding.
- **YouTube Mix** (`list=RD…`) **(Claude's call)**: shows the Mix message ("That's a YouTube Mix,
  which changes every time. Paste a regular playlist or single videos instead.") with a **Just this
  video** button.
- **Private or missing playlist** **(Claude's call)**: shows "This playlist is private or doesn't
  exist." with a **Just this video** button. yt-dlp signals this by falling back to the single video
  ("Unable to recognize playlist").
- A plain `watch?v=…` link adds the video, and a `playlist?list=…` link adds the playlist, as before.
- A private or missing `playlist?list=…` link shows "This playlist is private or doesn't exist."
  instead of yt-dlp's raw text ("YouTube said: The playlist does not exist.").
- Paste still adds right away (Donny). Enter and Add remain for typed links. When the question
  appears, nothing is added until a button is chosen.

### 11.2 Video quality is remembered

The last video quality sticks when switching MP3 → Video, and across restarts (saved in
`settings.json` alongside `choice`).

### 11.3 Closing asks first

Decided by Donny, replacing §5.6's "no prompt": if any video is downloading or waiting, closing the
window shows a standard Windows dialog, "Downloads are in progress. Close anyway?", with **Close** and
**Keep downloading** **(Claude's call: native dialog)**. Close then runs §5.6's cleanup. With nothing
downloading or waiting, the app closes at once.
