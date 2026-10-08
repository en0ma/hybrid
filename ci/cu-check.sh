#!/usr/bin/env bash
set -euo pipefail

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
expected = ["noop","passive_quote","hybrid_match","multilevel_match","state_backed_match","state_backed_plan","multipage_state_backed_match","place_ask","cancel_ask","place_bid","cancel_bid","swap_buy_exact_in","swap_buy_exact_out"]
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

python3 - "$TMP/measurements.txt" ci/cu-budgets.json <<'PY'
import json, pathlib, sys
rows={}
for line in pathlib.Path(sys.argv[1]).read_text().splitlines():
    parts=line.split()
    if len(parts)==2 and parts[0] in {"noop","passive_quote","hybrid_match","multilevel_match","state_backed_match","state_backed_plan","multipage_state_backed_match","place_ask","cancel_ask","place_bid","cancel_bid","swap_buy_exact_in","swap_buy_exact_out"} and parts[1].isdigit():
        rows[parts[0]]=int(parts[1])
budgets=json.load(open(sys.argv[2]))
missing=set(budgets)-set(rows)
if missing:
    raise SystemExit(f"Missing CU measurements: {sorted(missing)}")
bad=[]
for name, limit in budgets.items():
    units=rows[name]
    print(f"CU {name}: {units} / budget {limit}")
    if units > limit:
        bad.append((name, units, limit))
if bad:
    raise SystemExit(f"CU budget exceeded: {bad}")
PY
