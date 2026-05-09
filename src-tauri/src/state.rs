// src-tauri/src/state.rs
// Application state. Each AI panel is modelled as an explicit state machine —
// no ad-hoc boolean flags that get out of sync.
//
// State machine per panel:
//   Idle ──send──> Generating ──complete──> Done ──reset──> Idle
//                          └───error──> Error ──reset──> Idle

use std::collections::HashMap;
use std::sync::Mutex;
use serde::{Deserialize, Serialize};

// ── Panel identifiers ─────────────────────────────────────────────────────────

/// Canonical string IDs for each AI panel. These become window labels in Tauri.
pub const PANEL_GEMINI:   &str = "gemini";
pub const PANEL_DEEPSEEK: &str = "deepseek";
pub const PANEL_GROK:     &str = "grok";
pub const PANEL_CLAUDE:   &str = "claude";

/// All known panel IDs, in display order.
pub const ALL_PANELS: &[&str] = &[
    PANEL_GEMINI,
    PANEL_DEEPSEEK,
    PANEL_GROK,
    PANEL_CLAUDE,
];

// ── State machine ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum PanelStatus {
    /// Window not open. Default state.
    Closed,
    /// Window open, waiting for the page to finish loading.
    Loading,
    /// Page loaded, bridge injected. Ready to receive messages.
    Idle,
    /// Message sent, waiting for the AI to finish generating.
    Generating,
    /// Generation complete. Output available in last_output.
    Done,
    /// Something went wrong. Message contains what.
    Error { message: String },
}

impl PanelStatus {
    pub fn is_open(&self) -> bool {
        !matches!(self, PanelStatus::Closed)
    }
}

// ── Panel info ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PanelInfo {
    pub id:          String,
    pub label:       String,
    pub url:         String,
    pub status:      PanelStatus,
    /// Most recently captured output from this panel.
    pub last_output: Option<String>,
}

impl PanelInfo {
    fn new(id: &str, label: &str, url: &str) -> Self {
        Self {
            id:          id.to_string(),
            label:       label.to_string(),
            url:         url.to_string(),
            status:      PanelStatus::Closed,
            last_output: None,
        }
    }
}

// ── App state (shared across Tauri commands via State<T>) ─────────────────────

pub struct AppState {
    /// Inner mutex so multiple commands can hold State<AppState> safely.
    pub panels: Mutex<HashMap<String, PanelInfo>>,
    /// Port the HTTP bridge server is bound to (set once at startup).
    pub bridge_port: Mutex<u16>,
}

impl AppState {
    pub fn new() -> Self {
        let mut panels = HashMap::new();

        let known: &[(&str, &str, &str)] = &[
            (PANEL_GEMINI,   "Gemini",   "https://gemini.google.com"),
            (PANEL_DEEPSEEK, "DeepSeek", "https://chat.deepseek.com"),
            (PANEL_GROK,     "Grok",     "https://grok.com"),
            (PANEL_CLAUDE,   "Claude",   "https://claude.ai"),
        ];

        for (id, label, url) in known {
            panels.insert(id.to_string(), PanelInfo::new(id, label, url));
        }

        Self {
            panels:      Mutex::new(panels),
            bridge_port: Mutex::new(0),
        }
    }

    /// Transition a panel's status. Returns false if the panel id is unknown.
    pub fn set_status(&self, panel_id: &str, status: PanelStatus) -> bool {
        let mut panels = self.panels.lock().unwrap();
        if let Some(p) = panels.get_mut(panel_id) {
            p.status = status;
            true
        } else {
            false
        }
    }

    /// Store output for a panel and mark it Done.
    /// Returns false if the panel is unknown OR if the output is identical to
    /// what's already stored (deduplication — IPC + beacon can both deliver
    /// the same event; we only emit once).
    pub fn store_output(&self, panel_id: &str, output: String) -> bool {
        let mut panels = self.panels.lock().unwrap();
        if let Some(p) = panels.get_mut(panel_id) {
            // Deduplicate: same output string arriving twice (IPC + beacon race)
            if p.last_output.as_deref() == Some(output.as_str()) {
                return false;
            }
            p.last_output = Some(output);
            p.status = PanelStatus::Done;
            true
        } else {
            false
        }
    }

    pub fn get_bridge_port(&self) -> u16 {
        *self.bridge_port.lock().unwrap()
    }
}
