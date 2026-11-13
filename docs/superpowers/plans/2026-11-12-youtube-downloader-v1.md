# YouTube Downloader V1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Windows desktop app (Tauri 2) where Donny pastes a YouTube URL, picks Video (at a real resolution) or MP3, picks a folder, and downloads one video at a time with progress and Cancel.

**Architecture:** A Cargo workspace with two crates. `engine` is pure Rust with no Tauri dependency: it runs `yt-dlp -J` for info and stream URLs, downloads streams itself with `reqwest` in ranged chunks, and runs ffmpeg to merge or convert. `app` is the Tauri crate: it exposes engine calls as commands and forwards engine `JobEvent`s to a plain-TypeScript frontend.

**Tech Stack:** Rust (stable, edition 2021), tokio, reqwest, serde/serde_json, thiserror, tokio-util (`CancellationToken`), async-trait; tests with tempfile and wiremock. Tauri 2 with tauri-plugin-store, tauri-plugin-dialog, tauri-plugin-opener. Frontend: TypeScript + Vite, no framework; tests with vitest.

**Spec:** `docs/superpowers/specs/2026-11-12-youtube-downloader-design.md`

## Global Constraints

- Windows 10/11 only. Commands in this plan are for PowerShell.
- yt-dlp, ffmpeg, and Deno are installed by the user on PATH. Never bundle, download, or install them from the app.
- Probe command is exactly `yt-dlp -J --no-playlist <url>`.
- Never re-encode video: merge with ffmpeg `-c copy`. MP3 uses `libmp3lame -q:a 0`.
- Container: MP4 only when video codec is H.264 (`avc1…`) **and** audio is AAC (`mp4a…`); otherwise MKV.
- Every child process (yt-dlp, ffmpeg, deno) is started with the Windows `CREATE_NO_WINDOW` flag (`0x0800_0000`) so no console window flashes, and with `kill_on_drop(true)`.
- `engine` must not depend on `tauri` or any `tauri-*` crate.
- Every `JobEvent` carries a `job_id`. The **app** (not the engine) refuses a second job while one runs.
- Files are never overwritten. Temp files are `*.part` in the target folder and are removed on success, failure, and cancel.
- User-facing errors are plain language with a suggested next step. Any generic yt-dlp failure suggests running `yt-dlp -U`.
- Window: about 420×600, resizable. Theme follows Windows (`prefers-color-scheme`).
- Last folder is stored with tauri-plugin-store in `settings.json` under key `lastFolder`. First-run default is the user's Downloads folder.
- Exact crate versions are the builder's choice: add dependencies with `cargo add` / `npm install` (latest).
- **Learning rule (spec §4 anti-goal):** after each task's commit, the implementer explains to Donny in plain language what changed, which files, and any Rust concept that appeared for the first time (for example `Result`, traits, `async`, ownership of a `PathBuf`). Wait for Donny's go-ahead before the next task.

## Review Focus

1. **Cancel while ffmpeg is merging or converting:** ffmpeg is killed and no partial output or `.part` file is left. Test: Task 7 `cancel_during_merge_cleans_up`.
2. **A stream fails mid-download** (expired URL returns 403, or the connection drops early): the job fails with a plain message and no truncated file is left. Tests: Task 5 `http_403_is_error`, `short_body_is_error`; Task 7 `failure_reports_message_and_cleans_up`.
3. **Saved folder no longer exists** (deleted, or an unplugged drive): on startup the app falls back to Downloads; if it disappears after being chosen, the job fails before any download starts. Tests: Task 8 `resolve_folder_falls_back_when_missing`; Task 7 `missing_folder_fails_before_download`.
4. **Pasting again before a probe returns, or during a download:** only the latest URL's probe result is shown; pasting during a download is ignored. Tests: Task 9 `stale_probe_result_is_ignored`, `paste_during_download_is_ignored`.
5. **Age-restricted, private, members-only, live, or playlist URLs:** a specific message, not a raw yt-dlp dump. Tests: Task 2 `classify_*`, `rejects_live`, `rejects_playlist`.

---

## File Structure

```
Cargo.toml                      workspace: members = ["engine", "app/src-tauri"]
.gitignore                      target/, node_modules/, app/dist/
README.md                       prerequisites + how to run
docs/manual-test-checklist.md   UI checklist (Task 10)
engine/
  Cargo.toml
  examples/spike.rs             Task 1 risk spike
  src/lib.rs                    Engine struct: probe / start / cancel; re-exports
  src/types.rs                  VideoInfo, VideoOption, DownloadJob, JobId, JobEvent…
  src/error.rs                  EngineError + user_message()
  src/ytdlp.rs                  raw yt-dlp JSON model, parse, error classification
  src/select.rs                 resolution list, stream choice, container choice
  src/naming.rs                 filename cleaning, unique paths
  src/download.rs               ranged HTTP download with progress + cancel
  src/tools.rs                  process spawning, yt-dlp probe, ffmpeg, tool check
  src/backend.rs                Backend trait + SystemBackend
  src/job.rs                    run_job: phases, events, cleanup
  tests/fixtures/synthetic.json
  tests/fixtures/combined_only.json
  tests/fixtures/live.json
  tests/fixtures/playlist.json
  tests/network.rs              #[ignore] real-video tests
app/                            Vite + TS frontend (create-tauri-app vanilla-ts)
  index.html
  src/types.ts                  TS mirror of engine JSON
  src/state.ts                  pure reducer (tested)
  src/state.test.ts
  src/api.ts                    invoke/listen wrappers
  src/view.ts                   render(state)
  src/main.ts                   wiring
  src/styles.css
  src-tauri/                    crate "app"
    tauri.conf.json
    src/main.rs, src/lib.rs     builder, plugins, commands registration
    src/commands.rs             Tauri commands
    src/state.rs                ActiveJob guard (tested)
    src/settings.rs             resolve_folder (tested)
```

---

### Task 1: Repo setup and the stream-download spike

The spec (§10) makes this the gate: if Rust can't download YouTube streams at full speed, the engine internals switch to approach #1 before anything else is built.

**Files:**
- Create: `Cargo.toml`, `.gitignore`, `README.md`, `engine/Cargo.toml`, `engine/src/lib.rs` (empty), `engine/examples/spike.rs`

**Interfaces:**
- Produces: workspace with an `engine` library crate (later tasks add modules to it).

- [ ] **Step 1: Check prerequisites**

