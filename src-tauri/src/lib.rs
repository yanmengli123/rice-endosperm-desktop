mod commands;
mod config;
mod credentials;
mod database;
mod error;
mod state;
mod yuxi;

use tauri::Manager;

use commands::{
    cancel_run, create_thread, delete_api_key, delete_thread, get_public_settings, list_threads,
    load_messages, rename_thread, save_connection, send_message, test_connection,
};
use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let version = app.package_info().version.to_string();
            let state = tauri::async_runtime::block_on(AppState::open(&app_data_dir, &version))
                .map_err(|error| Box::<dyn std::error::Error>::from(error.to_string()))?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_public_settings,
            save_connection,
            test_connection,
            delete_api_key,
            create_thread,
            list_threads,
            load_messages,
            rename_thread,
            delete_thread,
            send_message,
            cancel_run,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
