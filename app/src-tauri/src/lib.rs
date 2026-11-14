//! Tauri glue: registers plugins, shared state, and the commands the frontend calls.

mod commands;
mod settings;
mod state;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use engine::Engine;
use state::ActiveJob;
use tauri::{AppHandle, Manager, RunEvent};

/// Longest we keep the app open after the window closes, waiting for a cancelled job to clean up.
const EXIT_CLEANUP_TIMEOUT: Duration = Duration::from_secs(10);

/// Set once we've started a cleanup exit, so the second exit request goes through.
static CLEANUP_EXIT: AtomicBool = AtomicBool::new(false);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .manage(Engine::new())
        .manage(ActiveJob::default())
        .invoke_handler(tauri::generate_handler![
            commands::missing_tools,
            commands::probe,
            commands::start_download,
            commands::cancel_download,
            commands::get_folder,
            commands::pick_folder,
            commands::reveal,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            // Closing the window mid-download: exiting straight away would skip the job's
            // cleanup (leaving .part files and possibly a running ffmpeg). Cancel it, wait for
            // the job to finish cleaning up, then exit.
            if let RunEvent::ExitRequested { api, .. } = event {
                let Some(id) = app.state::<ActiveJob>().current() else { return };
                if CLEANUP_EXIT.swap(true, Ordering::SeqCst) {
                    return; // second request (ours, or the timeout passed): let it exit
                }
                api.prevent_exit();
                app.state::<Engine>().cancel(id);
                let app = app.clone();
                tauri::async_runtime::spawn(async move { exit_after_cleanup(app).await });
            }
        });
}

/// Waits until the cancelled job has finished (its slot is freed after its last event, which
/// comes after cleanup), or the timeout passes, then exits.
async fn exit_after_cleanup(app: AppHandle) {
    let started = Instant::now();
    while app.state::<ActiveJob>().current().is_some() && started.elapsed() < EXIT_CLEANUP_TIMEOUT {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    app.exit(0);
}
