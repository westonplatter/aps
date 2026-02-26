#!/bin/bash
set -euo pipefail

# Only run in remote (Claude Code on the web) environments
if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

# Install Rust toolchain with fmt and clippy components if not present
if ! command -v rustup &>/dev/null; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
  source "$HOME/.cargo/env"
fi

# Ensure rustfmt and clippy are installed
rustup component add rustfmt clippy 2>/dev/null || true

# Pre-build dependencies so cargo build/test/clippy are fast later
cd "$CLAUDE_PROJECT_DIR"
cargo build 2>/dev/null || true
