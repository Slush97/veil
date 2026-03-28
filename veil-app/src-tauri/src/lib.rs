use std::sync::Arc;

use tauri::Manager;
use tokio::sync::RwLock;

mod commands;
mod network;
pub mod state;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter("veil=debug,tauri=info")
        .init();

    let shared_state = Arc::new(RwLock::new(state::AppState::default()));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(shared_state.clone())
        .invoke_handler(tauri::generate_handler![
            commands::get_version,
            commands::has_identity,
            commands::create_identity,
            commands::load_identity,
            commands::dev_login,
            commands::get_identity,
            commands::get_groups,
            commands::set_active_group,
            commands::create_group,
            commands::delete_group,
            commands::rename_group,
            commands::get_channels,
            commands::set_active_channel,
            commands::create_channel,
            commands::delete_channel,
            commands::get_messages,
            commands::send_message,
            commands::send_file,
            commands::send_file_bytes,
            commands::get_blob,
            commands::get_connection_info,
            commands::connect_relay,
            commands::start_network,
            // Voice
            commands::voice_join,
            commands::voice_leave,
            commands::voice_set_mute,
            commands::voice_set_deafen,
            commands::voice_sdp_answer,
            commands::voice_ice_candidate,
            commands::get_voice_encryption_key,
            commands::get_voice_state,
            // Hosted relay
            commands::start_hosted_relay,
            commands::stop_hosted_relay,
            commands::get_hosted_relay_status,
        ])
        .setup(|app| {
            #[cfg(debug_assertions)]
            {
                let window = app.get_webview_window("main").unwrap();
                window.open_devtools();
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Veil");
}
