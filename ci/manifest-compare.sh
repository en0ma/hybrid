#!/usr/bin/env bash
set -euo pipefail

PIN="${MANIFEST_COMMIT:-d04b7aa90b098ba64ebcdf398e9a24d89a9114d8}"
ROOT="${RUNNER_TEMP:-/tmp}/manifest-baseline"
rm -rf "$ROOT"

git clone --filter=blob:none https://github.com/Bonasa-Tech/manifest.git "$ROOT"
git -C "$ROOT" checkout --detach "$PIN"

ACTUAL="$(git -C "$ROOT" rev-parse HEAD)"
test "$ACTUAL" = "$PIN"
echo "Manifest baseline: $ACTUAL"

if [[ "${MANIFEST_COMPARE_ONLY:-0}" != "1" ]]; then
  # The checkout is short lived. Keep outputs in a persistent directory.
  MANIFEST_TARGET_DIR="${MANIFEST_TARGET_DIR:-$ROOT/target}"
  # Only wrap native rustc. Solana's SBF toolchain uses its own compiler.
  # A compiler cache survives Cargo fingerprint rebuilds after a fresh clone.
  if [[ "${MANIFEST_NATIVE_SCCACHE:-0}" == "1" ]]; then
    command -v sccache >/dev/null 2>&1 || { echo "sccache missing" >&2; exit 1; }
    CARGO_TARGET_DIR="$MANIFEST_TARGET_DIR" RUSTC_WRAPPER=sccache cargo test --manifest-path "$ROOT/Cargo.toml" -p manifest-dex --lib
    sccache --show-stats
  else
    CARGO_TARGET_DIR="$MANIFEST_TARGET_DIR" cargo test --manifest-path "$ROOT/Cargo.toml" -p manifest-dex --lib
  fi
  CARGO_TARGET_DIR="$MANIFEST_TARGET_DIR" cargo build-sbf --manifest-path "$ROOT/programs/manifest/Cargo.toml"
fi

if [[ "${MANIFEST_BASELINE_ONLY:-0}" == "1" ]]; then
  echo "Pinned Manifest tests and SBF build passed. The CU comparison runs in another required job."
elif [[ -f Cargo.toml ]]; then
  if [[ ! -f ci/compare-cu.sh ]]; then
    echo "ci/compare-cu.sh is required once Hybrid contains Rust program code."
    exit 1
  fi
  MANIFEST_REPO="$ROOT" MANIFEST_COMMIT="$PIN" bash ci/compare-cu.sh
else
  echo "Hybrid program not present yet; pinned Manifest correctness/build baseline passed."
fi
