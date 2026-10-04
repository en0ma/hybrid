#!/usr/bin/env bash
set -euo pipefail

export SBF_OUT_DIR="${SBF_OUT_DIR:-$PWD/target/deploy}"
cargo build-sbf --manifest-path program/Cargo.toml

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

measure() {
  local label="$1"
  local test_name="$2"
  local out="$TMP/$label.log"

  cargo test-sbf --manifest-path program/Cargo.toml --features test-sbf --test cu "$test_name" -- --exact --nocapture 2>&1 | tee "$out"

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
  measure hybrid_match measure_hybrid_match_cu
  measure multilevel_match measure_multilevel_match_cu
} > "$TMP/measurements.txt"

python3 - "$TMP/measurements.txt" ci/cu-budgets.json <<'PY'
import json, pathlib, sys
rows={}
for line in pathlib.Path(sys.argv[1]).read_text().splitlines():
    parts=line.split()
    if len(parts)==2 and parts[0] in {"noop","passive_quote","hybrid_match","multilevel_match"} and parts[1].isdigit():
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
