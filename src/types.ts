// src/types.ts
// Canonical type definitions for the orchestrator.
// These mirror the Rust structs in state.rs and commands.rs.
// If you change the Rust side, update here too.

// ── Panel identity ────────────────────────────────────────────────────────────

export type PanelId = 'gemini' | 'deepseek' | 'grok' | 'claude';

export const PANEL_LABELS: Record<PanelId, string> = {
  gemini:   'Gemini',
  deepseek: 'DeepSeek',
  grok:     'Grok',
  claude:   'Claude',
};

export const PANEL_COLORS: Record<PanelId, string> = {
  gemini:   '#4285F4', // Google blue
  deepseek: '#00C4CC', // DeepSeek teal
  grok:     '#E7E9EA', // X/Twitter white
  claude:   '#D97706', // Claude amber
};

export const ALL_PANEL_IDS: PanelId[] = ['gemini', 'deepseek', 'grok', 'claude'];

// ── Panel state machine ───────────────────────────────────────────────────────
// Must match PanelStatus enum in state.rs (serde tag = "status")

export type PanelStatus =
  | { status: 'closed' }
  | { status: 'loading' }
  | { status: 'idle' }
  | { status: 'generating' }
  | { status: 'done' }
  | { status: 'error'; message: string };

export type PanelStatusKind = PanelStatus['status'];

export interface PanelInfo {
  id:          PanelId;
  label:       string;
  url:         string;
  status:      PanelStatus;
  last_output: string | null;
}

// ── IPC command result wrapper ────────────────────────────────────────────────
// Must match CmdResult<T> in commands.rs

export interface CmdResult<T> {
  ok:    boolean;
  data:  T | null;
  error: string | null;
}

// ── Tauri event payloads (emitted by bridge_server.rs) ────────────────────────

export interface PanelEventPayload {
  panel_id: PanelId;
  output?:  string;
  message?: string;
}

export const TAURI_EVENTS = {
  PANEL_OUTPUT:     'panel:output',
  PANEL_READY:      'panel:ready',
  PANEL_ERROR:      'panel:error',
  PANEL_GENERATING: 'panel:generating',
} as const;

// ── Orchestrator routing ──────────────────────────────────────────────────────

export type RoutingMode =
  | 'broadcast'   // send to all active panels simultaneously
  | 'sequential'  // send to first, route its output to next, etc.
  | 'single'      // send to one specific panel
  | 'loop';       // cyclic chat: each model's output feeds the next, endlessly

export interface RoutingConfig {
  mode:    RoutingMode;
  targets: PanelId[];
}

// ── Conversation history entry ────────────────────────────────────────────────

export interface ConversationEntry {
  id:        string;
  timestamp: number;
  from:      PanelId | 'user';
  to:        PanelId | 'all';
  content:   string;
}
