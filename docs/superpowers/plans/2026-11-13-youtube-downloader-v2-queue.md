# YouTube Downloader V2 (Download Queue) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the one-video app into a queue: paste playlist or video links, choose quality once (Best / Up to 1080p / Up to 720p / Smallest, or MP3), press Start, and walk away while it downloads one video at a time.

**Architecture:** A new pure-Rust `Queue` in the `engine` crate runs entries one at a time through V1's existing `run_job`, using the same `Backend` seam (so queue rules are tested with the fake backend). The Tauri crate exposes queue commands, forwards `QueueEvent`s, keeps the PC awake while running, and cleans up on close. The TypeScript reducer mirrors queue state from events; the view becomes layout A.

**Tech Stack:** As V1 (Rust stable edition 2021, tokio, reqwest, serde, thiserror, async-trait, tokio-util; Tauri 2 + store/dialog/opener; TypeScript + Vite + vitest). New: `windows-sys` (feature `Win32_System_Power`) in the app crate for keep-awake.

**Spec:** `docs/superpowers/specs/2026-11-13-youtube-downloader-v2-queue-design.md` (builds on V1's spec `docs/superpowers/specs/2026-11-12-youtube-downloader-design.md`)

## Global Constraints

- Everything in V1's plan Global Constraints still applies (Windows only, tools on PATH and never bundled, `CREATE_NO_WINDOW` + `kill_on_drop`, never re-encode, MP4 only for H.264+AAC, never overwrite, `.part` cleanup, plain-language errors, engine has no Tauri dependency).
- One download at a time. No parallel downloads.
- Probe command for every link: `yt-dlp -J --no-playlist --flat-playlist <url>` (V1's `probe_args`, unchanged).
- Quality choices, exactly: **Best · Up to 1080p · Up to 720p · Smallest**, plus **MP3**. Default when nothing is saved: Video, Best. Saved in `settings.json` under key `choice`.
- Playlist files: `<chosen folder>\<sanitized playlist title>\NN - <sanitized title>.<ext>`; `NN` zero-padded to the digit count of the playlist size, minimum 2.
- Mix message, verbatim: "That's a YouTube Mix, which changes every time. Paste a regular playlist or single videos instead."
- Channel message: "That's a channel link. Paste a playlist or video link instead."
- Window: default about 460×720, minimum 420×600, resizable.
- Queue state lives in memory only; nothing about the queue is saved between runs.
- **Learning rule (as V1):** after each task's commit, explain to Donny in plain language what changed, which files, and any Rust concept that appeared for the first time. Wait for his go-ahead before the next task.

## Review Focus

1. **A playlist with unavailable entries** ("[Private video]", deleted): those rows fail with a specific message and the queue moves on to the next row. Test: Task 4 `failed_entry_does_not_stop_queue`.
2. **✕ on the running video while it's merging**: the job is cancelled, its files are cleaned up *before* the next video starts, and the next video starts. Test: Task 4 `removing_running_entry_cancels_cleans_up_and_continues`.
3. **The same playlist downloaded twice**: the folder is reused, and new files get ` (1)` instead of overwriting. Test: Task 3 `playlist_job_reuses_folder_without_overwriting`.
4. **A playlist title with illegal or no usable characters** (`a/b:c`, `???`): the folder name is sanitized with V1's rules (`abc`, `download`). Test: Task 3 `playlist_folder_name_is_sanitized`.
5. **Start with nothing waiting, Start pressed twice, or Stop while idle**: no-op, no duplicate runner, no extra status events. Test: Task 4 `start_and_stop_are_idempotent`.

---

## File Structure

```
engine/src/
  types.rs        + Quality, Choice; DownloadKind/DownloadJob changed; Probed, PlaylistInfo, PlaylistEntry
  error.rs        + UnsupportedReason::{Channel, Mix}; Playlist variant removed
  ytdlp.rs        + RawInfo playlist fields, RawEntry; parse_info accepts playlists, rejects channel/mix
  select.rs       + select_video_for(info, Quality); to_probed(info)
  job.rs          job input changes (quality, subfolder, prefix); run_job returns JobOutcome
  testing.rs      NEW (#[cfg(test)]): FakeBackend shared by job and queue tests
  queue.rs        NEW: Queue, entries, groups, QueueEvent, runner
  lib.rs          Engine::probe -> Probed; exports
engine/tests/fixtures/
  flat_playlist.json, channel.json, mix.json   NEW
engine/tests/network.rs   + playlist listing and two-MP3 queue tests (ignored)
app/src-tauri/src/
  commands.rs     queue commands replace start/cancel
  settings.rs     + resolve_choice
  keep_awake.rs   NEW: dedicated thread calling SetThreadExecutionState
  state.rs        DELETED (ActiveJob replaced by the queue)
  lib.rs          setup(): build Queue + event forwarder; exit hook uses the queue
app/src/
  types.ts, state.ts, state.test.ts, api.ts, view.ts, main.ts, styles.css, index.html (layout A)
docs/manual-test-checklist.md, README.md
```

---

### Task 1: Quality rule

**Files:**
- Modify: `engine/src/types.rs`, `engine/src/select.rs`

**Interfaces:**
- Consumes (V1): `video_options(&RawInfo) -> Vec<VideoOption>` (height desc, then fps desc), `select_video(&RawInfo, height: u32, fps: u32) -> Option<&RawFormat>`, `fps_class(Option<f64>) -> u32`.
- Produces (`types.rs`):
  - `Quality` — `#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)] #[serde(tag = "type", rename_all = "camelCase")] enum Quality { Best, UpTo { height: u32 }, Smallest }`
- Produces (`select.rs`): `pub fn select_video_for(info: &RawInfo, quality: Quality) -> Option<&RawFormat>`

Rule: take `video_options(info)`. Best → first option. `UpTo { height }` → first option with `option.height <= height`, else the last option. Smallest → last option. Then return `select_video(info, option.height, fps_class(Some(option.fps as f64)))`. (The options' ordering already makes Best and caps prefer higher fps and Smallest prefer lower fps.)

- [ ] **Step 1: Write failing tests** (in `select.rs` tests, using the V1 `SYNTH` fixture)

```rust
#[test] fn quality_best_takes_top_option() { assert_eq!(select_video_for(&info(SYNTH), Quality::Best).unwrap().format_id, "303"); }
#[test] fn quality_cap_takes_best_at_or_below() {
    assert_eq!(select_video_for(&info(SYNTH), Quality::UpTo { height: 1080 }).unwrap().format_id, "303");
    assert_eq!(select_video_for(&info(SYNTH), Quality::UpTo { height: 720 }).unwrap().format_id, "298"); // 720p60
}
#[test] fn quality_cap_below_everything_falls_back_to_smallest() { assert_eq!(select_video_for(&info(SYNTH), Quality::UpTo { height: 480 }).unwrap().format_id, "247"); }
#[test] fn quality_smallest_prefers_lower_fps() { assert_eq!(select_video_for(&info(SYNTH), Quality::Smallest).unwrap().format_id, "247"); }
#[test] fn quality_none_when_no_video() { assert!(select_video_for(&info(COMBINED), Quality::Best).is_none()); }

// types.rs
#[test] fn quality_json_shape() {
    assert_eq!(serde_json::to_value(Quality::UpTo { height: 720 }).unwrap(), serde_json::json!({"type":"upTo","height":720}));
    assert_eq!(serde_json::to_value(Quality::Best).unwrap(), serde_json::json!({"type":"best"}));
}
```

- [ ] **Step 2: Run to verify FAIL** — `cargo test -p engine --lib quality` — Expected: compile errors (`Quality`, `select_video_for` not defined)
- [ ] **Step 3: Implement `Quality` and `select_video_for`** to the interface and rule above.
- [ ] **Step 4: Run to verify PASS** — `cargo test -p engine --lib` — Expected: all pass
- [ ] **Step 5: Commit** — `git commit -am "feat(engine): quality rule (best, cap, smallest)"`

---

### Task 2: Playlist probing

**Files:**
- Create: `engine/tests/fixtures/flat_playlist.json`, `channel.json`, `mix.json`
- Modify: `engine/src/types.rs`, `engine/src/error.rs`, `engine/src/ytdlp.rs`, `engine/src/select.rs`, `engine/src/lib.rs`

**Interfaces:**
- Produces (`types.rs`, camelCase serde like V1):
  - `PlaylistEntry { url: String, title: String, index: u32 }`
  - `PlaylistInfo { title: String, entries: Vec<PlaylistEntry> }`
  - `Probed` — `#[serde(tag = "type", rename_all = "camelCase")] enum Probed { Video { info: VideoInfo }, Playlist { playlist: PlaylistInfo } }`
- Produces (`error.rs`): `UnsupportedReason` gains `Channel` and `Mix`; **`Playlist` is removed** (playlists are now supported). `user_message()` for `Channel` and `Mix` uses the Global Constraints wording verbatim.
- Produces (`ytdlp.rs`):
  - `RawInfo` gains `id: Option<String>`, `webpage_url: Option<String>`, `#[serde(default)] entries: Vec<RawEntry>`
  - `RawEntry { id: Option<String>, url: Option<String>, title: Option<String>, playlist_index: Option<u32> }` (Deserialize, Clone, Debug)
  - `parse_info(json) -> Result<RawInfo, EngineError>` now **accepts** `_type == "playlist"`, still rejects live, and rejects as `Unsupported(Channel)` when `webpage_url` contains `/@`, `/channel/`, `/c/`, or `/user/`, and as `Unsupported(Mix)` when the playlist `id` starts with `RD`.
- Produces (`select.rs`): `pub fn to_probed(info: &RawInfo) -> Result<Probed, EngineError>` — a playlist becomes `Probed::Playlist` (entry `url` = `url`, or `https://www.youtube.com/watch?v={id}` when absent; entries with neither are skipped; `title` = entry title or `"Untitled"`; `index` = `playlist_index`, or the 1-based position when absent); a video becomes `Probed::Video { info: to_video_info(info)? }`.
- Produces (`lib.rs`): `Engine::probe(&self, url) -> Result<Probed, EngineError>` (was `VideoInfo`).

- [ ] **Step 1: Create fixtures**

`flat_playlist.json`:
```json
{"_type":"playlist","id":"PLtest","title":"Road Trip: Mix/Vol 1","webpage_url":"https://www.youtube.com/playlist?list=PLtest","entries":[
{"_type":"url","id":"aaa","url":"https://www.youtube.com/watch?v=aaa","title":"Opening track","playlist_index":1},
{"_type":"url","id":"bbb","title":"Mountain drive","playlist_index":2},
{"_type":"url","id":"ccc","url":"https://www.youtube.com/watch?v=ccc","title":"[Private video]"}]}
```
`channel.json`: `{"_type":"playlist","id":"UCx","title":"Some Channel","webpage_url":"https://www.youtube.com/@SomeChannel/videos","entries":[]}`
`mix.json`: `{"_type":"playlist","id":"RDabc","title":"Mix - Something","webpage_url":"https://www.youtube.com/watch?v=abc&list=RDabc","entries":[]}`

- [ ] **Step 2: Write failing tests**

```rust
// ytdlp.rs — replace V1's rejects_playlist
#[test] fn accepts_playlist() { let i = parse_info(FLAT).unwrap(); assert_eq!(i.kind.as_deref(), Some("playlist")); assert_eq!(i.entries.len(), 3); }
#[test] fn rejects_channel() { assert!(matches!(parse_info(CHANNEL), Err(EngineError::Unsupported(UnsupportedReason::Channel)))); }
#[test] fn rejects_mix() { assert!(matches!(parse_info(MIX), Err(EngineError::Unsupported(UnsupportedReason::Mix)))); }
// error.rs
#[test] fn mix_message_is_exact() { assert_eq!(EngineError::Unsupported(UnsupportedReason::Mix).user_message(), "That's a YouTube Mix, which changes every time. Paste a regular playlist or single videos instead."); }
// select.rs
#[test] fn playlist_probe_lists_entries_in_order() {
    let Probed::Playlist { playlist } = to_probed(&info(FLAT)).unwrap() else { panic!() };
    assert_eq!(playlist.title, "Road Trip: Mix/Vol 1");
    assert_eq!(playlist.entries.iter().map(|e| e.index).collect::<Vec<_>>(), [1, 2, 3]);
    assert_eq!(playlist.entries[1].url, "https://www.youtube.com/watch?v=bbb");
}
#[test] fn video_probe_is_video() { assert!(matches!(to_probed(&info(SYNTH)).unwrap(), Probed::Video { .. })); }
// types.rs
#[test] fn probed_json_shape() { /* Probed::Playlist serializes with "type":"playlist" and a "playlist" field */ }
```
Update `engine_probe_returns_video_info` in `job.rs` tests to match `Probed::Video { info }`.

- [ ] **Step 3: Run to verify FAIL** — `cargo test -p engine --lib` — Expected: compile errors / FAIL
- [ ] **Step 4: Implement** the interfaces above. `run_steps` in `job.rs` must still refuse a playlist result for a job: if `info.kind == Some("playlist")` return `NoStream("video")`.
- [ ] **Step 5: Run to verify PASS** — `cargo test -p engine --lib`
- [ ] **Step 6: Commit** — `"feat(engine): playlist probing; reject channels and Mixes"`

---

### Task 3: Job input, file placement, outcome, shared fake backend

**Files:**
- Create: `engine/src/testing.rs` (`#[cfg(test)] pub(crate) mod testing;` in `lib.rs`)
- Modify: `engine/src/types.rs`, `engine/src/job.rs`, `engine/src/lib.rs`, `engine/tests/network.rs`

**Interfaces:**
- Consumes: `Quality`, `select_video_for` (Task 1); `sanitize_filename`, `unique_path` (V1).
- Produces (`types.rs`):
  - `DownloadKind` becomes `enum DownloadKind { Video { quality: Quality }, Mp3 }` (same serde tagging as V1). This type is also the user's saved **choice**.
  - `DownloadJob { url: String, kind: DownloadKind, folder: PathBuf, subfolder: Option<String>, prefix: Option<String> }`
- Produces (`job.rs`):
  - `pub enum JobOutcome { Done(PathBuf), Cancelled, Failed(EngineError) }`
  - `run_job(...)` keeps its V1 parameters and events, and now **returns `JobOutcome`** (the terminal event is still emitted).
- Produces (`testing.rs`): `pub(crate) struct FakeBackend` with V1's behavior (`block_in`, `fail_in`, `download_calls`) plus `private_urls: HashSet<String>` (a probe of one of these URLs returns `Err(EngineError::Unsupported(UnsupportedReason::Private))`, every other URL returns `json`) so a test can make one entry fail; `pub(crate) enum Stage { Download, Ffmpeg }`. V1's job tests move to use it.

File placement in `run_steps`: check `job.folder` exists (V1 `Folder` error) → `dir = job.folder` joined with `sanitize_filename(subfolder)` when present, created with `create_dir_all` (failure → `Io`) → `stem = sanitize_filename(title)` prefixed with `"{prefix} - "` when present → `unique_path(&dir, &stem, ext)`. Video selection uses `select_video_for(&info, quality)` (`None` → `NoStream("video")`).

- [ ] **Step 1: Write failing tests** (`job.rs`, using `FakeBackend`)

```rust
#[tokio::test] async fn best_quality_video_job() { /* kind Video{Best} → Done path ends with "Synthetic Video.mkv" (303 vp9 + opus) */ }
#[tokio::test] async fn playlist_job_goes_to_numbered_subfolder() {
    // subfolder Some("Road Trip"), prefix Some("01") → Done path == dir/"Road Trip"/"01 - Synthetic Video.mkv"
}
#[tokio::test] async fn playlist_job_reuses_folder_without_overwriting() {
    // pre-create dir/"Road Trip"/"01 - Synthetic Video.mkv" → Done path ends with "01 - Synthetic Video (1).mkv"; original untouched
}
#[tokio::test] async fn playlist_folder_name_is_sanitized() {
    // subfolder "a/b:c" → file lands in dir/"abc"; subfolder "???" → dir/"download"
}
#[tokio::test] async fn run_job_returns_outcome() {
    // success → JobOutcome::Done(path) matching the Done event; missing folder → JobOutcome::Failed(EngineError::Folder(_)); cancel → JobOutcome::Cancelled
}
```

- [ ] **Step 2: Run to verify FAIL** — `cargo test -p engine --lib job` — Expected: compile errors
- [ ] **Step 3: Implement** the interfaces and placement above; move `FakeBackend` to `testing.rs`; update all V1 job tests and `Engine` tests to the new `DownloadJob`; update `tests/network.rs` (`real_cancel_then_restart` uses `Video { quality: Quality::Best }`).
- [ ] **Step 4: Run to verify PASS** — `cargo test -p engine --lib`, then `$env:YTDL_TEST_URL="<url>"; cargo test -p engine --test network -- --ignored`
- [ ] **Step 5: Commit** — `"feat(engine): job quality choice, playlist placement, JobOutcome"`

---

### Task 4: Engine queue

**Files:**
- Create: `engine/src/queue.rs`
- Modify: `engine/src/lib.rs` (`pub mod queue; pub use queue::*;`), `engine/tests/network.rs`

**Interfaces:**
- Consumes: `run_job` → `JobOutcome`, `DownloadJob`, `DownloadKind`, `PlaylistInfo` (Tasks 2–3); `Backend` (V1); `FakeBackend` (Task 3).
- Produces (all camelCase serde; enums `tag = "type"`):
  - `EntryId(pub u64)`, `GroupId(pub u64)` — `Copy, Eq, Hash`, `#[serde(transparent)]`
  - `EntryState { Waiting, Running, Done { path: PathBuf }, Failed { msg: String } }`
  - `QueueEntry { id: EntryId, group: Option<GroupId>, url: String, title: String, prefix: Option<String>, state: EntryState }`
  - `QueueGroup { id: GroupId, title: String }`
  - `QueueStatus { Idle, Running, Paused }` (unit variants → plain strings, like V1's `Phase`)
  - `QueueEvent { Added { group: Option<QueueGroup>, entries: Vec<QueueEntry> }, Removed { ids: Vec<EntryId>, group: Option<GroupId> }, State { id: EntryId, state: EntryState }, Phase { id: EntryId, phase: Phase }, Progress { id: EntryId, done: u64, total: Option<u64> }, Status { status: QueueStatus } }`
  - `pub struct Queue<B: Backend = SystemBackend>` (cheaply `Clone`; internally `Arc`s), with:
    - `pub fn new(backend: Arc<B>, events: UnboundedSender<QueueEvent>) -> Self` (status Idle, choice `Video { quality: Best }`, empty folder)
    - `pub fn set_choice(&self, kind: DownloadKind)`, `pub fn set_folder(&self, folder: PathBuf)`
    - `pub fn add_video(&self, url: String, title: String) -> EntryId`
    - `pub fn add_playlist(&self, playlist: &PlaylistInfo) -> GroupId` — prefix = `index` zero-padded to `max(2, digits(entries.len()))`; the group's subfolder is the playlist title
    - `pub fn remove(&self, id: EntryId)`, `pub fn remove_group(&self, id: GroupId)`
    - `pub fn start(&self)` (must be called inside a tokio runtime), `pub fn stop(&self)`, `pub fn clear_finished(&self)`
    - `pub fn status(&self) -> QueueStatus`, `pub fn entries(&self) -> Vec<QueueEntry>`, `pub fn is_running(&self) -> bool` (the runner task is alive)

Runner rules (spec §5.4, §5.3):
- `start()`: if a runner is alive, or no entry is Waiting, do nothing. Otherwise set status Running (emit once) and spawn the runner.
- Runner loop: take the first Waiting entry; none → status Idle, exit. Mark it Running, read the **current** choice and folder, build its `DownloadJob` (group subfolder + prefix for playlist entries), run `run_job` with a fresh `CancellationToken` (kept so `stop`/`remove` can cancel), forward its `Phase`/`Progress` as `QueueEvent::Phase`/`Progress`. On the returned outcome:
  - `Done(path)` → `Done`.
  - `Failed(EngineError::Folder(_))` → `Failed { msg }`, status Paused, exit.
  - `Failed(e)` → `Failed { msg: e.user_message() }`, continue.
  - `Cancelled` → if the entry was removed, continue; if `stop()` was called, set it back to Waiting, status Paused, exit.
- `stop()`: if not running, do nothing; else mark stop-requested and cancel the running job.
- `remove(id)`: drop it from the list and emit `Removed`; if it's the running entry, cancel its job (the runner moves on after cleanup). Done/failed rows are just dropped; files are never touched.
- `remove_group(id)`: remove all its entries at once (one `Removed` event), cancelling the running one if inside.

- [ ] **Step 1: Write failing tests** (`queue.rs`, with `FakeBackend`; helper `collect_until_idle(rx)` that gathers events until `Status { Idle | Paused }`)

```rust
#[tokio::test] async fn runs_entries_in_order_one_at_a_time() { /* add 3 videos, start → States Running/Done in order a,b,c; never two Running at once; ends Idle */ }
#[tokio::test] async fn failed_entry_does_not_stop_queue() { /* probe for url "bad" fails (Private) → its Failed msg contains "private"; next entry Done; ends Idle */ }
#[tokio::test] async fn missing_folder_pauses_queue() { /* folder removed → first entry Failed with "Choose another folder", status Paused, second still Waiting; after set_folder(valid)+start → second Done */ }
#[tokio::test] async fn stop_puts_running_entry_back_to_waiting() { /* block_in Download; start; stop → entry Waiting, status Paused, no files left; start again → Done */ }
#[tokio::test] async fn removing_running_entry_cancels_cleans_up_and_continues() { /* block_in Ffmpeg on first; remove(first) → Removed; its files gone before second's Running; second Done */ }
#[tokio::test] async fn remove_group_removes_all_and_cancels_running_member() { /* playlist of 2 + single; remove_group while member runs → one Removed with both ids; single Done */ }
#[tokio::test] async fn added_while_running_joins_the_end() { /* add during first run → runs after */ }
#[tokio::test] async fn choice_is_read_when_each_entry_starts() { /* set_choice(Mp3) after first started → second's file ends ".mp3", first's ".mkv" */ }
#[tokio::test] async fn playlist_entries_get_padded_prefix_and_subfolder() { /* add_playlist with 3 entries (indices 1..3) → prefixes "01","02","03"; Done paths under dir/"Road Trip Mix Vol 1"... (sanitized title) */ }
#[tokio::test] async fn clear_finished_removes_done_and_failed_only() { /* Removed lists done+failed ids; waiting stays */ }
#[tokio::test] async fn start_and_stop_are_idempotent() { /* start with nothing waiting → no events; start twice → one Status Running; stop while idle → no events */ }
#[test] fn queue_event_json_shape() { /* Status serializes as {"type":"status","status":"running"}; State as {"type":"state","id":1,"state":{"type":"done","path":...}} */ }
```

- [ ] **Step 2: Run to verify FAIL** — `cargo test -p engine --lib queue` — Expected: compile errors
- [ ] **Step 3: Implement `queue.rs`** to the interfaces and rules above. Hold the queue's `std::sync::Mutex` only for short state changes, never across `.await`.
- [ ] **Step 4: Run to verify PASS** — `cargo test -p engine --lib`
- [ ] **Step 5: Add ignored network tests** to `engine/tests/network.rs`:
  - `real_playlist_listing`: `Engine::new().probe(env YTDL_TEST_PLAYLIST)` → `Probed::Playlist` with at least 1 entry.
  - `real_two_mp3_queue`: `Queue::new(Arc::new(SystemBackend::default()), tx)`, `set_choice(Mp3)`, add the first two entries of `YTDL_TEST_PLAYLIST`'s listing, start, wait for Idle → both Done, two `.mp3` files in the numbered subfolder.
  Run: `$env:YTDL_TEST_PLAYLIST="<playlist url>"; cargo test -p engine --test network -- --ignored real_playlist real_two` — Expected: PASS
- [ ] **Step 6: Commit** — `"feat(engine): download queue"`

---

### Task 5: App — queue commands, saved choice, keep-awake, close cleanup

**Files:**
- Create: `app/src-tauri/src/keep_awake.rs`
- Modify: `app/src-tauri/src/commands.rs`, `app/src-tauri/src/settings.rs`, `app/src-tauri/src/lib.rs`, `app/src-tauri/Cargo.toml`, `app/src-tauri/tauri.conf.json`
- Delete: `app/src-tauri/src/state.rs`

**Interfaces:**
- Consumes: `Engine::probe -> Probed`, `Queue` and its methods, `QueueEvent`, `QueueStatus`, `DownloadKind`, `Quality`, `PlaylistInfo`, `EntryId`, `GroupId` (Tasks 2–4).
- Produces (`settings.rs`): `pub const CHOICE: &str = "choice";` and `pub fn resolve_choice(saved: Option<serde_json::Value>) -> DownloadKind` — the saved value if it deserializes, else `Video { quality: Best }`.
- Produces (`keep_awake.rs`): `pub fn wants_awake(status: QueueStatus) -> bool` (true only for Running); `pub struct KeepAwake` with `pub fn new() -> Self` (spawns one dedicated `std::thread` that owns the execution state, because `SetThreadExecutionState` applies per thread) and `pub fn set(&self, awake: bool)` (sends to that thread: `ES_CONTINUOUS | ES_SYSTEM_REQUIRED` when true, `ES_CONTINUOUS` when false). Non-Windows builds compile to a no-op.
- Produces (Tauri commands; errors are `String` from `user_message()`):
  - `probe_link(url: String) -> Result<Probed, String>`
  - `add_video(url: String, title: String) -> EntryId`, `add_playlist(playlist: PlaylistInfo) -> GroupId`
  - `remove_entry(id: EntryId)`, `remove_group(id: GroupId)`, `start_queue()`, `stop_queue()`, `clear_finished()`
  - `get_choice() -> DownloadKind`, `set_choice(choice: DownloadKind)` (saves to store and calls `queue.set_choice`)
  - V1 commands kept: `missing_tools`, `get_folder` (also calls `queue.set_folder`), `pick_folder` (also calls `queue.set_folder`), `reveal`
  - Removed: `probe`, `start_download`, `cancel_download`
- Produces (event): `"queue-event"` with a `QueueEvent` payload

`lib.rs`: in `.setup()`, create the `QueueEvent` channel, build `Queue::new(Arc::new(SystemBackend::default()), tx)`, apply the saved choice, `manage` it with `Engine::new()` and `KeepAwake::new()`, and spawn a forwarder that emits each event and calls `keep_awake.set(wants_awake(status))` on `QueueEvent::Status`. The V1 exit hook now uses `queue.is_running()`: on the first `ExitRequested` while running → `prevent_exit`, `queue.stop()`, wait (≤ 10 s, polling `is_running`), `keep_awake.set(false)`, `app.exit(0)`. `tauri.conf.json`: window `width 460`, `height 720`, `minWidth 420`, `minHeight 600`. `Cargo.toml`: `windows-sys` with feature `Win32_System_Power` under `[target.'cfg(windows)'.dependencies]`.

- [ ] **Step 1: Write failing tests**

```rust
// settings.rs
#[test] fn resolve_choice_defaults_to_video_best() { assert_eq!(resolve_choice(None), DownloadKind::Video { quality: Quality::Best }); }
#[test] fn resolve_choice_uses_saved() { assert_eq!(resolve_choice(Some(serde_json::json!({"type":"mp3"}))), DownloadKind::Mp3); }
#[test] fn resolve_choice_ignores_garbage() { assert_eq!(resolve_choice(Some(serde_json::json!("nonsense"))), DownloadKind::Video { quality: Quality::Best }); }
// keep_awake.rs
#[test] fn awake_only_while_running() { assert!(wants_awake(QueueStatus::Running)); assert!(!wants_awake(QueueStatus::Idle)); assert!(!wants_awake(QueueStatus::Paused)); }
#[test] fn keep_awake_set_does_not_panic() { let k = KeepAwake::new(); k.set(true); k.set(false); }
```

- [ ] **Step 2: Run to verify FAIL** — `cargo test -p app` — Expected: compile errors
- [ ] **Step 3: Implement** the interfaces above; delete `state.rs` and its tests.
- [ ] **Step 4: Run to verify PASS** — `cargo test -p app` and `cargo test --workspace`
- [ ] **Step 5: Commit** — `"feat(app): queue commands, saved choice, keep-awake, close cleanup"`

---

### Task 6: Frontend queue state

**Files:**
- Modify: `app/src/types.ts`, `app/src/state.ts`, `app/src/state.test.ts` (V1 single-job tests replaced)

**Interfaces:**
- Consumes: JSON shapes pinned in Tasks 1, 2, 4 (`quality_json_shape`, `probed_json_shape`, `queue_event_json_shape`).
- Produces (`types.ts`): `Quality`, `Choice` (= Rust `DownloadKind`: `{type:'video', quality: Quality} | {type:'mp3'}`), `PlaylistEntry`, `PlaylistInfo`, `Probed`, `EntryState`, `QueueEntry`, `QueueGroup`, `QueueStatus = 'idle'|'running'|'paused'`, `QueueEvent`, plus V1's `VideoInfo`, `Phase`. V1's `DownloadJob`, `JobEvent`, `VideoOption` usage in the UI is removed.
- Produces (`state.ts`):
  ```ts
  export interface Row extends QueueEntry { phase?: Phase; percent: number | null }
  export interface State {
    link: 'idle' | 'reading'; linkUrl: string; linkError?: string;
    choice: Choice; folder: string; missingTools: string[];
    rows: Row[]; groups: QueueGroup[]; status: QueueStatus; lastDonePath?: string; openFailure?: number;
  }
  export type Action =
    | { type: 'toolsChecked'; missing: string[] } | { type: 'folderChanged'; folder: string }
    | { type: 'choiceChanged'; choice: Choice }
    | { type: 'linkSubmitted'; url: string } | { type: 'linkRead'; url: string } | { type: 'linkFailed'; url: string; message: string }
    | { type: 'linkCleared' } | { type: 'failureToggled'; id: number }
    | { type: 'queueEvent'; event: QueueEvent };
  export const initialState: State;   // choice {type:'video', quality:{type:'best'}}, status 'idle', link 'idle'
  export function reduce(s: State, a: Action): State;
  export function waitingCount(s: State): number;
  export function canStart(s: State): boolean;   // status !== 'running', waitingCount > 0, folder non-empty, no missing tools
  export function canAdd(s: State): boolean;     // link 'idle' and no missing tools
  export function totals(s: State): { done: number; failed: number; waiting: number };
  ```
  Rules: `linkSubmitted` while `reading` is ignored (Add can't fire twice) and a blank url clears; `linkRead`/`linkFailed` apply only to the current `linkUrl` (stale results ignored); `linkRead` clears the box. `Added` appends rows (and the group); `Removed` drops ids (and the group); `State` sets state (Done also sets `lastDonePath`, resets percent); `Phase` sets phase and percent 0; `Progress` sets percent (`null` when total unknown); `Status` sets status.

- [ ] **Step 1: Write failing tests** (`state.test.ts`, with small `entry(id, opts)` / `added(...)` helpers)

```ts
test('add is ignored while a link is being read', ...)            // linkSubmitted u1, then u2 → linkUrl stays u1
test('stale link result is ignored', ...)                          // linkSubmitted u1, linkCleared, linkRead u1 → link 'idle', no change
test('link failure shows the message and keeps the box', ...)
test('queue events build and update rows', ...)                    // Added(2) → 2 waiting; State Running; Progress 40/100 → 40; Phase → 0; Done → lastDonePath
test('removed drops rows and the group', ...)
test('canStart', ...)                                              // false with no waiting, no folder, missing tools, or while running; true when paused with waiting
test('totals count done, failed and waiting', ...)
test('choice change is stored', ...)
```

- [ ] **Step 2: Run to verify FAIL** — `cd app; npm test`
- [ ] **Step 3: Implement `types.ts` and `state.ts`**
- [ ] **Step 4: Run to verify PASS** — `npm test` and `npx tsc --noEmit -p .`
- [ ] **Step 5: Commit** — `"feat(ui): queue state reducer"`

---

### Task 7: Layout A, wiring, docs, manual check

**Files:**
- Modify: `app/index.html`, `app/src/styles.css`, `app/src/view.ts`, `app/src/main.ts`, `app/src/api.ts`, `app/src/format.ts` (+ test), `docs/manual-test-checklist.md`, `README.md`

**Interfaces:**
- Consumes: Task 5 command names and `"queue-event"`; Task 6 `reduce`, `canStart`, `canAdd`, `totals`, `waitingCount`.
- Produces (`format.ts`): `qualityLabel(q: Quality): string` → `"Best"`, `"Up to 1080p"`, `"Up to 720p"`, `"Smallest"`; `QUALITY_CHOICES: Quality[]` in that order (Best, UpTo 1080, UpTo 720, Smallest).

- [ ] **Step 1: Write failing test** (`format.test.ts`): `qualityLabel` returns the four labels above; `QUALITY_CHOICES` has exactly those four in order.
- [ ] **Step 2: Run to verify FAIL**, implement, run to verify PASS — `npm test`
- [ ] **Step 3: Build the screen to spec §5.7 (layout A):** link box with ✕ and **Add**; "Getting video info…" while reading; link error line; Video | MP3 toggle; quality `<select>` (hidden for MP3); folder button + path; **Start (N waiting)**; the queue list (group rows with 📂 title, count and ✕; entry rows with `NN ·` prefix for playlist entries, title, state text or progress bar + percent, and ✕ on every row with spec §5.4's meaning per state); empty-queue hint "Paste a playlist or video link to start."; footer totals, **Stop** (while running), **Clear finished**, **Open folder** (enabled when `lastDonePath`); clicking a failed row toggles its full message. The list scrolls on its own. Keep V1's missing-tools panel and theming.
- [ ] **Step 4: Wire `main.ts`:** on load `missingTools`, `getFolder`, `getChoice`, listen to `"queue-event"`. Paste/Enter/Add → `linkSubmitted` then `probeLink(url)` → on `Probed.video` call `addVideo(url, info.title)`, on `Probed.playlist` call `addPlaylist(playlist)`, then `linkRead`; on error `linkFailed`. ✕/Esc in the box → `linkCleared`. Choice changes → `setChoice`. Start/Stop/Clear/row ✕/group ✕ → the matching commands. Open folder → `reveal(lastDonePath)`.
- [ ] **Step 5: Update `docs/manual-test-checklist.md`** for V2: keep V1 items that still apply (tools panel, theme, resize, clear link, installed app, cancel cleanup) and add: real playlist downloads into its numbered folder; a Mix link shows the Mix message; a channel link shows the channel message; a private entry fails and the queue continues; Stop then Start resumes; ✕ on the running row continues with the next; ✕ on a group; Clear finished; quality change mid-queue affects only waiting rows; closing mid-queue leaves no `.part` and no `ffmpeg.exe`; `powercfg /requests` (in an admin PowerShell) lists the app while the queue runs and not after. README: one paragraph on the queue.
- [ ] **Step 6: Verify** — `cd app; npm test; npx tsc --noEmit -p .; cd ..; cargo test --workspace`; then `cd app; npm run tauri dev` and walk the checklist with Donny. Expected: all automated tests pass; every checklist item checked.
- [ ] **Step 7: Commit** — `"feat(ui): queue screen (layout A), wiring, V2 checklist"`

---

## Amendment tasks (spec §11)

### Task 8: Engine — video-in-playlist probing, private-playlist message

**Files:** Modify `engine/src/backend.rs`, `engine/src/tools.rs`, `engine/src/types.rs`, `engine/src/error.rs`, `engine/src/ytdlp.rs`, `engine/src/lib.rs`, `engine/src/testing.rs`; new fixtures in `engine/tests/fixtures/` as needed.

**Interfaces:**
- `Backend` gains `async fn probe_playlist_json(&self, url: &str) -> Result<String, EngineError>`. `SystemBackend` runs `yt-dlp -J --yes-playlist --flat-playlist <url>` via a new `playlist_probe_args(url) -> [&str; 4]` beside `probe_args`. `FakeBackend` gains `playlist_json: Option<&'static str>` (None → returns `json`).
- `UnsupportedReason::PlaylistUnavailable`, message exactly: "This playlist is private or doesn't exist." `classify_ytdlp_error` maps stderr containing "The playlist does not exist" to it.
- `types.rs`: `#[serde(tag = "type", rename_all = "camelCase")] enum PlaylistLookup { Found { playlist: PlaylistInfo }, Mix, Unavailable }`; `Probed` gains `VideoInPlaylist { info: VideoInfo, lookup: PlaylistLookup }`.
- `Engine::probe(url)`: a URL with both a video id (`v=`) and a `list=` parameter → probe the video with `probe_json`; then if the list id starts with `RD` → `lookup: Mix` (no second probe); otherwise `probe_playlist_json`: a playlist result → `Found`; a video result, or `Unsupported(PlaylistUnavailable)` → `Unavailable`. All other URLs behave as before.

- [ ] **Step 1: Failing tests:** `playlist_probe_args_exact` (`["-J","--yes-playlist","--flat-playlist","U"]`); `classify_playlist_does_not_exist` ("ERROR: [youtube:tab] PLx: YouTube said: The playlist does not exist." → `PlaylistUnavailable`); `probe_video_in_playlist_found`, `probe_video_in_playlist_mix` (and the playlist probe is not called), `probe_video_in_playlist_unavailable` (FakeBackend `json` = SYNTH video, `playlist_json` = FLAT / SYNTH); `probed_video_in_playlist_json_shape` (`{"type":"videoInPlaylist","info":{…},"lookup":{"type":"found","playlist":{…}}}`).
- [ ] **Step 2: RED**, implement, **GREEN** — `cargo test --workspace`
- [ ] **Step 3: Commit** — `"feat(engine): ask-about-playlist probing, private playlist message"`

### Task 9: Frontend — playlist question, remembered quality

**Files:** Modify `app/src/types.ts`, `app/src/state.ts`, `app/src/state.test.ts`, `app/src/link.ts`, `app/src/link.test.ts`, `app/src/view.ts`, `app/src/main.ts`, `app/src/api.ts`, `app/src/styles.css`, `app/index.html`; `app/src-tauri/src/commands.rs`, `app/src-tauri/src/settings.rs`, `app/src-tauri/src/lib.rs`.

**Rules:**
- Saved video quality: store key `videoQuality` (a `Quality`, default Best when missing or invalid). New command `get_video_quality() -> Quality`; `set_choice` also saves `videoQuality` whenever it receives a video choice. Test the resolve helper like `resolve_choice`.
- `link.ts`: on `Probed.videoInPlaylist`, add nothing; dispatch `linkAsk { url, info, lookup }`. State gains `ask?: { url: string; info: VideoInfo; lookup: PlaylistLookup }`. **Whole playlist** (only when `lookup.type === 'found'`) → `add_playlist(lookup.playlist)` then `linkRead`; **Just this video** → `add_video(<the video's own URL: https://www.youtube.com/watch?v=<id> taken from the submitted URL>, info.title)` then `linkRead`; ✕/Esc → `linkCleared` (clears `ask`). The submit token guards these: a cleared ask never adds.
- View: the question replaces the "Getting video info…" line, with the exact texts from spec §11.1 (Found: "This video is part of the playlist “<title>” (<N> videos)."; Mix: the Mix message; Unavailable: "This playlist is private or doesn't exist.").
- Quality: state keeps `videoQuality` (loaded with `get_video_quality`); MP3 → Video restores it; choosing a quality updates it.
- [ ] **Step 1: Failing tests:** reducer — `linkAsk` sets `ask` and `link` back to `'idle'`... (implementer's call on the exact link state, but Add/paste must work again after an ask is answered or cleared); clearing removes `ask`; MP3 → Video restores `videoQuality`. link.test.ts — whole playlist adds the playlist; just this video adds the plain watch URL; clear during an ask adds nothing; a Mix/Unavailable ask has no whole-playlist path. Rust — `resolve_video_quality` defaults to Best.
- [ ] **Step 2: RED**, implement, **GREEN** — `npm test`, `npx tsc --noEmit -p .` zero errors, `cargo test --workspace`; smoke launch
- [ ] **Step 3: Commit** — `"feat(ui): ask whole playlist or just this video; remember quality"`

### Task 10: App — confirm before closing mid-queue

**Files:** Modify `app/src-tauri/src/lib.rs` (+ a small pure helper and its tests).

**Rules:** on the main window's `CloseRequested`, if any queue entry is Running or Waiting: `prevent_close`, then show a native dialog (tauri-plugin-dialog, from Rust) with the text "Downloads are in progress. Close anyway?" and buttons **Close** / **Keep downloading**. Close → run the existing exit cleanup (stop, wait ≤10 s, keep-awake off, exit). Keep downloading → nothing. Otherwise close immediately. Helper `pub fn needs_close_confirm(entries: &[QueueEntry]) -> bool`.
- [ ] **Step 1: Failing tests:** `needs_close_confirm` — empty → false; only done/failed → false; any waiting → true; any running → true.
- [ ] **Step 2: RED**, implement, **GREEN** — `cargo test --workspace`; smoke launch
- [ ] **Step 3: Commit** — `"feat(app): confirm before closing with downloads in progress"`
