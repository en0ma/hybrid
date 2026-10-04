#!/usr/bin/env bash
set -euo pipefail

echo "== hybrid deterministic CI =="

if [[ ! -f Cargo.toml ]]; then
  echo "No Cargo.toml yet; Rust/SBF stages are not applicable."
  exit 0
fi

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features

if command -v cargo-build-sbf >/dev/null 2>&1; then
  cargo build-sbf --workspace
fi

if command -v cargo-test-sbf >/dev/null 2>&1; then
  cargo test-sbf --workspace
fi

if [[ -f fuzz/Cargo.toml ]]; then
  if ! command -v cargo-fuzz >/dev/null 2>&1; then
    cargo install cargo-fuzz --locked
  fi
  mapfile -t targets < <(cargo fuzz list --manifest-path fuzz/Cargo.toml)
  for target in "${targets[@]}"; do
    cargo fuzz run "$target" --manifest-path fuzz/Cargo.toml -- -max_total_time=30
  done
fi

if [[ -x ci/cu-check.sh ]]; then
  ci/cu-check.sh
else
  echo "No ci/cu-check.sh yet; CU budget stage becomes mandatory when executable."
fi
