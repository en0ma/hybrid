#!/usr/bin/env bash
set -euo pipefail

echo "== hybrid deterministic CI =="

if [[ ! -f Cargo.toml ]]; then
  echo "No Cargo.toml yet; Rust/SBF/fuzz/CU stages are not applicable during bootstrap."
  exit 0
fi

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features

command -v cargo-build-sbf >/dev/null 2>&1 || { echo "cargo-build-sbf missing"; exit 1; }
command -v cargo-test-sbf >/dev/null 2>&1 || { echo "cargo-test-sbf missing"; exit 1; }

cargo build-sbf --workspace
cargo test-sbf --workspace

if [[ ! -f fuzz/Cargo.toml ]]; then
  echo "fuzz/Cargo.toml is required once Hybrid contains Rust program code."
  exit 1
fi
if ! command -v cargo-fuzz >/dev/null 2>&1; then
  cargo install cargo-fuzz --locked
fi
mapfile -t targets < <(cargo fuzz list --manifest-path fuzz/Cargo.toml)
if [[ "${#targets[@]}" -eq 0 ]]; then
  echo "No fuzz targets found."
  exit 1
fi
for target in "${targets[@]}"; do
  cargo fuzz run "$target" --manifest-path fuzz/Cargo.toml -- -max_total_time=30
done

if [[ ! -f ci/cu-check.sh ]]; then
  echo "ci/cu-check.sh is required once Hybrid contains Rust program code."
  exit 1
fi
bash ci/cu-check.sh