Run each: `rustc --version`, `cargo --version`, `node --version`, `yt-dlp --version`, `ffmpeg -version`, `deno --version`, `git --version`.
Expected: all print versions. Also confirm "Desktop development with C++" (MSVC build tools) is installed — Tauri needs it on Windows. If yt-dlp is old, run `yt-dlp -U`.

- [ ] **Step 2: Initialize git and the workspace**

`git init`. Root `Cargo.toml` is a workspace with `members = ["engine"]` and `resolver = "2"` (Task 8 adds `app/src-tauri`). `cargo new engine --lib`. `.gitignore`: `target/`, `node_modules/`, `app/dist/`. In `engine`: `cargo add serde --features derive`, `cargo add serde_json thiserror async-trait futures-util tokio-util`, `cargo add tokio --features full`, `cargo add reqwest --features stream`, `cargo add --dev tempfile wiremock`.

README: one paragraph on what the app is, the prerequisites list from Step 1 (with `yt-dlp -U` and the note that Deno is required by yt-dlp for YouTube), and "personal use only" from spec §5.

- [ ] **Step 3: Write `engine/examples/spike.rs`**

Usage: `cargo run -p engine --example spike -- <youtube-url>`. It:
1. Runs `yt-dlp -J --no-playlist <url>` and parses stdout as `serde_json::Value`.
2. From `formats`, takes the **last** format with `vcodec == "none"`, `acodec != "none"`, `protocol == "https"` (best audio), and the last with `acodec == "none"`, `vcodec != "none"`, `protocol == "https"` (best video).
3. For each: sends two GETs with that format's `http_headers` plus `Range: bytes=0-10485759` and `Range: bytes=10485760-20971519`, reads the full body, and prints status code, bytes received, seconds, and MB/s.

- [ ] **Step 4: Run the spike on a public video Donny is allowed to download**

Run: `cargo run -p engine --example spike -- "<url>"`
**Pass:** every request returns `206`, each chunk is 10 MiB (or the rest of the file), and throughput is well above 1 MB/s.
**Fail (403, or a crawl around 50–100 KB/s):** STOP. Tell Donny. Approach #1 replaces only the internals of Task 5 and `SystemBackend::download` in Task 7 (yt-dlp downloads; progress parsed from its output). Re-plan those two before continuing.

- [ ] **Step 5: Commit**

```powershell
git add .
git commit -m "chore: workspace, README, stream-download spike"
```

---

### Task 2: Engine types, errors, and yt-dlp JSON parsing

**Files:**
- Create: `engine/src/types.rs`, `engine/src/error.rs`, `engine/src/ytdlp.rs`, `engine/tests/fixtures/{synthetic,combined_only,live,playlist}.json`
- Modify: `engine/src/lib.rs` (declare modules, `pub use types::*; pub use error::*;`)

**Interfaces:**
- Produces (`types.rs`, all `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]`, structs `#[serde(rename_all = "camelCase")]`, enums `#[serde(tag = "type", rename_all = "camelCase")]` unless noted):
  - `VideoInfo { title: String, duration_secs: Option<f64>, thumbnail_url: Option<String>, video_options: Vec<VideoOption>, has_audio: bool }`
  - `VideoOption { label: String, height: u32, fps: u32, codec: String, est_size: Option<u64> }`
  - `DownloadKind { Video { option: VideoOption }, Mp3 }`
  - `DownloadJob { url: String, kind: DownloadKind, folder: PathBuf }`
  - `JobId(pub u64)` — also `Copy, Eq, Hash`, `#[serde(transparent)]`
  - `Phase { Probing, DownloadingVideo, DownloadingAudio, Merging, Converting }` — `#[serde(rename_all = "camelCase")]`, unit variants serialize as plain strings
  - `JobEventKind { Phase { phase: Phase }, Progress { done: u64, total: Option<u64> }, Done { path: PathBuf }, Failed { msg: String }, Cancelled }`
  - `JobEvent { job_id: JobId, kind: JobEventKind }`; `impl JobEvent { pub fn is_terminal(&self) -> bool }` (Done, Failed, Cancelled)
- Produces (`error.rs`): `EngineError` (thiserror) with variants `ToolMissing(String)`, `Unsupported(UnsupportedReason)`, `YtDlp(String)`, `NoStream(&'static str)`, `Http(String)`, `Ffmpeg(String)`, `Folder(PathBuf)`, `Io(#[from] std::io::Error)`, `Cancelled`; `UnsupportedReason { AgeRestricted, Private, MembersOnly, Live, Playlist }`; `impl EngineError { pub fn user_message(&self) -> String }`
- Produces (`ytdlp.rs`):
  - `RawInfo { #[serde(rename = "_type")] kind: Option<String>, #[serde(default)] title: String, duration: Option<f64>, thumbnail: Option<String>, #[serde(default)] formats: Vec<RawFormat>, is_live: Option<bool>, live_status: Option<String> }`
  - `RawFormat { format_id: String, url: Option<String>, protocol: Option<String>, ext: Option<String>, vcodec: Option<String>, acodec: Option<String>, height: Option<u32>, fps: Option<f64>, filesize: Option<u64>, filesize_approx: Option<u64>, #[serde(default)] http_headers: HashMap<String, String> }` (Deserialize, Clone, Debug)
  - `pub fn parse_info(json: &str) -> Result<RawInfo, EngineError>` — rejects playlist (`kind == Some("playlist")`) and live (`is_live == Some(true)` or `live_status` is `"is_live"`/`"is_upcoming"`) as `Unsupported`
  - `pub fn classify_ytdlp_error(stderr: &str) -> EngineError`

- [ ] **Step 1: Create fixtures**

`synthetic.json` — yt-dlp lists formats worst → best, so order matters:

