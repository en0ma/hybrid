#!/usr/bin/env bash
set -euo pipefail

SOLANA_VERSION="${SOLANA_VERSION:-v4.3.0}"
BIN="$HOME/.local/share/solana/install/active_release/bin"

if [[ ! -x "$BIN/solana" ]] || [[ "$("$BIN/solana" --version 2>/dev/null || true)" != *"${SOLANA_VERSION#v}"* ]]; then
  sh -c "$(curl --proto '=https' --tlsv1.2 -sSfL "https://release.anza.xyz/$SOLANA_VERSION/install")"
fi

export PATH="$BIN:$PATH"
if [[ -n "${GITHUB_PATH:-}" ]]; then
  echo "$BIN" >> "$GITHUB_PATH"
fi

solana --version
cargo build-sbf --version
