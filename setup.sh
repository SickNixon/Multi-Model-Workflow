#!/usr/bin/env bash
# setup.sh
# One-shot dev environment setup for vibe-orchestrator on macOS.
# Run this once from the project root: chmod +x setup.sh && ./setup.sh

set -e

BOLD='\033[1m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

info()    { echo -e "${BOLD}[setup]${NC} $1"; }
success() { echo -e "${GREEN}[ok]${NC}    $1"; }
warn()    { echo -e "${YELLOW}[warn]${NC}  $1"; }
die()     { echo -e "${RED}[fail]${NC}  $1"; exit 1; }

echo ""
echo "  ██╗   ██╗██╗██████╗ ███████╗"
echo "  ██║   ██║██║██╔══██╗██╔════╝"
echo "  ██║   ██║██║██████╔╝█████╗  "
echo "  ╚██╗ ██╔╝██║██╔══██╗██╔══╝  "
echo "   ╚████╔╝ ██║██████╔╝███████╗"
echo "    ╚═══╝  ╚═╝╚═════╝ ╚══════╝"
echo "  ORCHESTRATOR — Setup"
echo ""

# ── Homebrew ──────────────────────────────────────────────────
if ! command -v brew &>/dev/null; then
    die "Homebrew not found. Install it first: https://brew.sh"
fi
success "Homebrew: $(brew --version | head -1)"

# ── Rust ──────────────────────────────────────────────────────
if ! command -v rustc &>/dev/null; then
    info "Installing Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
else
    success "Rust: $(rustc --version)"
fi

# Ensure we're on stable and up to date
rustup update stable --no-self-update 2>/dev/null || true
rustup default stable

# ── Node.js ───────────────────────────────────────────────────
if ! command -v node &>/dev/null; then
    info "Installing Node.js via Homebrew..."
    brew install node
else
    NODE_VER=$(node --version | sed 's/v//' | cut -d. -f1)
    if [ "$NODE_VER" -lt 18 ]; then
        warn "Node $(node --version) is old. Upgrading to LTS..."
        brew upgrade node || brew install node
    else
        success "Node: $(node --version)"
    fi
fi
success "npm: $(npm --version)"

# ── macOS system deps for Tauri ────────────────────────────────
info "Checking macOS Tauri system dependencies..."
# Xcode Command Line Tools
if ! xcode-select -p &>/dev/null; then
    info "Installing Xcode Command Line Tools (you may be prompted)..."
    xcode-select --install
    warn "Run setup.sh again after Xcode tools finish installing."
    exit 0
fi
success "Xcode CLI tools: present"

# ── Tauri CLI ──────────────────────────────────────────────────
if ! cargo tauri --version &>/dev/null 2>&1; then
    info "Installing Tauri CLI 2.x..."
    cargo install tauri-cli --version "^2" --locked
else
    TAURI_VER=$(cargo tauri --version 2>/dev/null || echo "unknown")
    success "Tauri CLI: $TAURI_VER"
fi

# ── npm deps ───────────────────────────────────────────────────
info "Installing npm dependencies..."
npm install
success "npm deps installed"

# ── Rust deps (pre-fetch) ──────────────────────────────────────
info "Fetching Rust dependencies (first time may take a while)..."
cd src-tauri && cargo fetch && cd ..
success "Rust deps fetched"

echo ""
echo -e "${GREEN}${BOLD}✓ Setup complete!${NC}"
echo ""
echo "  To start dev server:    cargo tauri dev"
echo "  To build release:       cargo tauri build"
echo ""
echo "  Bridge runs on:         http://127.0.0.1:7539"
echo ""
