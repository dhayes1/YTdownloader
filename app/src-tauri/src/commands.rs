//! The functions the frontend calls with `invoke(...)`. Errors come back as plain-language
//! strings from `EngineError::user_message()`.

use engine::{DownloadJob, Engine, JobId, VideoInfo};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_store::StoreExt;
use tokio::sync::{mpsc, oneshot};

use crate::settings::{resolve_folder, LAST_FOLDER, STORE_FILE};
use crate::state::ActiveJob;

/// Event name the frontend listens on; the payload is an `engine::JobEvent`.
pub const JOB_EVENT: &str = "job-event";

#[tauri::command]
pub async fn missing_tools() -> Vec<String> {
    engine::missing_tools().await.into_iter().map(String::from).collect()
}

#[tauri::command]
pub async fn probe(engine: State<'_, Engine>, url: String) -> Result<VideoInfo, String> {
    engine.probe(&url).await.map_err(|e| e.user_message())
}

#[tauri::command]
pub async fn start_download(
    app: AppHandle,
    engine: State<'_, Engine>,
    active: State<'_, ActiveJob>,
    job: DownloadJob,
) -> Result<JobId, String> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let id = active
        .begin(|| engine.start(job, tx))
        .ok_or_else(|| "A download is already running.".to_string())?;

    // Forward the job's events to the window, then free the single-job slot.
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            let terminal = event.is_terminal();
            let _ = app.emit(JOB_EVENT, &event);
            if terminal {
                break;
            }
        }
        app.state::<ActiveJob>().finish(id);
    });
    Ok(id)
}

#[tauri::command]
pub fn cancel_download(engine: State<'_, Engine>, job_id: JobId) {
    engine.cancel(job_id);
}

#[tauri::command]
pub fn get_folder(app: AppHandle) -> String {
    let saved = app
        .store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(LAST_FOLDER))
        .and_then(|v| v.as_str().map(String::from));
    let downloads = app.path().download_dir().unwrap_or_default();
    resolve_folder(saved, downloads).to_string_lossy().into_owned()
}

/// Opens a folder picker. Returns the chosen folder (and remembers it), or `None` if cancelled.
#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Option<String> {
    let (tx, rx) = oneshot::channel();
    app.dialog().file().pick_folder(move |picked| {
        let _ = tx.send(picked);
    });
    let folder = rx.await.ok()??.into_path().ok()?;
    let folder = folder.to_string_lossy().into_owned();

    if let Ok(store) = app.store(STORE_FILE) {
        store.set(LAST_FOLDER, folder.clone());
        let _ = store.save();
    }
    Some(folder)
}

/// Shows the file in File Explorer.
#[tauri::command]
pub fn reveal(app: AppHandle, path: String) -> Result<(), String> {
    app.opener().reveal_item_in_dir(path).map_err(|e| e.to_string())
}
