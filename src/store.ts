// src/store.ts
// Global client-side state via Zustand.
// Handles: panel state, routing config, conversation history, UI state.
//
// Tauri events (from bridge_server.rs) are wired to this store in App.tsx.

import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import {
  type PanelId,
  type PanelInfo,
  type RoutingConfig,
  type ConversationEntry,
  type CmdResult,
  ALL_PANEL_IDS,
} from './types';

// ── Store shape ───────────────────────────────────────────────────────────────

interface OrchestratorStore {
  // Panel state (mirrors Rust AppState)
  panels: Record<PanelId, PanelInfo>;

  // Routing configuration
  routing: RoutingConfig;

  // Message log (local to this session — not persisted yet)
  history: ConversationEntry[];

  // UI state
  promptDraft:  string;
  bridgePort:   number;
  isRefreshing: boolean;

  // ── Actions ──

  /** Pull fresh panel states from Rust. */
  refreshPanels: () => Promise<void>;

  /** Open (or focus) an AI panel window. */
  openPanel: (id: PanelId) => Promise<void>;

  /** Close a panel window. */
  closePanel: (id: PanelId) => Promise<void>;

  /** Send a message to panels according to current routing config. */
  sendPrompt: (text: string) => Promise<void>;

  /** Called by Tauri event listener when a panel reports output. */
  onPanelOutput: (panelId: PanelId, output: string) => void;

  /** Called by Tauri event listener when a panel is ready. */
  onPanelReady: (panelId: PanelId) => void;

  /** Called by Tauri event listener when a panel reports generating. */
  onPanelGenerating: (panelId: PanelId) => void;

  /** Called by Tauri event listener when a panel reports an error. */
  onPanelError: (panelId: PanelId, message: string) => void;

  setPromptDraft: (text: string) => void;
  setRouting: (config: Partial<RoutingConfig>) => void;
  clearHistory: () => void;
}

// ── Default routing ───────────────────────────────────────────────────────────

const DEFAULT_ROUTING: RoutingConfig = {
  mode:    'broadcast',
  targets: ['gemini', 'deepseek'],
};

// ── Helpers ───────────────────────────────────────────────────────────────────

function makeEntry(
  from: PanelId | 'user',
  to: PanelId | 'all',
  content: string,
): ConversationEntry {
  return {
    id:        `${Date.now()}-${Math.random().toString(36).slice(2)}`,
    timestamp: Date.now(),
    from,
    to,
    content,
  };
}

// ── Store ─────────────────────────────────────────────────────────────────────

export const useStore = create<OrchestratorStore>((set, get) => ({
  // Initial panel state — will be populated on first refreshPanels()
  panels: {} as Record<PanelId, PanelInfo>,

  routing:      DEFAULT_ROUTING,
  history:      [],
  promptDraft:  '',
  bridgePort:   7539,
  isRefreshing: false,

  // ── refreshPanels ──────────────────────────────────────────────────────────

  refreshPanels: async () => {
    set({ isRefreshing: true });
    try {
      const result = await invoke<CmdResult<PanelInfo[]>>('get_panel_states');
      if (result.ok && result.data) {
        const map = {} as Record<PanelId, PanelInfo>;
        for (const p of result.data) {
          map[p.id as PanelId] = p;
        }
        set({ panels: map });
      }

      const portResult = await invoke<CmdResult<number>>('get_bridge_port');
      if (portResult.ok && portResult.data) {
        set({ bridgePort: portResult.data });
      }
    } catch (err) {
      console.error('[store] refreshPanels failed:', err);
    } finally {
      set({ isRefreshing: false });
    }
  },

  // ── openPanel ─────────────────────────────────────────────────────────────

  openPanel: async (id: PanelId) => {
    try {
      const result = await invoke<CmdResult<null>>('open_panel', { panelId: id });
      if (!result.ok) {
        console.error('[store] openPanel failed:', result.error);
      }
      // Refresh to get updated status
      await get().refreshPanels();
    } catch (err) {
      console.error('[store] openPanel threw:', err);
    }
  },

  // ── closePanel ────────────────────────────────────────────────────────────

  closePanel: async (id: PanelId) => {
    try {
      await invoke<CmdResult<null>>('close_panel', { panelId: id });
      await get().refreshPanels();
    } catch (err) {
      console.error('[store] closePanel threw:', err);
    }
  },

  // ── sendPrompt ────────────────────────────────────────────────────────────

  sendPrompt: async (text: string) => {
    const { routing, history } = get();
    if (!text.trim()) return;

    // Log the user's message
    const userEntry = makeEntry('user', routing.mode === 'single' ? routing.targets[0] : 'all', text);
    set({ history: [...history, userEntry], promptDraft: '' });

    // Send to target panels
    const targets = routing.mode === 'single'
      ? routing.targets.slice(0, 1)
      : routing.targets;

    for (const panelId of targets) {
      try {
        const result = await invoke<CmdResult<null>>('send_to_panel', {
          panelId,
          message: text,
        });
        if (!result.ok) {
          console.error(`[store] sendPrompt to ${panelId} failed:`, result.error);
        }
      } catch (err) {
        console.error(`[store] sendPrompt to ${panelId} threw:`, err);
      }
    }

    await get().refreshPanels();
  },

  // ── Event handlers (called by Tauri event listeners in App.tsx) ───────────

  onPanelOutput: (panelId: PanelId, output: string) => {
    const entry = makeEntry(panelId, 'user', output);
    set(state => ({
      history: [...state.history, entry],
      panels: {
        ...state.panels,
        [panelId]: {
          ...state.panels[panelId],
          status:      { status: 'done' },
          last_output: output,
        },
      },
    }));
  },

  onPanelReady: (panelId: PanelId) => {
    set(state => ({
      panels: {
        ...state.panels,
        [panelId]: {
          ...state.panels[panelId],
          status: { status: 'idle' },
        },
      },
    }));
  },

  onPanelGenerating: (panelId: PanelId) => {
    set(state => ({
      panels: {
        ...state.panels,
        [panelId]: {
          ...state.panels[panelId],
          status: { status: 'generating' },
        },
      },
    }));
  },

  onPanelError: (panelId: PanelId, message: string) => {
    set(state => ({
      panels: {
        ...state.panels,
        [panelId]: {
          ...state.panels[panelId],
          status: { status: 'error', message },
        },
      },
    }));
  },

  // ── Simple setters ─────────────────────────────────────────────────────────

  setPromptDraft: (text: string) => set({ promptDraft: text }),

  setRouting: (config: Partial<RoutingConfig>) =>
    set(state => ({ routing: { ...state.routing, ...config } })),

  clearHistory: () => set({ history: [] }),
}));

// Expose a subset for use without the hook (in event listeners etc.)
export const getStore = () => useStore.getState();
