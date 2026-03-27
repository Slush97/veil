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
            commands::get_channels,
            commands::set_active_channel,
            commands::get_messages,
            commands::send_message,
            commands::get_connection_info,
            commands::connect_relay,
            commands::start_network,
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
