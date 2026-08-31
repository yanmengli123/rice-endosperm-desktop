mod commands;
mod config;
mod credentials;
mod database;
mod diagnostics;
mod error;
mod session;
mod state;
mod workflow;
mod yuxi;

use tauri::Manager;

use commands::{
    cancel_run, create_thread, delete_api_key, delete_thread, get_chat_model_preference,
    get_public_settings, get_thread_run_context, import_model_configuration, list_accounts,
    list_byok_credentials, list_chat_models, list_threads, load_messages, parse_chat_attachment,
    remove_account, remove_byok_credential, rename_thread, save_byok_credential, save_connection,
    save_connection_with_login, save_custom_model_credential, send_message,
    set_chat_model_preference, switch_account, sync_pending_runs, test_connection,
    upload_chat_attachment,
};
use state::AppState;
use workflow::{
    bridge_workflow_artifact_to_qa, cancel_workflow_agent, cancel_workflow_run,
    create_workflow_project, delete_workflow_model_settings, delete_workflow_project,
    get_workflow_engine_status, get_workflow_model_settings, list_workflow_agent_turns,
    list_workflow_artifacts, list_workflow_projects, list_workflow_runs, open_workflow_artifact,
    pick_workflow_directory, respond_workflow_approval, run_counts_pca_workflow,
    run_workflow_agent, save_workflow_model_settings,
};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    diagnostics::install_panic_hook();
    let result = tauri::Builder::default()
        // 单实例保护必须最先注册：双实例并发会竞争 Stronghold 快照
        // （后写者胜出可能丢 Key）并重复触发更新器。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let version = app.package_info().version.to_string();
            diagnostics::initialize(&app_data_dir, &version)
                .map_err(|error| Box::<dyn std::error::Error>::from(error.to_string()))?;
            let state = tauri::async_runtime::block_on(AppState::open(&app_data_dir, &version))
                .map_err(|error| Box::<dyn std::error::Error>::from(error.to_string()))?;
            app.manage(state);
            diagnostics::log("INFO", "startup_ready", "application state initialized");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_public_settings,
            save_connection,
            save_connection_with_login,
            test_connection,
            delete_api_key,
            create_thread,
            list_threads,
            load_messages,
            get_thread_run_context,
            sync_pending_runs,
            rename_thread,
            delete_thread,
            send_message,
            upload_chat_attachment,
            parse_chat_attachment,
            cancel_run,
            list_byok_credentials,
            save_byok_credential,
            save_custom_model_credential,
            import_model_configuration,
            remove_byok_credential,
            list_chat_models,
            get_chat_model_preference,
            set_chat_model_preference,
            list_accounts,
            switch_account,
            remove_account,
            pick_workflow_directory,
            create_workflow_project,
            list_workflow_projects,
            delete_workflow_project,
            list_workflow_runs,
            list_workflow_artifacts,
            list_workflow_agent_turns,
            get_workflow_engine_status,
            run_counts_pca_workflow,
            cancel_workflow_run,
            open_workflow_artifact,
            get_workflow_model_settings,
            save_workflow_model_settings,
            delete_workflow_model_settings,
            run_workflow_agent,
            respond_workflow_approval,
            cancel_workflow_agent,
            bridge_workflow_artifact_to_qa,
        ])
        .run(tauri::generate_context!());

    if let Err(error) = result {
        diagnostics::report_startup_error(&error.to_string());
    }
}
