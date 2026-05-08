// src/store.ts
import { create } from 'zustand';
import { invoke } from '@tauri-apps/api/core';
import {
  type PanelId, type PanelInfo, type RoutingConfig,
  type ConversationEntry, type CmdResult, ALL_PANEL_IDS,
} from './types';

interface OrchestratorStore {
  panels:        Record<PanelId, PanelInfo>;
  routing:       RoutingConfig;
  history:       ConversationEntry[];
  promptDraft:   string;
  bridgePort:    number;
  isRefreshing:  boolean;
  // Tracks sequential routing progress: which panel index we're waiting on
  sequenceIndex: number;

  refreshPanels:     () => Promise<void>;
  openPanel:         (id: PanelId) => Promise<void>;
  closePanel:        (id: PanelId) => Promise<void>;
  showPanel:         (id: PanelId) => Promise<void>;
  hidePanel:         (id: PanelId) => Promise<void>;
  sendPrompt:        (text: string) => Promise<void>;
  onPanelOutput:     (panelId: PanelId, output: string) => void;
  onPanelReady:      (panelId: PanelId) => void;
  onPanelGenerating: (panelId: PanelId) => void;
  onPanelError:      (panelId: PanelId, message: string) => void;
  setPromptDraft:    (text: string) => void;
  setRouting:        (config: Partial<RoutingConfig>) => void;
  clearHistory:      () => void;
}

const DEFAULT_ROUTING: RoutingConfig = { mode: 'broadcast', targets: ['gemini', 'deepseek'] };

function makeEntry(from: PanelId | 'user', to: PanelId | 'all', content: string): ConversationEntry {
  return {
    id: `${Date.now()}-${Math.random().toString(36).slice(2)}`,
    timestamp: Date.now(), from, to, content,
  };
}

async function sendToPanel(panelId: PanelId, message: string): Promise<void> {
  try {
    await invoke<CmdResult<null>>('send_to_panel', { panelId, message });
  } catch (err) {
    console.error(`[store] sendToPanel ${panelId}:`, err);
  }
}

export const useStore = create<OrchestratorStore>((set, get) => ({
  panels:        {} as Record<PanelId, PanelInfo>,
  routing:       DEFAULT_ROUTING,
  history:       [],
  promptDraft:   '',
  bridgePort:    7539,
  isRefreshing:  false,
  sequenceIndex: 0,

  refreshPanels: async () => {
    set({ isRefreshing: true });
    try {
      const result = await invoke<CmdResult<PanelInfo[]>>('get_panel_states');
      if (result.ok && result.data) {
        const map = {} as Record<PanelId, PanelInfo>;
        for (const p of result.data) map[p.id as PanelId] = p;
        set({ panels: map });
      }
      const portResult = await invoke<CmdResult<number>>('get_bridge_port');
      if (portResult.ok && portResult.data) set({ bridgePort: portResult.data });
    } catch (err) {
      console.error('[store] refreshPanels:', err);
    } finally {
      set({ isRefreshing: false });
    }
  },

  openPanel: async (id: PanelId) => {
    try { await invoke<CmdResult<null>>('open_panel', { panelId: id }); await get().refreshPanels(); }
    catch (err) { console.error('[store] openPanel:', err); }
  },

  closePanel: async (id: PanelId) => {
    try { await invoke<CmdResult<null>>('close_panel', { panelId: id }); await get().refreshPanels(); }
    catch (err) { console.error('[store] closePanel:', err); }
  },

  showPanel: async (id: PanelId) => {
    try { await invoke<CmdResult<null>>('show_panel', { panelId: id }); }
    catch (err) { console.error('[store] showPanel:', err); }
  },

  hidePanel: async (id: PanelId) => {
    try { await invoke<CmdResult<null>>('hide_panel', { panelId: id }); }
    catch (err) { console.error('[store] hidePanel:', err); }
  },

  sendPrompt: async (text: string) => {
    const { routing, history } = get();
    if (!text.trim()) return;

    const userEntry = makeEntry('user', routing.mode === 'single' ? routing.targets[0] : 'all', text);
    set({ history: [...history, userEntry], promptDraft: '' });

    if (routing.mode === 'broadcast') {
      // Send to all targets simultaneously
      for (const panelId of routing.targets) {
        await sendToPanel(panelId, text);
      }
    } else if (routing.mode === 'sequential') {
      // Send only to the FIRST target — onPanelOutput handles chaining
      set({ sequenceIndex: 0 });
      if (routing.targets.length > 0) {
        await sendToPanel(routing.targets[0], text);
      }
    } else if (routing.mode === 'single') {
      if (routing.targets.length > 0) {
        await sendToPanel(routing.targets[0], text);
      }
    }

    await get().refreshPanels();
  },

  onPanelOutput: (panelId: PanelId, output: string) => {
    const entry = makeEntry(panelId, 'user', output);
    set(state => ({
      history: [...state.history, entry],
      panels: {
        ...state.panels,
        [panelId]: { ...state.panels[panelId], status: { status: 'done' }, last_output: output },
      },
    }));

    // ── Sequential chaining ──────────────────────────────────────────────────
    // If we're in sequential mode, pipe this output to the next panel in the chain.
    const { routing, sequenceIndex } = get();
    if (routing.mode === 'sequential') {
      const currentIndex = routing.targets.indexOf(panelId);
      // Only chain if this output came from the panel we were waiting on
      if (currentIndex !== -1 && currentIndex === sequenceIndex) {
        const nextIndex = currentIndex + 1;
        if (nextIndex < routing.targets.length) {
          const nextPanel = routing.targets[nextIndex];
          set({ sequenceIndex: nextIndex });

          // Build the chained message — label who said what so the next model has context
          const chainedMessage =
            `[${panelId.toUpperCase()} said]:\n${output}`;

          // Small delay so the UI can update before we fire the next request
          setTimeout(() => {
            void sendToPanel(nextPanel, chainedMessage);
            // Log the routing in history
            const routeEntry = makeEntry(panelId, nextPanel, `→ routing to ${nextPanel.toUpperCase()}`);
            set(state => ({ history: [...state.history, routeEntry] }));
          }, 800);
        }
      }
    }
  },

  onPanelReady: (panelId: PanelId) => {
    set(state => ({
      panels: { ...state.panels, [panelId]: { ...state.panels[panelId], status: { status: 'idle' } } },
    }));
  },

  onPanelGenerating: (panelId: PanelId) => {
    set(state => ({
      panels: { ...state.panels, [panelId]: { ...state.panels[panelId], status: { status: 'generating' } } },
    }));
  },

  onPanelError: (panelId: PanelId, message: string) => {
    set(state => ({
      panels: { ...state.panels, [panelId]: { ...state.panels[panelId], status: { status: 'error', message } } },
    }));
  },

  setPromptDraft: (text: string) => set({ promptDraft: text }),
  setRouting: (config: Partial<RoutingConfig>) => set(state => ({ routing: { ...state.routing, ...config } })),
  clearHistory: () => set({ history: [] }),
}));

export const getStore = () => useStore.getState();
