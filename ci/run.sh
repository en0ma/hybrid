#!/usr/bin/env bash
set -euo pipefail

echo "== hybrid deterministic CI =="

CI_START_SECONDS="$SECONDS"
ci_stage() {
  local name="$1"
  shift
  local started="$SECONDS"
  echo "::group::CI stage: $name"
  local status=0
  "$@" || status=$?
  echo "::endgroup::"
  echo "CI_STAGE_SECONDS $name $((SECONDS - started)) status=$status"
  return "$status"
}
trap 'echo "CI_TOTAL_SECONDS $((SECONDS - CI_START_SECONDS))"' EXIT

if [[ ! -f Cargo.toml ]]; then
  echo "No Cargo.toml yet; Rust/SBF/fuzz/CU stages are not applicable during bootstrap."
  exit 0
fi

ci_stage format cargo fmt --all -- --check
ci_stage clippy cargo clippy --workspace --all-targets -- -D warnings
ci_stage native-tests cargo test --workspace

if [[ ! -f ci/state-check.sh || ! -f ci/state-budgets.json ]]; then
  echo "Persistent state byte budget files are required."
  exit 1
fi
ci_stage state-budgets bash ci/state-check.sh

command -v cargo-build-sbf >/dev/null 2>&1 || { echo "cargo-build-sbf missing"; exit 1; }
command -v cargo-test-sbf >/dev/null 2>&1 || { echo "cargo-test-sbf missing"; exit 1; }

ci_stage sbf-build cargo build-sbf --manifest-path program/Cargo.toml

if [[ ! -f ci/bytecode-check.sh || ! -f ci/bytecode-budgets.json ]]; then
  echo "Bytecode budget files are required once Hybrid contains Rust program code."
  exit 1
fi
ci_stage bytecode-budgets bash ci/bytecode-check.sh

ci_stage sbf-tests cargo test-sbf --manifest-path program/Cargo.toml --features test-sbf -- --nocapture --test-threads=1

if [[ ! -f fuzz/Cargo.toml ]]; then
  echo "fuzz/Cargo.toml is required once Hybrid contains Rust program code."
  exit 1
fi
FUZZ_RUST_TOOLCHAIN="${FUZZ_RUST_TOOLCHAIN:-nightly-2026-10-01}"
rustup toolchain install "$FUZZ_RUST_TOOLCHAIN" --profile minimal --no-self-update
if ! command -v cargo-fuzz >/dev/null 2>&1; then
  cargo install cargo-fuzz --locked
fi
mapfile -t targets < <(cd fuzz && cargo +"$FUZZ_RUST_TOOLCHAIN" fuzz list)
if [[ "${#targets[@]}" -eq 0 ]]; then
  echo "No fuzz targets found."
  exit 1
fi
for target in "${targets[@]}"; do
  (cd fuzz && cargo +"$FUZZ_RUST_TOOLCHAIN" fuzz run "$target" -- -max_total_time=30)
done

if [[ ! -f ci/cu-check.sh ]]; then
  echo "ci/cu-check.sh is required once Hybrid contains Rust program code."
  exit 1
fi
ci_stage cu-budget bash ci/cu-check.sh
