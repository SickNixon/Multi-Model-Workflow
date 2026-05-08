// src-tauri/src/lib.rs
// Main Tauri application setup.
// Wires together: state, commands, bridge server, plugins.
//
// STATE NOTE:
// AppState is managed by Tauri (via .manage()) and retrieved in both commands
// (via State<AppState>) and the bridge server (via app_handle.state::<AppState>()).
// This ensures a single shared instance — no double-init bug.

mod bridge;
mod bridge_server;
mod commands;
mod state;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        // Single AppState instance, accessible everywhere via app_handle.state::<AppState>()
        .manage(state::AppState::new())
        .setup(|app| {
            let app_handle = app.handle().clone();

            // Spawn the HTTP bridge server on Tauri's tokio runtime.
            // Uses app_handle to retrieve AppState — same instance as commands.
            tauri::async_runtime::spawn(async move {
                bridge_server::start(app_handle).await;
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
            commands::bridge_event,
        ])
        .run(tauri::generate_context!())
        .expect("error while running vibe-orchestrator");
}
