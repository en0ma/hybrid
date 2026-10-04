#!/usr/bin/env bash
set -euo pipefail

PIN="${MANIFEST_COMMIT:-d04b7aa90b098ba64ebcdf398e9a24d89a9114d8}"
ROOT="${RUNNER_TEMP:-/tmp}/manifest-baseline"
rm -rf "$ROOT"

git clone --filter=blob:none https://github.com/Bonasa-Tech/manifest.git "$ROOT"
git -C "$ROOT" checkout --detach "$PIN"

echo "Manifest baseline: $(git -C "$ROOT" rev-parse HEAD)"

# Publicly reproducible correctness baseline. Manifest's production replay CU
# harness is private, so Hybrid owns its own apples-to-apples CU scenarios.
cargo test --manifest-path "$ROOT/Cargo.toml" -p manifest-dex --lib

if command -v cargo-build-sbf >/dev/null 2>&1; then
  cargo build-sbf --manifest-path "$ROOT/programs/manifest/Cargo.toml"
fi

if [[ -x ci/compare-cu.sh ]]; then
  MANIFEST_REPO="$ROOT" ci/compare-cu.sh
else
  echo "Manifest correctness/build baseline passed."
  echo "CU comparison becomes mandatory when ci/compare-cu.sh is added with Hybrid's first executable program."
fi
