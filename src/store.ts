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
  sequenceIndex: number;

  refreshPanels:      () => Promise<void>;
  openPanel:          (id: PanelId) => Promise<void>;
  closePanel:         (id: PanelId) => Promise<void>;
  showPanel:          (id: PanelId) => Promise<void>;
  hidePanel:          (id: PanelId) => Promise<void>;
  capturePanel:       (id: PanelId) => Promise<void>;
  sendPrompt:         (text: string) => Promise<void>;
  onPanelOutput:      (panelId: PanelId, output: string) => void;
  onPanelReady:       (panelId: PanelId) => void;
  onPanelGenerating:  (panelId: PanelId) => void;
  onPanelError:       (panelId: PanelId, message: string) => void;
  loopActive:         boolean;
  stopLoop:           () => void;
  setPromptDraft:     (text: string) => void;
  appendPromptDraft:  (text: string) => void;
  setRouting:         (config: Partial<RoutingConfig>) => void;
  clearHistory:       () => void;
}

const DEFAULT_ROUTING: RoutingConfig = { mode: 'broadcast', targets: ['gemini', 'deepseek'] };

function makeEntry(from: PanelId | 'user', to: PanelId | 'all', content: string): ConversationEntry {
  return { id: `${Date.now()}-${Math.random().toString(36).slice(2)}`, timestamp: Date.now(), from, to, content };
}

async function sendToPanel(panelId: PanelId, message: string): Promise<void> {
  try { await invoke<CmdResult<null>>('send_to_panel', { panelId, message }); }
  catch (err) { console.error(`[store] sendToPanel ${panelId}:`, err); }
}

export const useStore = create<OrchestratorStore>((set, get) => ({
  panels:        {} as Record<PanelId, PanelInfo>,
  routing:       DEFAULT_ROUTING,
  history:       [],
  promptDraft:   '',
  bridgePort:    7539,
  isRefreshing:  false,
  sequenceIndex: 0,
  loopActive:    false,

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
    } catch (err) { console.error('[store] refreshPanels:', err); }
    finally { set({ isRefreshing: false }); }
  },

  openPanel:   async (id) => { try { await invoke('open_panel',  { panelId: id }); await get().refreshPanels(); } catch(e) { console.error(e); } },
  closePanel:  async (id) => { try { await invoke('close_panel', { panelId: id }); await get().refreshPanels(); } catch(e) { console.error(e); } },
  showPanel:   async (id) => { try { await invoke('show_panel',  { panelId: id }); } catch(e) { console.error(e); } },
  hidePanel:   async (id) => { try { await invoke('hide_panel',  { panelId: id }); } catch(e) { console.error(e); } },
  capturePanel:async (id) => { try { await invoke('capture_panel_output', { panelId: id }); } catch(e) { console.error(e); } },

  sendPrompt: async (text: string) => {
    const { routing, history } = get();
    if (!text.trim()) return;
    const userEntry = makeEntry('user', routing.mode === 'single' ? routing.targets[0] : 'all', text);
    set({ history: [...history, userEntry], promptDraft: '' });

    if (routing.mode === 'broadcast') {
      for (const p of routing.targets) await sendToPanel(p, text);
    } else if (routing.mode === 'sequential') {
      set({ sequenceIndex: 0 });
      if (routing.targets.length > 0) await sendToPanel(routing.targets[0], text);
    } else if (routing.mode === 'loop') {
      // Start the loop: activate it, send to first target only
      set({ loopActive: true });
      if (routing.targets.length > 0) await sendToPanel(routing.targets[0], text);
    } else {
      if (routing.targets.length > 0) await sendToPanel(routing.targets[0], text);
    }
    await get().refreshPanels();
  },

  onPanelOutput: (panelId, output) => {
    const entry = makeEntry(panelId, 'user', output);
    set(state => ({
      history: [...state.history, entry],
      panels: { ...state.panels, [panelId]: { ...state.panels[panelId], status: { status: 'done' }, last_output: output } },
    }));
    const { routing, sequenceIndex, loopActive } = get();

    if (routing.mode === 'sequential') {
      const idx = routing.targets.indexOf(panelId);
      if (idx !== -1 && idx === sequenceIndex && idx + 1 < routing.targets.length) {
        const next = routing.targets[idx + 1];
        set({ sequenceIndex: idx + 1 });
        setTimeout(() => {
          void sendToPanel(next, `[${panelId.toUpperCase()} said]:\n${output}`);
          set(s => ({ history: [...s.history, makeEntry(panelId, next, `→ routing to ${next.toUpperCase()}`)] }));
        }, 800);
      }
    }

    if (routing.mode === 'loop' && loopActive && routing.targets.length > 1) {
      const idx = routing.targets.indexOf(panelId);
      if (idx !== -1) {
        const nextIdx = (idx + 1) % routing.targets.length;
        const next = routing.targets[nextIdx];
        // Brief pause so the user can see the response before the next fires
        setTimeout(() => {
          if (!get().loopActive) return; // user stopped the loop
          const msg = `[${panelId.toUpperCase()} said]:\n${output.slice(0, 2000)}`;
          void sendToPanel(next, msg);
          set(s => ({ history: [...s.history, makeEntry(panelId, next, `→ loop → ${next.toUpperCase()}`)] }));
        }, 1500);
      }
    }
  },

  onPanelReady:      (id) => set(s => ({ panels: { ...s.panels, [id]: { ...s.panels[id], status: { status: 'idle' } } } })),
  onPanelGenerating: (id) => set(s => ({ panels: { ...s.panels, [id]: { ...s.panels[id], status: { status: 'generating' } } } })),
  onPanelError:      (id, message) => set(s => ({ panels: { ...s.panels, [id]: { ...s.panels[id], status: { status: 'error', message } } } })),

  stopLoop:          ()      => set({ loopActive: false }),
  setPromptDraft:    (text)  => set({ promptDraft: text }),
  appendPromptDraft: (text)  => set(s => ({ promptDraft: s.promptDraft + (s.promptDraft && !s.promptDraft.endsWith(' ') ? ' ' : '') + text })),
  setRouting:        (cfg)   => set(s => ({ routing: { ...s.routing, ...cfg } })),
  clearHistory:      ()      => set({ history: [] }),
}));

export const getStore = () => useStore.getState();
