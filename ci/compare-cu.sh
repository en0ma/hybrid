#!/usr/bin/env bash
set -euo pipefail

: "${MANIFEST_REPO:?MANIFEST_REPO must point at pinned Manifest checkout}"

export SBF_OUT_DIR="${SBF_OUT_DIR:-$PWD/target/deploy}"
cargo build-sbf --manifest-path program/Cargo.toml

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

# Run the CU test binary once. Each test prints one structured measurement.
# Do not rely on generic "consumed" log lines from nested token CPIs.
LOG="$TMP/cu.log"
if ! cargo test-sbf --manifest-path program/Cargo.toml --features test-sbf --test cu -- --nocapture --test-threads=1 >"$LOG" 2>&1; then
  echo "CU suite failed" >&2
  tail -n 120 "$LOG" >&2
  exit 1
fi

python3 - "$LOG" "$TMP/measurements.txt" <<'PY'
import pathlib, re, sys
log = pathlib.Path(sys.argv[1]).read_text()
expected = ["noop","passive_quote","hybrid_match","multilevel_match","state_backed_match","state_backed_plan","multipage_state_backed_match"]
matches = re.findall(r"HYBRID_CU ([a-z_]+) ([0-9]+)", log)
rows = {}
for name, units in matches:
    if name in rows:
        raise SystemExit(f"Duplicate CU measurement: {name}")
    rows[name] = int(units)
missing = set(expected) - set(rows)
unexpected = set(rows) - set(expected)
if missing or unexpected:
    raise SystemExit(f"CU measurements mismatch: missing={sorted(missing)} unexpected={sorted(unexpected)}")
pathlib.Path(sys.argv[2]).write_text(
    "".join(f"{name} {rows[name]}\\n" for name in expected).replace("\\n", "\n")
)
PY

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
