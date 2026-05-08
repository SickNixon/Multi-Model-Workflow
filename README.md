# Vibe Orchestrator

> A Tauri-based multi-model AI orchestration tool. Loads real AI chat interfaces (Gemini, DeepSeek, Grok, Claude) in native WebView windows, injects JavaScript bridges to read/write their inputs and outputs, and routes messages between them via a master control panel — zero API keys required.

```
┌─────────────────────────────────────────────────────────┐
│  Gemini WebView  │  DeepSeek WebView  │  Grok WebView   │
│  + JS bridge     │  + JS bridge       │  + JS bridge    │
└────────┬─────────┴──────────┬──────────┴───────┬────────┘
         │      Tauri IPC     │                  │
         └────────────────────┼──────────────────┘
                              │
                    ┌─────────▼──────────┐
                    │   Rust Core        │
                    │   (router + state) │
                    └─────────┬──────────┘
                              │
                    ┌─────────▼──────────────────┐
                    │  Orchestrator Panel         │
                    │  React/TypeScript UI        │
                    └────────────────────────────┘
                    ┌──────────────────────────────┐
                    │ Claude WebView (toggleable)   │
                    └──────────────────────────────┘
```

---

## How It Works

The app is just a very organised browser. Each AI panel loads the real website in a Tauri WebView. A JavaScript bridge is injected into each page that:
1. Writes text into the chat input
2. Submits the message
3. Watches for generation to complete (MutationObserver + debounce)
4. POSTs the output back to a local HTTP bridge server on `127.0.0.1:7539`

The Rust core receives that output, updates panel state, and emits Tauri events to the orchestrator UI.

---

## Stack

| Layer | Tech |
|-------|------|
| Desktop shell | Tauri 2.x |
| Backend logic | Rust |
| Frontend UI | React 18 + TypeScript |
| Bundler | Vite |
| State management | Zustand |
| Local bridge | axum (HTTP, loopback only) |

---

## Local Development

### Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Node.js](https://nodejs.org/) 18+
- Tauri system deps for your OS — see [Tauri Prerequisites](https://tauri.app/start/prerequisites/)

### Run

```bash
npm install
cargo tauri dev
```

### Build

```bash
cargo tauri build
```

---

## CI / Releases

GitHub Actions handles cross-platform builds.

- **`ci.yml`** — runs on every push/PR: `cargo check`, `clippy`, `tsc`
- **`release.yml`** — triggered by pushing a tag `v*.*.*`: builds macOS `.dmg`, Windows `.msi`, Linux `.AppImage` via `tauri-apps/tauri-action`

To trigger a release:

```bash
git tag v0.1.0
git push origin v0.1.0
```

---

## JS Bridge Selector Maintenance

The bridges in `src-tauri/src/bridge.rs` use DOM selectors to find each site's input/output elements. These **will break** when target sites update their UI — this is expected and normal.

When a selector breaks, you'll see this in the WebView's DevTools console:

```
[OrchestratorBridge:gemini] input selector failed — check INPUT_SELECTORS
```

Fix: open DevTools on the site, find the correct selector, update `bridge.rs`. It's a 5-minute job.

Each site's selectors are marked `// VERIFY:` in the source.

---

## Architecture Notes

- Bridge server binds to `127.0.0.1` only — never exposed externally
- All panel state is modelled as an explicit state machine in `state.rs` (no ad-hoc booleans)
- Each WebView window is a separate OS window, not an iframe
- Tauri's `initialization_script` runs the bridge JS before the page's own scripts on every navigation

---

## Project Structure

```
vibe-orchestrator/
├── src/                          # React/TypeScript frontend
│   ├── App.tsx                   # Root — Tauri event wiring + layout
│   ├── store.ts                  # Zustand state store
│   ├── types.ts                  # Shared TypeScript types
│   └── components/
│       ├── ModelCard.tsx         # Per-panel toggle card
│       └── OrchestratorPanel.tsx # Master control surface
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs                # Tauri app setup
│   │   ├── main.rs               # Binary entry point
│   │   ├── state.rs              # Panel state machines
│   │   ├── bridge.rs             # Site-specific JS bridges
│   │   ├── bridge_server.rs      # Local HTTP bridge server
│   │   └── commands.rs           # Tauri IPC commands
│   ├── capabilities/
│   │   └── default.json          # Tauri 2 permissions
│   └── tauri.conf.json
├── .github/workflows/
│   ├── ci.yml
│   └── release.yml
└── README.md
```
