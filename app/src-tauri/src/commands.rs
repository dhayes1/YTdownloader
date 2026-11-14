//! The functions the frontend calls with `invoke(...)`. Errors come back as plain-language
//! strings from `EngineError::user_message()`.
//!
//! Commands that can start the queue are `async` so they run on Tauri's tokio runtime
//! (`Queue::start` spawns tasks).

use engine::{DownloadKind, Engine, EntryId, GroupId, PlaylistInfo, Probed, Quality, Queue};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_store::StoreExt;
use tokio::sync::oneshot;

use crate::settings::{resolve_choice, resolve_folder, resolve_video_quality, CHOICE, LAST_FOLDER, STORE_FILE, VIDEO_QUALITY};

/// Event name the frontend listens on; the payload is an `engine::QueueEvent`.
pub const QUEUE_EVENT: &str = "queue-event";

#[tauri::command]
pub async fn missing_tools() -> Vec<String> {
    engine::missing_tools().await.into_iter().map(String::from).collect()
}

#[tauri::command]
pub async fn probe_link(engine: State<'_, Engine>, url: String) -> Result<Probed, String> {
    engine.probe(&url).await.map_err(|e| e.user_message())
}

#[tauri::command]
pub async fn add_video(queue: State<'_, Queue>, url: String, title: String) -> Result<EntryId, String> {
    Ok(queue.add_video(url, title))
}

#[tauri::command]
pub async fn add_playlist(queue: State<'_, Queue>, playlist: PlaylistInfo) -> Result<GroupId, String> {
    Ok(queue.add_playlist(&playlist))
}

#[tauri::command]
pub async fn remove_entry(queue: State<'_, Queue>, id: EntryId) -> Result<(), String> {
    queue.remove(id);
    Ok(())
}

#[tauri::command]
pub async fn remove_group(queue: State<'_, Queue>, id: GroupId) -> Result<(), String> {
    queue.remove_group(id);
    Ok(())
}

#[tauri::command]
pub async fn start_queue(queue: State<'_, Queue>) -> Result<(), String> {
    queue.start();
    Ok(())
}

#[tauri::command]
pub async fn stop_queue(queue: State<'_, Queue>) -> Result<(), String> {
    queue.stop();
    Ok(())
}

#[tauri::command]
pub async fn clear_finished(queue: State<'_, Queue>) -> Result<(), String> {
    queue.clear_finished();
    Ok(())
}

#[tauri::command]
pub async fn get_choice(app: AppHandle) -> DownloadKind {
    resolve_choice(app.store(STORE_FILE).ok().and_then(|s| s.get(CHOICE)))
}

#[tauri::command]
pub async fn get_video_quality(app: AppHandle) -> Quality {
    resolve_video_quality(app.store(STORE_FILE).ok().and_then(|s| s.get(VIDEO_QUALITY)))
}

#[tauri::command]
pub async fn set_choice(app: AppHandle, queue: State<'_, Queue>, choice: DownloadKind) -> Result<(), String> {
    if let Ok(store) = app.store(STORE_FILE) {
        if let Ok(value) = serde_json::to_value(&choice) {
            store.set(CHOICE, value);
            if let DownloadKind::Video { quality } = &choice {
                if let Ok(q) = serde_json::to_value(quality) {
                    store.set(VIDEO_QUALITY, q);
                }
            }
            let _ = store.save();
        }
    }
    queue.set_choice(choice);
    Ok(())
}

/// The saved download folder (or Downloads), without side effects.
pub fn current_folder(app: &AppHandle) -> std::path::PathBuf {
    let saved = app
        .store(STORE_FILE)
        .ok()
        .and_then(|s| s.get(LAST_FOLDER))
        .and_then(|v| v.as_str().map(String::from));
    let downloads = app.path().download_dir().unwrap_or_default();
    resolve_folder(saved, downloads)
}

#[tauri::command]
pub async fn get_folder(app: AppHandle, queue: State<'_, Queue>) -> Result<String, String> {
    let folder = current_folder(&app);
    queue.set_folder(folder.clone());
    Ok(folder.to_string_lossy().into_owned())
}

/// Opens a folder picker. Returns the chosen folder (and remembers it), or `None` if cancelled.
#[tauri::command]
pub async fn pick_folder(app: AppHandle, queue: State<'_, Queue>) -> Result<Option<String>, String> {
    let (tx, rx) = oneshot::channel();
    app.dialog().file().pick_folder(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(folder) = rx.await.ok().flatten().and_then(|p| p.into_path().ok()) else {
        return Ok(None);
    };
    let folder_str = folder.to_string_lossy().into_owned();

    if let Ok(store) = app.store(STORE_FILE) {
        store.set(LAST_FOLDER, folder_str.clone());
        let _ = store.save();
    }
    queue.set_folder(folder);
    Ok(Some(folder_str))
}

/// Shows the file in File Explorer.
#[tauri::command]
pub fn reveal(app: AppHandle, path: String) -> Result<(), String> {
    app.opener().reveal_item_in_dir(path).map_err(|e| e.to_string())
}
