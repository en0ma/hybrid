#!/usr/bin/env bash
set -euo pipefail

TMP="$(mktemp)"
trap 'rm -f "$TMP"' EXIT

cargo test -p hybrid-state --lib tests::layouts_are_exact_and_small -- --exact --nocapture 2>&1 | tee "$TMP"
cargo test -p hybrid-settlement --lib tests::maker_balance_account_round_trips_and_has_exact_layout -- --exact --nocapture 2>&1 | tee -a "$TMP"

python3 - "$TMP" ci/state-budgets.json <<'PY'
import json, pathlib, re, sys
log = pathlib.Path(sys.argv[1]).read_text()
budgets = json.load(open(sys.argv[2]))
rows = {
    name: int(value)
    for name, value in re.findall(r"HYBRID_STATE_BYTES\s+(\w+)\s+(\d+)", log)
}
missing = set(budgets) - set(rows)
if missing:
    raise SystemExit(f"Missing state-byte measurements: {sorted(missing)}")
bad = []
for name, limit in budgets.items():
    actual = rows[name]
    print(f"STATE {name}: {actual} / budget {limit} bytes")
    if actual > limit:
        bad.append((name, actual, limit))
if bad:
    raise SystemExit(f"State byte budget exceeded: {bad}")
PY
