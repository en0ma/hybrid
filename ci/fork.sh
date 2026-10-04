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
fi

docker run -d --name "$CONTAINER"   -p "$RPC_PORT:8899"   -e NO_DNA=1   "surfpool/surfpool:$SURFPOOL_VERSION" "${args[@]}" >/dev/null

for _ in {1..90}; do
  if curl -fsS "http://127.0.0.1:$RPC_PORT"       -H 'content-type: application/json'       -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' | grep -q '"ok"'; then
    break
  fi
  sleep 1
done

curl -fsS "http://127.0.0.1:$RPC_PORT"   -H 'content-type: application/json'   -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' | grep -q '"ok"'

# Prove the fork can lazily read a real mainnet program: Manifest core.
MANIFEST_PROGRAM="MNFSTqtC93rEfYHB6hF82sKdZpUDFWkViLByLd1k1Ms"
curl -fsS "http://127.0.0.1:$RPC_PORT"   -H 'content-type: application/json'   -d "{"jsonrpc":"2.0","id":2,"method":"getAccountInfo","params":["$MANIFEST_PROGRAM",{"encoding":"base64"}]}"   | python3 -c 'import json,sys; d=json.load(sys.stdin); assert d["result"]["value"] is not None, "Manifest program was not fetched from mainnet fork"'

export HYBRID_FORK_RPC="http://127.0.0.1:$RPC_PORT"

if [[ -x ci/fork-test.sh ]]; then
  ci/fork-test.sh
elif [[ -f Cargo.toml ]] && grep -Rqs 'mainnet_fork' tests 2>/dev/null; then
  cargo test --test mainnet_fork -- --nocapture
else
  echo "Fork smoke test passed. Add ci/fork-test.sh (or tests/mainnet_fork.rs) for protocol scenarios."
fi
