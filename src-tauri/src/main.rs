//! WLTP - Modern WinMTR for Windows/macOS
//!
//! A user-friendly network diagnostic tool that provides clear,
//! human-readable interpretations of traceroute results.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use wltp_lib::commands;
use wltp_lib::AppState;

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    // Create app state
    let state = std::sync::Arc::new(AppState::default());

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::resolve_host,
            commands::start_trace,
            commands::stop_trace,
            commands::get_session_hops,
            commands::interpret_hops,
            commands::export_json,
            commands::export_html,
            commands::save_file,
            commands::get_settings,
            commands::update_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
