#!/usr/bin/env bash
set -euo pipefail

BUDGETS="ci/bytecode-budgets.json"
DEPLOY_DIR="${SBF_OUT_DIR:-$PWD/target/deploy}"

python3 - "$BUDGETS" "$DEPLOY_DIR" <<'PY'
import json, pathlib, sys

budgets = json.load(open(sys.argv[1]))
deploy = pathlib.Path(sys.argv[2])
missing = []
bad = []

for filename, limit in budgets.items():
    path = deploy / filename
    if not path.exists():
        missing.append(filename)
        continue
    size = path.stat().st_size
    print(f"BYTECODE {filename}: {size} / budget {limit} bytes")
    if size > limit:
        bad.append((filename, size, limit))

if missing:
    raise SystemExit(f"Missing SBF bytecode artifacts: {missing}")
if bad:
    raise SystemExit(f"Bytecode budget exceeded: {bad}")
PY
