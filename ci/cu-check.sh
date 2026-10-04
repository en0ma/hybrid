#!/usr/bin/env bash
set -euo pipefail

OUT="$(mktemp)"
trap 'rm -f "$OUT"' EXIT

export SBF_OUT_DIR="${SBF_OUT_DIR:-$PWD/target/deploy}"
cargo build-sbf --manifest-path program/Cargo.toml
cargo test-sbf --manifest-path program/Cargo.toml --features test-sbf --test cu -- --nocapture 2>&1 | tee "$OUT"

python3 - "$OUT" ci/cu-budgets.json <<'PY'
import json, re, sys
log=open(sys.argv[1]).read()
budgets=json.load(open(sys.argv[2]))
seen={}
for name, units in re.findall(r"HYBRID_CU\s+(\S+)\s+(\d+)", log):
    seen[name]=int(units)
missing=set(budgets)-set(seen)
if missing:
    raise SystemExit(f"Missing CU measurements: {sorted(missing)}")
bad=[]
for name, limit in budgets.items():
    units=seen[name]
    print(f"CU {name}: {units} / budget {limit}")
    if units > limit:
        bad.append((name, units, limit))
if bad:
    raise SystemExit(f"CU budget exceeded: {bad}")
PY
