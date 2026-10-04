#!/usr/bin/env bash
set -euo pipefail

: "${MANIFEST_REPO:?MANIFEST_REPO must point at pinned Manifest checkout}"

# Hybrid CU is measured from actual SBF simulation.
HYBRID_LOG="$(mktemp)"
trap 'rm -f "$HYBRID_LOG"' EXIT
export SBF_OUT_DIR="${SBF_OUT_DIR:-$PWD/target/deploy}"
cargo test-sbf --manifest-path program/Cargo.toml --features test-sbf --test cu -- --nocapture | tee "$HYBRID_LOG"

python3 - "$HYBRID_LOG" "$MANIFEST_REPO" <<'PY'
import json, pathlib, re, subprocess, sys
hybrid_log=pathlib.Path(sys.argv[1]).read_text()
manifest=pathlib.Path(sys.argv[2])
hybrid={k:int(v) for k,v in re.findall(r"HYBRID_CU\s+(\S+)\s+(\d+)", hybrid_log)}
if not hybrid:
    raise SystemExit("No Hybrid CU measurements found")

# Manifest's public repo includes CU tests, while its large replay fixture is private.
# Record the exact pinned revision and Hybrid measurements now; scenario-by-scenario
# Manifest swaps are added as the Hybrid swap instruction lands.
sha=subprocess.check_output(["git","-C",str(manifest),"rev-parse","HEAD"], text=True).strip()
report={"manifest_commit":sha,"hybrid_cu":hybrid,"comparison_status":"bootstrap-program-only"}
pathlib.Path("target").mkdir(exist_ok=True)
pathlib.Path("target/cu-comparison.json").write_text(json.dumps(report, indent=2)+"\n")
print(json.dumps(report, indent=2))
PY
