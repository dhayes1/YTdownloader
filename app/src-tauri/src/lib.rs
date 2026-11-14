//! Tauri glue: registers plugins, shared state, and the commands the frontend calls.

mod commands;
mod keep_awake;
mod settings;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use engine::{Engine, Queue, QueueEvent, SystemBackend};
use keep_awake::{wants_awake, KeepAwake};
use tauri::{AppHandle, Emitter, Manager, RunEvent};
use tauri_plugin_store::StoreExt;

/// Longest we keep the app open after the window closes, waiting for the queue to stop and clean up.
const EXIT_CLEANUP_TIMEOUT: Duration = Duration::from_secs(10);

/// Set once we've started a cleanup exit, so the second exit request goes through.
static CLEANUP_EXIT: AtomicBool = AtomicBool::new(false);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .setup(|app| {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<QueueEvent>();
            let queue = Queue::new(Arc::new(SystemBackend::default()), tx);

            // The queue reads folder and choice as each video starts, so give it the saved ones now.
            let saved_choice = app.store(settings::STORE_FILE).ok().and_then(|s| s.get(settings::CHOICE));
            queue.set_choice(settings::resolve_choice(saved_choice));
            queue.set_folder(commands::current_folder(app.handle()));

            app.manage(queue);
            app.manage(Engine::new());
            app.manage(KeepAwake::new());

            // Forward queue events to the window and keep the PC awake while downloading.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Some(event) = rx.recv().await {
                    if let QueueEvent::Status { status } = &event {
                        handle.state::<KeepAwake>().set(wants_awake(*status));
                    }
                    let _ = handle.emit(commands::QUEUE_EVENT, &event);
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::missing_tools,
            commands::probe_link,
            commands::add_video,
            commands::add_playlist,
            commands::remove_entry,
            commands::remove_group,
            commands::start_queue,
            commands::stop_queue,
            commands::clear_finished,
            commands::get_choice,
            commands::set_choice,
            commands::get_folder,
            commands::pick_folder,
            commands::reveal,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Closing the window mid-download: exiting straight away would skip the job's
            // cleanup (leaving .part files and possibly a running ffmpeg). Stop the queue, wait
            // for it to finish cleaning up, then exit.
            if let RunEvent::ExitRequested { api, .. } = event {
                let queue = app.state::<Queue>();
                if !queue.is_running() {
                    app.state::<KeepAwake>().set(false);
                    return;
                }
                if CLEANUP_EXIT.swap(true, Ordering::SeqCst) {
                    return; // second request (ours, or the timeout passed): let it exit
                }
                api.prevent_exit();
                queue.stop();
                let app = app.clone();
                tauri::async_runtime::spawn(async move { exit_after_cleanup(app).await });
            }
        });
}

/// Waits until the stopped queue has finished (cleanup happens before it goes idle), or the
/// timeout passes, then releases keep-awake and exits.
async fn exit_after_cleanup(app: AppHandle) {
    let started = Instant::now();
    while app.state::<Queue>().is_running() && started.elapsed() < EXIT_CLEANUP_TIMEOUT {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    app.state::<KeepAwake>().set(false);
    app.exit(0);
}
