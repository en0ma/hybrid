#!/usr/bin/env bash
set -euo pipefail

: "${MANIFEST_REPO:?MANIFEST_REPO must point at pinned Manifest checkout}"

export SBF_OUT_DIR="${SBF_OUT_DIR:-$PWD/target/deploy}"
cargo build-sbf --manifest-path program/Cargo.toml

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

measure() {
  local label="$1"
  local test_name="$2"
  local out="$TMP/$label.log"

  cargo test-sbf --manifest-path program/Cargo.toml --features test-sbf --test cu "$test_name" -- --exact --nocapture 2>&1 | tee "$out" >&2

  python3 - "$out" "$label" <<'PY'
import pathlib, re, sys
log=pathlib.Path(sys.argv[1]).read_text()
label=sys.argv[2]
values=[int(x) for x in re.findall(r"consumed\s+(\d+)\s+of\s+\d+\s+compute units", log)]
if len(values) != 1:
    raise SystemExit(f"{label}: expected exactly one program CU measurement, found {values}")
print(f"{label} {values[0]}")
PY
}

{
  measure noop measure_noop_cu
  measure passive_quote measure_passive_quote_cu
} > "$TMP/measurements.txt"

python3 - "$TMP/measurements.txt" "$MANIFEST_REPO" <<'PY'
import json, pathlib, subprocess, sys
rows={}
for line in pathlib.Path(sys.argv[1]).read_text().splitlines():
    parts=line.split()
    if len(parts)==2 and parts[1].isdigit():
        rows[parts[0]]=int(parts[1])
if not rows:
    raise SystemExit("No Hybrid CU measurements found")
manifest=pathlib.Path(sys.argv[2])
sha=subprocess.check_output(["git","-C",str(manifest),"rev-parse","HEAD"], text=True).strip()
report={"manifest_commit":sha,"hybrid_cu":rows,"comparison_status":"bootstrap-program-only"}
pathlib.Path("target").mkdir(exist_ok=True)
pathlib.Path("target/cu-comparison.json").write_text(json.dumps(report, indent=2)+"\n")
print(json.dumps(report, indent=2))
PY