```json
{"title":"Synthetic Video","duration":754.0,"thumbnail":"https://i.ytimg.com/vi/x/maxresdefault.jpg","formats":[
{"format_id":"sb0","protocol":"mhtml","ext":"mhtml","vcodec":"none","acodec":"none","url":"https://x/sb"},
{"format_id":"139","protocol":"https","ext":"m4a","vcodec":"none","acodec":"mp4a.40.5","filesize":1000000,"url":"https://x/139","http_headers":{"User-Agent":"UA"}},
{"format_id":"140","protocol":"https","ext":"m4a","vcodec":"none","acodec":"mp4a.40.2","filesize":4000000,"url":"https://x/140","http_headers":{"User-Agent":"UA"}},
{"format_id":"251","protocol":"https","ext":"webm","vcodec":"none","acodec":"opus","filesize":4200000,"url":"https://x/251","http_headers":{"User-Agent":"UA"}},
{"format_id":"18","protocol":"https","ext":"mp4","vcodec":"avc1.42001E","acodec":"mp4a.40.2","height":360,"fps":30,"url":"https://x/18"},
{"format_id":"136","protocol":"https","ext":"mp4","vcodec":"avc1.4d401f","acodec":"none","height":720,"fps":30,"filesize":20000000,"url":"https://x/136"},
{"format_id":"247","protocol":"https","ext":"webm","vcodec":"vp9","acodec":"none","height":720,"fps":30,"filesize":18000000,"url":"https://x/247"},
{"format_id":"298","protocol":"https","ext":"mp4","vcodec":"avc1.4d4020","acodec":"none","height":720,"fps":60,"filesize":30000000,"url":"https://x/298"},
{"format_id":"137","protocol":"https","ext":"mp4","vcodec":"avc1.640028","acodec":"none","height":1080,"fps":30,"filesize":50000000,"url":"https://x/137"},
{"format_id":"248","protocol":"https","ext":"webm","vcodec":"vp9","acodec":"none","height":1080,"fps":30,"filesize":45000000,"url":"https://x/248"},
{"format_id":"299","protocol":"https","ext":"mp4","vcodec":"avc1.64002a","acodec":"none","height":1080,"fps":60,"filesize":80000000,"url":"https://x/299"},
{"format_id":"303","protocol":"https","ext":"webm","vcodec":"vp9","acodec":"none","height":1080,"fps":59.94,"filesize_approx":75000000,"url":"https://x/303"},
{"format_id":"96","protocol":"m3u8_native","ext":"mp4","vcodec":"avc1.640028","acodec":"mp4a.40.2","height":1080,"fps":30,"url":"https://x/96"}]}
```

`combined_only.json`: title "Old Video", formats = only the `"18"` entry above. `live.json`: title "Live", `"is_live": true`, formats = `[]`. `playlist.json`: `{"_type":"playlist","title":"A Playlist","entries":[]}`.

- [ ] **Step 2: Write failing tests** (unit tests at the bottom of each module; read fixtures with `include_str!("../tests/fixtures/…")`)

```rust
// ytdlp.rs
#[test] fn parses_synthetic() { let i = parse_info(SYNTH).unwrap(); assert_eq!(i.title, "Synthetic Video"); assert_eq!(i.formats.len(), 13); assert_eq!(i.formats[3].http_headers["User-Agent"], "UA"); }
#[test] fn rejects_live() { assert!(matches!(parse_info(LIVE), Err(EngineError::Unsupported(UnsupportedReason::Live)))); }
#[test] fn rejects_playlist() { assert!(matches!(parse_info(PLAYLIST), Err(EngineError::Unsupported(UnsupportedReason::Playlist)))); }
#[test] fn invalid_json_is_ytdlp_error() { assert!(matches!(parse_info("not json"), Err(EngineError::YtDlp(_)))); }
#[test] fn classify_age() { assert!(matches!(classify_ytdlp_error("ERROR: [youtube] abc: Sign in to confirm your age. This video may be inappropriate for some users."), EngineError::Unsupported(UnsupportedReason::AgeRestricted))); }
#[test] fn classify_private() { /* "ERROR: [youtube] abc: Private video. Sign in if you've been granted access to this video" → Private */ }
#[test] fn classify_members() { /* "ERROR: [youtube] abc: Join this channel to get access to members-only content like this video" → MembersOnly */ }
#[test] fn classify_other_keeps_last_line() { assert_eq!(classify_ytdlp_error("WARNING: x\nERROR: something broke\n").to_string(), "yt-dlp failed: ERROR: something broke"); }

// error.rs
#[test] fn ytdlp_message_suggests_update() { assert!(EngineError::YtDlp("x".into()).user_message().contains("yt-dlp -U")); }
#[test] fn folder_message_suggests_new_folder() { assert!(EngineError::Folder("D:\\gone".into()).user_message().contains("Choose another folder")); }
#[test] fn age_message_is_specific() { assert!(EngineError::Unsupported(UnsupportedReason::AgeRestricted).user_message().to_lowercase().contains("age-restricted")); }

// types.rs — pins the JSON contract the TypeScript side mirrors
#[test] fn job_event_json_shape() {
    let e = JobEvent { job_id: JobId(7), kind: JobEventKind::Progress { done: 5, total: Some(10) } };
    assert_eq!(serde_json::to_value(&e).unwrap(), serde_json::json!({"jobId":7,"kind":{"type":"progress","done":5,"total":10}}));
    let p = JobEvent { job_id: JobId(7), kind: JobEventKind::Phase { phase: Phase::DownloadingVideo } };
    assert_eq!(serde_json::to_value(&p).unwrap()["kind"]["phase"], "downloadingVideo");
}
#[test] fn download_kind_json_shape() { assert_eq!(serde_json::to_value(DownloadKind::Mp3).unwrap(), serde_json::json!({"type":"mp3"})); }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p engine`
Expected: compile errors / FAIL (items not defined).

- [ ] **Step 4: Implement `types.rs`, `error.rs`, `ytdlp.rs` to the interfaces above**

`user_message()` wording is the builder's choice (spec §9) but must satisfy the tests; `ToolMissing` names the tool and says to install it and restart; `Http` suggests trying again; `Cancelled` is "Download cancelled."

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p engine` — Expected: all PASS.

- [ ] **Step 6: Commit**

`git add engine` then `git commit -m "feat(engine): types, errors, yt-dlp JSON parsing"`

---

### Task 3: Resolution list and stream selection

**Files:**
- Create: `engine/src/select.rs`
- Modify: `engine/src/lib.rs` (add `mod select;`)

**Interfaces:**
- Consumes: `RawInfo`, `RawFormat`, `VideoInfo`, `VideoOption`, `EngineError` (Task 2)
- Produces:
  - `pub fn fps_class(fps: Option<f64>) -> u32` — rounded fps if > 30, else 0
  - `pub fn video_options(info: &RawInfo) -> Vec<VideoOption>`
  - `pub fn select_video(info: &RawInfo, height: u32, fps: u32) -> Option<&RawFormat>` — matches on height and `fps_class`
  - `pub fn select_audio(info: &RawInfo) -> Option<&RawFormat>`
  - `pub enum Container { Mp4, Mkv }` with `pub fn ext(&self) -> &'static str` ("mp4"/"mkv")
  - `pub fn container_for(vcodec: &str, acodec: &str) -> Container`
  - `pub fn to_video_info(info: &RawInfo) -> Result<VideoInfo, EngineError>`

