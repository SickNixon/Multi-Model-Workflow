// src-tauri/src/main.rs
// Binary entry point. All logic lives in lib.rs.
// Prevents a console window from appearing on Windows in release mode.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    vibe_orchestrator_lib::run();
}
