// src-tauri/src/lib.rs

mod bridge;
mod bridge_server;
mod commands;
mod state;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(state::AppState::new())
        .setup(|app| {
            let app_handle = app.handle().clone();

            // Start the HTTP bridge server
            tauri::async_runtime::spawn(async move {
                bridge_server::start(app_handle).await;
            });

            // Auto-open all panels in the background on startup.
            // If the session cookie is still valid, they'll go straight to READY.
            // If not, user clicks VIEW to log in once.
            let app_handle2 = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Brief delay to let the bridge server bind its port first
                tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
                for panel_id in state::ALL_PANELS {
                    commands::open_panel_window(&app_handle2, panel_id);
                    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_panel_states,
            commands::get_bridge_port,
            commands::open_panel,
            commands::close_panel,
            commands::show_panel,
            commands::hide_panel,
            commands::send_to_panel,
            commands::reset_panel_output,
            commands::capture_panel_output,
            commands::bridge_event,
            commands::reset_claude_session,
            commands::open_in_browser,
            commands::open_panel_devtools,
        ])
        .run(tauri::generate_context!())
        .expect("error while running vibe-orchestrator");
}
