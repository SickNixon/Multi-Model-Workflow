// src/App.tsx
// Root component. Responsible for:
//   1. Subscribing to Tauri events and piping them into the Zustand store
//   2. Polling panel state on mount
//   3. Rendering the top-level layout (panel grid + orchestrator control)

import { useEffect } from 'react';
import { listen } from '@tauri-apps/api/event';
import { useStore } from './store';
import {
  type PanelId,
  type PanelEventPayload,
  TAURI_EVENTS,
  ALL_PANEL_IDS,
} from './types';
import { ModelCard } from './components/ModelCard';
import { OrchestratorPanel } from './components/OrchestratorPanel';

// ── Tauri event wiring ────────────────────────────────────────────────────────

function useTauriEvents() {
  const store = useStore();

  useEffect(() => {
    // Set up listeners — returns cleanup functions
    const unlisten: Array<() => void> = [];

    (async () => {
      unlisten.push(
        await listen<PanelEventPayload>(TAURI_EVENTS.PANEL_OUTPUT, ({ payload }) => {
          store.onPanelOutput(payload.panel_id, payload.output ?? '');
        }),
        await listen<PanelEventPayload>(TAURI_EVENTS.PANEL_READY, ({ payload }) => {
          store.onPanelReady(payload.panel_id);
        }),
        await listen<PanelEventPayload>(TAURI_EVENTS.PANEL_GENERATING, ({ payload }) => {
          store.onPanelGenerating(payload.panel_id);
        }),
        await listen<PanelEventPayload>(TAURI_EVENTS.PANEL_ERROR, ({ payload }) => {
          store.onPanelError(payload.panel_id, payload.message ?? 'unknown error');
        }),
      );
    })();

    return () => {
      for (const fn of unlisten) fn();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []); // mount-once — store methods are stable (Zustand guarantees this)
}

// ── App root ──────────────────────────────────────────────────────────────────

export default function App() {
  const panels         = useStore(s => s.panels);
  const refreshPanels  = useStore(s => s.refreshPanels);
  const isRefreshing   = useStore(s => s.isRefreshing);

  // Wire Tauri events → store
  useTauriEvents();

  // Pull initial panel state from Rust
  useEffect(() => {
    void refreshPanels();
    // Refresh every 5s to catch window close events (user closing a panel manually)
    const interval = setInterval(() => void refreshPanels(), 5000);
    return () => clearInterval(interval);
  }, [refreshPanels]);

  return (
    <div style={layout.root}>
      {/* Scanline overlay — purely cosmetic */}
      <div style={layout.scanlines} aria-hidden="true" />

      {/* Panel grid — 2×2 AI model cards */}
      <div style={layout.grid}>
        {/* Header bar */}
        <div style={layout.gridHeader}>
          <span className="dim" style={{ fontSize: 10, letterSpacing: '0.1em' }}>
            AI PANELS
          </span>
          <button
            className="btn-ghost"
            style={{ fontSize: 9, padding: '2px 8px' }}
            onClick={() => void refreshPanels()}
            disabled={isRefreshing}
          >
            {isRefreshing ? 'SYNCING…' : 'REFRESH'}
          </button>
        </div>

        {/* Model cards */}
        {ALL_PANEL_IDS.map(id => (
          <ModelCard
            key={id}
            panelId={id}
            info={panels[id]}
          />
        ))}
      </div>

      {/* Orchestrator control panel */}
      <div style={layout.orchestrator}>
        <OrchestratorPanel />
      </div>
    </div>
  );
}

// ── Layout ────────────────────────────────────────────────────────────────────

const layout = {
  root: {
    display: 'grid',
    gridTemplateColumns: '1fr 380px',
    gridTemplateRows: '100vh',
    width: '100vw',
    height: '100vh',
    overflow: 'hidden',
    position: 'relative' as const,
  },

  scanlines: {
    position: 'absolute' as const,
    inset: 0,
    backgroundImage: `repeating-linear-gradient(
      0deg,
      transparent,
      transparent 2px,
      rgba(0, 0, 0, 0.04) 2px,
      rgba(0, 0, 0, 0.04) 4px
    )`,
    pointerEvents: 'none' as const,
    zIndex: 999,
  },

  grid: {
    display: 'grid',
    gridTemplateColumns: '1fr 1fr',
    gridTemplateRows: 'auto 1fr 1fr',
    gap: 12,
    padding: 16,
    alignContent: 'start',
    overflowY: 'auto' as const,
  },

  gridHeader: {
    gridColumn: '1 / -1',
    display: 'flex',
    alignItems: 'center',
    justifyContent: 'space-between',
    paddingBottom: 4,
    borderBottom: '1px solid var(--border)',
  },

  orchestrator: {
    height: '100vh',
    display: 'flex',
    flexDirection: 'column' as const,
    overflow: 'hidden',
  },
};