Rules: only formats with `protocol == Some("https")` and a `url` are usable. Video-only = `acodec == "none"` and `vcodec != "none"`. Audio-only = `vcodec == "none"` and `acodec != "none"`. "Best" = the **last** matching format in `formats` (yt-dlp's ordering). One option per distinct `(height, fps_class)`, ordered height desc then fps desc. Label = `"{height}p"` plus the fps when `fps_class > 0` (`"1080p60"`). `VideoOption.fps` = rounded fps of the best format; `codec` = its `vcodec`; `est_size` = (video `filesize` or `filesize_approx`) + best audio size when both are known, else `None`. `to_video_info` returns `Err(NoStream("video or audio"))` when there are no video options and no audio-only stream.

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn options_from_synthetic() {
    let o = video_options(&info(SYNTH));
    let labels: Vec<_> = o.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(labels, ["1080p60", "1080p", "720p60", "720p"]);
    assert_eq!(o[0].fps, 60);
    assert_eq!(o[0].codec, "vp9");
    assert_eq!(o[0].est_size, Some(79_200_000));   // 303 approx + 251
    assert_eq!(o[1].est_size, Some(49_200_000));   // 248 + 251
}
#[test] fn picks_best_video_per_option() { let i = info(SYNTH); assert_eq!(select_video(&i, 1080, 60).unwrap().format_id, "303"); assert_eq!(select_video(&i, 720, 0).unwrap().format_id, "247"); assert!(select_video(&i, 480, 0).is_none()); }
#[test] fn picks_best_audio() { assert_eq!(select_audio(&info(SYNTH)).unwrap().format_id, "251"); }
#[test] fn ignores_combined_storyboard_and_hls() { assert!(video_options(&info(SYNTH)).iter().all(|o| o.height != 360)); }
#[test] fn fps_classes() { assert_eq!(fps_class(Some(59.94)), 60); assert_eq!(fps_class(Some(30.0)), 0); assert_eq!(fps_class(Some(25.0)), 0); assert_eq!(fps_class(None), 0); }
#[test] fn container_rules() { assert_eq!(container_for("avc1.640028", "mp4a.40.2"), Container::Mp4); assert_eq!(container_for("avc1.640028", "opus"), Container::Mkv); assert_eq!(container_for("vp9", "mp4a.40.2"), Container::Mkv); assert_eq!(container_for("av01.0.08M.08", "opus"), Container::Mkv); }
#[test] fn video_info_from_synthetic() { let v = to_video_info(&info(SYNTH)).unwrap(); assert_eq!(v.title, "Synthetic Video"); assert_eq!(v.duration_secs, Some(754.0)); assert!(v.has_audio); assert_eq!(v.video_options.len(), 4); }
#[test] fn combined_only_has_nothing_usable() { assert!(matches!(to_video_info(&info(COMBINED)), Err(EngineError::NoStream(_)))); }
```

(`info(s)` is a test helper: `parse_info(s).unwrap()`.)

- [ ] **Step 2: Run to verify FAIL** — `cargo test -p engine select`
- [ ] **Step 3: Implement `select.rs`** to the interfaces and rules above.
- [ ] **Step 4: Run to verify PASS** — `cargo test -p engine`
- [ ] **Step 5: Commit** — `git commit -am "feat(engine): resolution list and stream selection"` (add the new file first)

---

### Task 4: File naming

**Files:**
- Create: `engine/src/naming.rs`; Modify: `engine/src/lib.rs`

**Interfaces:**
- Produces:
  - `pub fn sanitize_filename(title: &str) -> String` — removes `< > : " / \ | ? *` and control chars; trims leading/trailing spaces and trailing dots; appends `_` to Windows reserved names (`CON PRN AUX NUL COM1–COM9 LPT1–LPT9`, case-insensitive); truncates to 200 chars (on a char boundary); empty result → `"download"`
  - `pub fn unique_path(folder: &Path, stem: &str, ext: &str) -> PathBuf` — `stem.ext`, else `stem (1).ext`, `stem (2).ext`, …

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn strips_illegal() { assert_eq!(sanitize_filename(r#"a<b>c:d"e/f\g|h?i*j"#), "abcdefghij"); }
#[test] fn strips_controls() { assert_eq!(sanitize_filename("a\u{7}b"), "ab"); }
#[test] fn trims_dots_and_spaces() { assert_eq!(sanitize_filename("  Song. . "), "Song"); }
#[test] fn reserved_names() { assert_eq!(sanitize_filename("CON"), "CON_"); assert_eq!(sanitize_filename("lpt1"), "lpt1_"); }
#[test] fn empty_becomes_download() { assert_eq!(sanitize_filename("???"), "download"); }
#[test] fn keeps_unicode() { assert_eq!(sanitize_filename("🎵 Café Mix"), "🎵 Café Mix"); }
#[test] fn truncates_long() { assert!(sanitize_filename(&"é".repeat(300)).chars().count() <= 200); }
#[test] fn unique_paths() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(unique_path(d.path(), "Song", "mp4"), d.path().join("Song.mp4"));
    std::fs::write(d.path().join("Song.mp4"), b"").unwrap();
    assert_eq!(unique_path(d.path(), "Song", "mp4"), d.path().join("Song (1).mp4"));
    std::fs::write(d.path().join("Song (1).mp4"), b"").unwrap();
    assert_eq!(unique_path(d.path(), "Song", "mp4"), d.path().join("Song (2).mp4"));
}
```

- [ ] **Step 2: Run to verify FAIL** — `cargo test -p engine naming`
- [ ] **Step 3: Implement `naming.rs`**
- [ ] **Step 4: Run to verify PASS** — `cargo test -p engine`
- [ ] **Step 5: Commit** — `"feat(engine): safe file names and unique paths"`

---

### Task 5: Ranged HTTP downloader with progress and cancel

**Files:**
- Create: `engine/src/download.rs`; Modify: `engine/src/lib.rs`

**Interfaces:**
- Consumes: `EngineError` (Task 2)
- Produces:
  - `pub const CHUNK_SIZE: u64 = 10 * 1024 * 1024;`
  - `pub type ProgressFn<'a> = &'a (dyn Fn(u64, Option<u64>) + Send + Sync);`
  - `pub async fn download_ranged(client: &reqwest::Client, url: &str, headers: &HashMap<String, String>, chunk_size: u64, dest: &Path, cancel: &CancellationToken, progress: ProgressFn<'_>) -> Result<u64, EngineError>` — returns bytes written

Behavior: create/truncate `dest`; loop sending `Range: bytes={start}-{start+chunk_size-1}` with all `headers`. Learn the total from `Content-Range: bytes a-b/TOTAL` on a 206. If the server answers 200 (ignored range), stream the whole body and use `Content-Length`. Non-2xx → `Http("HTTP {status}")`. A chunk shorter than requested before reaching the total → `Http("connection ended early")`. Stream the body with `bytes_stream()`, and race each read against `cancel.cancelled()` with `tokio::select!` → `Err(Cancelled)`. Call `progress(done, total)` at most every 200 ms, plus once at the end. Do **not** delete `dest` here; cleanup belongs to the job (Task 7).

- [ ] **Step 1: Write failing tests** (tokio tests using wiremock; write a small `RangeResponder { body: Vec<u8>, truncate: bool, delay: Option<Duration> }` implementing `wiremock::Respond` that parses the `Range` header and returns 206 with `Content-Range`)

```rust
#[tokio::test] async fn downloads_in_ranges_and_reports_total() {
    // body = 25 bytes (0..25), chunk_size = 10
    // assert: returned 25, file bytes == body, server received 3 requests,
    //         last progress call == (25, Some(25)), every request carried header "User-Agent: UA"
}
#[tokio::test] async fn http_403_is_error() { /* server returns 403 → Err(EngineError::Http(m)) and m.contains("403") */ }
#[tokio::test] async fn short_body_is_error() { /* truncate: true → Err(EngineError::Http(_)) */ }
#[tokio::test] async fn server_ignoring_range_still_works() { /* responder returns 200 + full body → Ok(25), file == body */ }
#[tokio::test] async fn cancel_stops_download() {
    // delay 5s; cancel after 100 ms; assert Err(EngineError::Cancelled) and elapsed < 1 s
}
```

- [ ] **Step 2: Run to verify FAIL** — `cargo test -p engine download`
- [ ] **Step 3: Implement `download_ranged`**
- [ ] **Step 4: Run to verify PASS** — `cargo test -p engine`
- [ ] **Step 5: Commit** — `"feat(engine): ranged downloader with progress and cancel"`

---

### Task 6: External tools — process helper, yt-dlp probe, ffmpeg, tool check

**Files:**
- Create: `engine/src/tools.rs`; Modify: `engine/src/lib.rs`

**Interfaces:**
- Consumes: `classify_ytdlp_error`, `EngineError` (Task 2)
- Produces:
  - `pub fn command(program: &str) -> tokio::process::Command` — sets `CREATE_NO_WINDOW` under `#[cfg(windows)]`, `kill_on_drop(true)`, piped stdout/stderr
  - `pub async fn run_ytdlp_probe(url: &str) -> Result<String, EngineError>` — runs `yt-dlp -J --no-playlist <url>`; spawn `NotFound` → `ToolMissing("yt-dlp")`; non-zero exit → `classify_ytdlp_error(stderr)`; else stdout
  - `pub fn merge_args(video: &Path, audio: &Path, out: &Path) -> Vec<OsString>`
  - `pub fn mp3_args(audio: &Path, out: &Path) -> Vec<OsString>`
  - `pub async fn run_ffmpeg(args: Vec<OsString>, cancel: &CancellationToken) -> Result<(), EngineError>` — on cancel, kill the child, wait for it, return `Cancelled`; non-zero exit → `Ffmpeg(last stderr line)`; `NotFound` → `ToolMissing("ffmpeg")`
  - `pub async fn tool_available(program: &str, version_arg: &str) -> bool`
  - `pub async fn missing_tools() -> Vec<&'static str>` — checks `yt-dlp --version`, `ffmpeg -version`, `deno --version`; returns missing names in that order

- [ ] **Step 1: Write failing tests**

```rust
#[test] fn merge_args_exact() {
    let a = merge_args(Path::new("v.part"), Path::new("a.part"), Path::new("out.mkv"));
    let a: Vec<_> = a.iter().map(|s| s.to_str().unwrap()).collect();
    assert_eq!(a, ["-hide_banner","-loglevel","error","-n","-i","v.part","-i","a.part","-map","0:v:0","-map","1:a:0","-c","copy","out.mkv"]);
}
#[test] fn mp3_args_exact() {
    let a = mp3_args(Path::new("a.part"), Path::new("out.mp3"));
    let a: Vec<_> = a.iter().map(|s| s.to_str().unwrap()).collect();
    assert_eq!(a, ["-hide_banner","-loglevel","error","-n","-i","a.part","-vn","-c:a","libmp3lame","-q:a","0","out.mp3"]);
}
#[tokio::test] async fn unknown_tool_is_unavailable() { assert!(!tool_available("definitely-not-a-real-tool-xyz", "--version").await); }
#[tokio::test] #[ignore = "needs ffmpeg on PATH"] async fn ffmpeg_cancel_kills_process() {
    // args: -f lavfi -i anullsrc -t 600 -c:a libmp3lame <tempdir>/long.mp3 ; cancel after 300 ms
    // assert Err(Cancelled) within 2 s
}
```

- [ ] **Step 2: Run to verify FAIL** — `cargo test -p engine tools`
- [ ] **Step 3: Implement `tools.rs`**
- [ ] **Step 4: Run to verify PASS** — `cargo test -p engine` and `cargo test -p engine -- --ignored ffmpeg_cancel`
- [ ] **Step 5: Commit** — `"feat(engine): yt-dlp/ffmpeg process helpers and tool check"`

---

### Task 7: Backend trait, job runner, and the Engine API

This is the V2 seam (spec §7.2). Jobs talk to the outside world only through a `Backend` trait, so tests use a fake and the real one wraps Tasks 5–6.

**Files:**
- Create: `engine/src/backend.rs`, `engine/src/job.rs`, `engine/tests/network.rs`
- Modify: `engine/src/lib.rs` (add `Engine`)

**Interfaces:**
- Consumes: everything from Tasks 2–6
- Produces (`backend.rs`):
  ```rust
  #[async_trait::async_trait]
  pub trait Backend: Send + Sync + 'static {
      async fn probe_json(&self, url: &str) -> Result<String, EngineError>;
      async fn download(&self, fmt: &RawFormat, dest: &Path, cancel: &CancellationToken, progress: ProgressFn<'_>) -> Result<(), EngineError>;
      async fn ffmpeg(&self, args: Vec<OsString>, cancel: &CancellationToken) -> Result<(), EngineError>;
  }
  pub struct SystemBackend { client: reqwest::Client }   // impl Default; delegates to run_ytdlp_probe, download_ranged(CHUNK_SIZE), run_ffmpeg
  ```
- Produces (`job.rs`): `pub async fn run_job<B: Backend>(backend: &B, id: JobId, job: DownloadJob, cancel: CancellationToken, events: &UnboundedSender<JobEvent>)`
- Produces (`lib.rs`):
  ```rust
  pub struct Engine<B: Backend = SystemBackend> { /* Arc<B>, AtomicU64 next id, Arc<Mutex<HashMap<JobId, CancellationToken>>> */ }
  impl Engine<SystemBackend> { pub fn new() -> Self }
  impl<B: Backend> Engine<B> {
      pub fn with_backend(backend: B) -> Self;
      pub async fn probe(&self, url: &str) -> Result<VideoInfo, EngineError>;          // probe_json → parse_info → to_video_info
      pub fn start(&self, job: DownloadJob, events: UnboundedSender<JobEvent>) -> JobId; // tokio::spawn(run_job), must be called inside a tokio runtime
      pub fn cancel(&self, id: JobId);                                                  // no-op for unknown ids
  }
  pub use tools::missing_tools;
  ```
  The spawned task removes its token from the map when `run_job` returns.

`run_job` sequence:
1. Emit `Phase(Probing)`; `probe_json` → `parse_info` (fresh stream URLs; the earlier UI probe may be stale).
2. If `job.folder` is not an existing directory → `Folder(path)`.
3. **Video:** `select_video(height, fps_class(Some(option.fps as f64)))` and `select_audio` (missing → `NoStream("video")` / `NoStream("audio")`). Final path = `unique_path(folder, sanitize_filename(title), container_for(v.vcodec, a.acodec).ext())`. Part files = `<final stem>.video.part`, `<final stem>.audio.part` in the folder. Emit `Phase(DownloadingVideo)` + `Progress` events, `Phase(DownloadingAudio)` + `Progress`, `Phase(Merging)`, run `merge_args`, delete parts, emit `Done { path }`.
4. **Mp3:** `select_audio`; final `.mp3`; `Phase(DownloadingAudio)` + progress, `Phase(Converting)`, `mp3_args`, delete part, `Done`.
5. Any `Err(Cancelled)` → delete parts **and the final path if it was created**, emit `Cancelled`. Any other error → same cleanup, emit `Failed { msg: err.user_message() }`. Exactly one terminal event, always last.

- [ ] **Step 1: Write the fake backend and failing tests** (in `job.rs` `#[cfg(test)]`)

`FakeBackend { json: &'static str, block_in: Option<Stage>, fail_in: Option<Stage>, download_calls: AtomicUsize }` with `enum Stage { Download, Ffmpeg }`. `download` writes 100 bytes to `dest` and calls `progress(100, Some(100))`; `ffmpeg` writes a few bytes to its last arg (the output path). `block_in` writes its bytes first and then waits for `cancel.cancelled()` and returns `Err(Cancelled)`. `fail_in` returns `Err(Http("HTTP 403"))`. Helper `run(fake, kind, folder) -> Vec<JobEvent>` runs a job to completion and collects events; helper `phases(&events)` extracts `Phase` values; `files(dir)` lists file names.

```rust
#[tokio::test] async fn video_job_event_order() {
    // 1080p60 option from SYNTH (vp9 + opus → mkv)
    assert_eq!(phases(&ev), [Probing, DownloadingVideo, DownloadingAudio, Merging]);
    assert!(matches!(&ev.last().unwrap().kind, JobEventKind::Done { path } if path.ends_with("Synthetic Video.mkv")));
    assert_eq!(ev.iter().filter(|e| e.is_terminal()).count(), 1);
    assert!(ev.iter().all(|e| e.job_id == JobId(1)));
}
#[tokio::test] async fn mp3_job_event_order() { /* phases == [Probing, DownloadingAudio, Converting]; Done path ends with "Synthetic Video.mp3" */ }
#[tokio::test] async fn parts_removed_after_success() { /* files(dir) == ["Synthetic Video.mkv"] */ }
#[tokio::test] async fn existing_file_is_not_overwritten() { /* pre-create "Synthetic Video.mkv"; Done path ends with "Synthetic Video (1).mkv" */ }
#[tokio::test] async fn cancel_during_download_cleans_up() { /* block_in Download; cancel after 50 ms; last event Cancelled; files(dir) empty */ }
#[tokio::test] async fn cancel_during_merge_cleans_up() { /* block_in Ffmpeg; cancel; last event Cancelled; files(dir) empty */ }
#[tokio::test] async fn failure_reports_message_and_cleans_up() { /* fail_in Download; last event Failed { msg } with !msg.is_empty(); files(dir) empty */ }
#[tokio::test] async fn missing_folder_fails_before_download() {
    // folder = dir.join("gone"); last event Failed { msg } with msg.contains("Choose another folder"); download_calls == 0
}
#[tokio::test] async fn engine_start_returns_unique_ids_and_cancel_unknown_is_noop() {
    // Engine::with_backend(fake); two start() calls → different JobIds; engine.cancel(JobId(999)) does not panic
}
#[tokio::test] async fn engine_probe_returns_video_info() { /* Engine::with_backend(fake SYNTH).probe("u") → title "Synthetic Video", 4 options */ }
```

- [ ] **Step 2: Run to verify FAIL** — `cargo test -p engine job`
- [ ] **Step 3: Implement `backend.rs`, `job.rs`, and `Engine` in `lib.rs`**
- [ ] **Step 4: Run to verify PASS** — `cargo test -p engine`
- [ ] **Step 5: Add ignored network tests in `engine/tests/network.rs`**

Both read the URL from env var `YTDL_TEST_URL` and a temp folder:
- `real_probe_lists_options`: `Engine::new().probe(url)` → non-empty title and `video_options`.
- `real_mp3_download`: start an Mp3 job, collect events until terminal; assert `Done` and the `.mp3` file is larger than 0 bytes.

Run: `$env:YTDL_TEST_URL="<url>"; cargo test -p engine --test network -- --ignored`
Expected: PASS (needs network, yt-dlp, ffmpeg, Deno).

- [ ] **Step 6: Commit** — `"feat(engine): job runner, backend trait, Engine API"`

---

### Task 8: Tauri app crate — commands, single-job guard, settings

**Files:**
- Create (via template): `app/` (vanilla-ts), `app/src-tauri/`
- Create: `app/src-tauri/src/commands.rs`, `app/src-tauri/src/state.rs`, `app/src-tauri/src/settings.rs`
- Modify: root `Cargo.toml` (members add `"app/src-tauri"`), `app/src-tauri/Cargo.toml` (package name `app`, add `engine = { path = "../../engine" }`), `app/src-tauri/src/lib.rs`, `app/src-tauri/tauri.conf.json`

**Interfaces:**
- Consumes: `engine::{Engine, VideoInfo, DownloadJob, JobId, JobEvent, missing_tools}` (Task 7)
- Produces (`state.rs`): `pub struct ActiveJob(Mutex<Option<JobId>>)` with `pub fn begin(&self, start: impl FnOnce() -> JobId) -> Option<JobId>` (holds the lock while calling `start`; `None` if a job is active) and `pub fn finish(&self, id: JobId)` (clears only if `id` matches)
- Produces (`settings.rs`): `pub fn resolve_folder(saved: Option<String>, downloads: PathBuf) -> PathBuf` — saved if it is an existing directory, else `downloads`
- Produces (Tauri commands, names used by the frontend; errors are `String` from `user_message()`):
  - `missing_tools() -> Vec<String>`
  - `probe(url: String) -> Result<VideoInfo, String>`
  - `start_download(job: DownloadJob) -> Result<JobId, String>` — `Err("A download is already running.")` when busy
  - `cancel_download(job_id: JobId)`
  - `get_folder() -> String` — `resolve_folder(store["lastFolder"], app.path().download_dir())`
  - `pick_folder() -> Option<String>` — folder dialog (tauri-plugin-dialog, from Rust), saves `lastFolder` to store
  - `reveal(path: String) -> Result<(), String>` — tauri-plugin-opener `reveal_item_in_dir`
- Produces (event): `"job-event"` with a `JobEvent` payload

- [ ] **Step 1: Scaffold**

From the repo root: `npm create tauri-app@latest app` → choose TypeScript / npm / Vanilla / TypeScript, identifier `com.donny.ytdownloader`. Delete the template's nested `.gitignore` entries that conflict only if needed. In `app/src-tauri`: `cargo add tauri-plugin-store tauri-plugin-dialog` (opener is already in the template); register all three with `.plugin(...)` in `lib.rs`. All plugin use happens in Rust commands, so `capabilities/default.json` needs only `core:default` (keep `opener:default` from the template). In `tauri.conf.json`: window title `"YouTube Downloader"`, `width 420`, `height 600`, `resizable true`, `minWidth 380`, `minHeight 520`. Leave the template's `"csp": null` so the YouTube thumbnail can load.

Run: `cd app; npm install; npm run tauri dev` — Expected: template window opens at ~420×600. Close it.

- [ ] **Step 2: Write failing tests**

```rust
// state.rs
#[test] fn second_begin_is_refused() { let a = ActiveJob::default(); assert_eq!(a.begin(|| JobId(1)), Some(JobId(1))); assert_eq!(a.begin(|| JobId(2)), None); }
#[test] fn finish_frees_slot() { let a = ActiveJob::default(); a.begin(|| JobId(1)); a.finish(JobId(1)); assert_eq!(a.begin(|| JobId(2)), Some(JobId(2))); }
#[test] fn finish_other_id_keeps_slot() { let a = ActiveJob::default(); a.begin(|| JobId(1)); a.finish(JobId(9)); assert_eq!(a.begin(|| JobId(2)), None); }
// settings.rs
#[test] fn resolve_folder_uses_saved() { let d = tempfile::tempdir().unwrap(); assert_eq!(resolve_folder(Some(d.path().to_string_lossy().into()), "C:\\dl".into()), d.path()); }
#[test] fn resolve_folder_defaults_to_downloads() { assert_eq!(resolve_folder(None, "C:\\dl".into()), PathBuf::from("C:\\dl")); }
#[test] fn resolve_folder_falls_back_when_missing() { assert_eq!(resolve_folder(Some("Z:\\no\\such\\dir".into()), "C:\\dl".into()), PathBuf::from("C:\\dl")); }
```

(`cargo add --dev tempfile` in `app/src-tauri`.)

- [ ] **Step 3: Run to verify FAIL** — `cargo test -p app`
- [ ] **Step 4: Implement `state.rs`, `settings.rs`, `commands.rs`; wire in `lib.rs`**

Managed state: `Engine` and `ActiveJob`. In `start_download`: inside `begin`, create an unbounded channel and call `engine.start`; spawn a task that forwards each event with `app.emit("job-event", &ev)` and calls `finish(id)` after a terminal event.

- [ ] **Step 5: Run to verify PASS** — `cargo test -p app` and `cargo test --workspace`
- [ ] **Step 6: Commit** — `"feat(app): Tauri crate with engine commands, single-job guard, settings"`

---

### Task 9: Frontend state machine

The UI's logic lives in one pure reducer so it can be tested without a window.

**Files:**
- Create: `app/src/types.ts`, `app/src/state.ts`, `app/src/state.test.ts`
- Modify: `app/package.json` (`"test": "vitest run"`; `npm install -D vitest`)

**Interfaces:**
- Consumes: the JSON shapes pinned in Task 2 (`job_event_json_shape`, `download_kind_json_shape`)
- Produces (`types.ts`): `VideoOption`, `VideoInfo`, `DownloadKind = {type:'video', option: VideoOption} | {type:'mp3'}`, `DownloadJob`, `Phase = 'probing'|'downloadingVideo'|'downloadingAudio'|'merging'|'converting'`, `JobEventKind`, `JobEvent = { jobId: number, kind: JobEventKind }` — camelCase fields exactly as the Rust serde output
- Produces (`state.ts`):
  ```ts
  export type Status = 'empty'|'probing'|'ready'|'downloading'|'done'|'error'|'toolsMissing';
  export interface State { status: Status; url: string; info?: VideoInfo; kind: 'video'|'mp3'; optionIndex: number; folder: string;
    jobId?: number; phase?: Phase; percent: number|null; donePath?: string; error?: string; missingTools: string[] }
  export type Action =
    | {type:'toolsChecked', missing: string[]} | {type:'folderChanged', folder: string}
    | {type:'urlPasted', url: string} | {type:'probeOk', url: string, info: VideoInfo} | {type:'probeFailed', url: string, message: string}
    | {type:'kindChanged', kind: 'video'|'mp3'} | {type:'optionChanged', index: number}
    | {type:'started', jobId: number} | {type:'startFailed', message: string} | {type:'jobEvent', event: JobEvent};
  export const initialState: State;
  export function reduce(s: State, a: Action): State;
  export function canDownload(s: State): boolean;   // status 'ready' or 'done', info present, folder non-empty, no missing tools, and the chosen kind is available
  export function buildJob(s: State): DownloadJob;
  ```

- [ ] **Step 1: Write failing tests** (`state.test.ts`, with a `SYNTH_INFO` constant matching Task 3's synthetic output)

```ts
test('missing tools block downloads', () => { let s = reduce(initialState, {type:'toolsChecked', missing:['deno']}); s = reduce(s, {type:'probeOk', url:'', info:SYNTH_INFO}); expect(s.missingTools).toEqual(['deno']); expect(canDownload(s)).toBe(false); });
test('paste probes, result makes ready with highest option', () => { let s = reduce(initialState, {type:'urlPasted', url:'u1'}); expect(s.status).toBe('probing'); s = reduce(s, {type:'probeOk', url:'u1', info:SYNTH_INFO}); expect(s.status).toBe('ready'); expect(s.optionIndex).toBe(0); expect(s.kind).toBe('video'); });
test('stale_probe_result_is_ignored', () => { let s = reduce(initialState, {type:'urlPasted', url:'u1'}); s = reduce(s, {type:'urlPasted', url:'u2'}); s = reduce(s, {type:'probeOk', url:'u1', info:SYNTH_INFO}); expect(s.status).toBe('probing'); expect(s.url).toBe('u2'); });
test('probe failure shows message', () => { /* probeFailed for current url → status 'error', error === message */ });
test('no video options defaults to mp3', () => { /* probeOk with video_options [] and hasAudio true → kind 'mp3' */ });
test('paste_during_download_is_ignored', () => { /* ready → started(1) → urlPasted('u9') → status 'downloading', url unchanged */ });
test('events for other jobs are ignored', () => { /* started(1), jobEvent with jobId 2 → state unchanged */ });
test('progress sets percent', () => { /* progress done 40 total 100 → percent 40; total null → percent null */ });
test('phase resets percent', () => { /* phase event → phase set, percent 0 */ });
test('done, cancelled, failed', () => { /* done → 'done' + donePath; cancelled → 'ready'; failed → 'error' with msg */ });
test('buildJob', () => { /* ready with optionIndex 1 → {url, kind:{type:'video', option: SYNTH_INFO.videoOptions[1]}, folder}; mp3 → {type:'mp3'} */ });
test('cannot download while probing or downloading', () => { /* canDownload false for both */ });
```

- [ ] **Step 2: Run to verify FAIL** — `cd app; npm test` — Expected: FAIL (module not found)
- [ ] **Step 3: Implement `types.ts` and `state.ts`**
- [ ] **Step 4: Run to verify PASS** — `npm test`
- [ ] **Step 5: Commit** — `"feat(ui): pure state reducer with tests"`

---

### Task 10: UI view, styling, wiring, and manual checklist

**Files:**
- Create: `app/src/api.ts`, `app/src/view.ts`, `docs/manual-test-checklist.md`
- Modify: `app/index.html`, `app/src/main.ts`, `app/src/styles.css` (replace template content)

**Interfaces:**
- Consumes: Task 8 command names and `"job-event"`; Task 9 `reduce`, `canDownload`, `buildJob`
- Produces (`api.ts`): `missingTools()`, `probe(url)`, `startDownload(job)`, `cancelDownload(jobId)`, `getFolder()`, `pickFolder()`, `reveal(path)` — thin `invoke` wrappers (Tauri's `invoke` uses camelCase arg names: `{ url }`, `{ job }`, `{ jobId }`, `{ path }`); `onJobEvent(cb: (e: JobEvent) => void)` via `listen('job-event')`
- Produces (`view.ts`): `render(s: State, root: HTMLElement): void` — updates existing elements; no framework

- [ ] **Step 1: Build `index.html` and `styles.css`** to the spec §7.5 layout: URL input; thumbnail + title + duration (`mm:ss` or `h:mm:ss`); segmented Video | MP3 toggle; resolution `<select>` showing `label` and `~N MB` when `estSize` is known; folder button and path text; one accent-colored Download button; thin progress bar with phase text and percent (indeterminate when `percent` is null); Cancel button; "Open folder" in the done state; a plain-language error area; a missing-tools panel listing each missing tool with its install hint. Rounded corners; light/dark via `prefers-color-scheme`. Video toggle disabled when `videoOptions` is empty; MP3 disabled when `hasAudio` is false.

- [ ] **Step 2: Implement `api.ts`, `view.ts`, and `main.ts` wiring**

On load: `missingTools()` → `toolsChecked`; `getFolder()` → `folderChanged`. The URL input's `paste` event (and `change`) dispatches `urlPasted` then calls `probe` → `probeOk`/`probeFailed` carrying that same url. Download → `startDownload(buildJob(state))` → `started`/`startFailed`. Cancel → `cancelDownload(jobId)`. Every `job-event` → `jobEvent`. After every dispatch, `render`.

Phase text: Probing → "Getting video info", downloadingVideo → "Downloading video", downloadingAudio → "Downloading audio", merging → "Merging", converting → "Converting to MP3".

- [ ] **Step 3: Write `docs/manual-test-checklist.md`**

One checkbox each: empty state; paste valid URL → info appears without a button; choose each resolution; switch to MP3; change folder, restart app, folder remembered; Video download completes and plays in VLC; MP3 download completes and plays; same video twice → `(1)` file; Cancel during downloading video, downloading audio, merging, and converting → no `.part` or partial files left; paste during download is ignored; private/age-restricted URL → specific message; garbage URL → plain error; rename `deno.exe` off PATH (or test on a PATH without it) → missing-tools panel and Download disabled; Windows dark mode → app follows; resize the window → layout holds.

- [ ] **Step 4: Verify**

Run: `cd app; npm test; cd ..; cargo test --workspace`, then `cd app; npm run tauri dev` and walk the whole checklist with Donny.
Expected: all automated tests pass; every checklist item checked.

- [ ] **Step 5: Commit**

`git add .` then `git commit -m "feat(ui): main window, wiring, manual test checklist"`
