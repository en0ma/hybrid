#!/usr/bin/env bash
set -euo pipefail

SURFPOOL_VERSION="${SURFPOOL_VERSION:-1.5.0}"
RPC_PORT="${SURFPOOL_RPC_PORT:-8899}"
CONTAINER="hybrid-surfpool-${GITHUB_RUN_ID:-local}-${GITHUB_RUN_ATTEMPT:-0}"

cleanup() {
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
}
trap cleanup EXIT

args=(start --ci --no-deploy)
if [[ -n "${SOLANA_RPC_URL:-}" ]]; then
  args+=(--rpc-url "$SOLANA_RPC_URL")
else
  args+=(--network mainnet)
fi

docker run -d --name "$CONTAINER" \
  -p "$RPC_PORT:8899" \
  -e NO_DNA=1 \
  "surfpool/surfpool:$SURFPOOL_VERSION" "${args[@]}" >/dev/null

for _ in {1..90}; do
  if curl -fsS "http://127.0.0.1:$RPC_PORT" \
      -H 'content-type: application/json' \
      -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' 2>/dev/null | grep -q '"ok"'; then
    break
  fi
  sleep 1
done

curl -fsS "http://127.0.0.1:$RPC_PORT" \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' | grep -q '"ok"'

MANIFEST_PROGRAM="MNFSTqtC93rEfYHB6hF82sKdZpUDFWkViLByLd1k1Ms"
ACCOUNT_JSON=""
for _ in {1..30}; do
  ACCOUNT_JSON="$(curl -sS "http://127.0.0.1:$RPC_PORT" \
    -H 'content-type: application/json' \
    --data-binary @- <<JSON || true
{"jsonrpc":"2.0","id":2,"method":"getAccountInfo","params":["$MANIFEST_PROGRAM",{"encoding":"base64"}]}
JSON
)"
  if ACCOUNT_JSON="$ACCOUNT_JSON" python3 - <<'PY'
import json, os, sys
try:
    d=json.loads(os.environ["ACCOUNT_JSON"])
    ok=d.get("result",{}).get("value") is not None
except Exception:
    ok=False
sys.exit(0 if ok else 1)
PY
  then
    break
  fi
  sleep 2
done

if ! ACCOUNT_JSON="$ACCOUNT_JSON" python3 - <<'PY'
import json, os, sys
d=json.loads(os.environ.get("ACCOUNT_JSON") or "{}")
if d.get("result",{}).get("value") is None:
    print("Mainnet account fetch failed:", json.dumps(d, indent=2))
    sys.exit(1)
PY
then
  docker logs "$CONTAINER" | tail -200 || true
  exit 1
fi

export HYBRID_FORK_RPC="http://127.0.0.1:$RPC_PORT"
if [[ -x ci/fork-test.sh ]]; then
  bash ci/fork-test.sh
else
  echo "Fork smoke test passed; Manifest program is readable through Surfpool."
fi
