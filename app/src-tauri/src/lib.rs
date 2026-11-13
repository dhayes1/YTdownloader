//! Tauri glue: registers plugins, shared state, and the commands the frontend calls.

mod commands;
mod settings;
mod state;

use engine::Engine;
use state::ActiveJob;

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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
