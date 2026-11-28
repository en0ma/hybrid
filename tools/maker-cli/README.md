# Hybrid maker CLI — P1 starting point

Initial commands are **read-only** wrappers around the existing reference manifest and readiness scripts. This avoids inventing a trading ABI or unattended key custody.

```bash
cd devnet-demo && npm install
cd ../tools/maker-cli && npm install
npm test
node cli.mjs manifest <MARKET> [MAKER]
node cli.mjs readiness <MARKET> <ASK_OWNER> <BID_OWNER> <BASE_VAULT> <QUOTE_VAULT>
```

The readiness command requires a correctly provisioned market and valid RPC configuration. It does not sign transactions.

## P1 completion criteria

Introduce a typed, versioned SDK with correct Q64 conversion, authenticated decoded book depth, maker free/locked balances, place/cancel/replace support, safe batch sequencing, simulation/confirmation handling, transaction retry journal, and executable end-to-end tests with the real program. Add examples and devnet evidence. Do not call this production-ready before those tests pass.

## M83 explicit operator interface

Default commands remain read-only. After installing dependencies in both directories, use `node cli.mjs observe config.json`, `plan config.json` or `inspect config.json`. The config must contain `rpcUrl`, `expectedGenesisHash`, `market`, `maker`, `askHeadOwner`, `bidHeadOwner`; `plan` additionally requires explicit `orders` and complete advisory `policy`. Price and quantity are decimal strings in atom units. No private keys belong in config. Advisory output never grants permission to submit.

For disposable isolated offline Surfpool only, configure an absolute `sessionPath` in a private directory, `isolatedTestWalletAcknowledged: true`, and explicit risk limits. Set `HYBRID_SANDBOX_EXECUTION=1 HYBRID_MAKER_KILL_SWITCH=off` for each single-step sandbox invocation. `sandbox-init` requires `acknowledgeEmptySession: true` after independently verifying no previous session/intent. Steps: `sandbox-reserve config.json`, externally sign the exact unsigned wire with the isolated maker, `sandbox-prepare config.json signed-wire.base64`, `sandbox-submit config.json signed-wire.base64`, `sandbox-finalize config.json`, `sandbox-reconcile config.json`. No signing, automatic retry, public broadcast or continuous loop. `sandbox-renew` is explicit and only permitted after the configured finalized quiet interval and successful reconciliation. Keep unsigned/signed wire and journals in a mode-0700 directory, never upload them to public logs. See [operations runbook](../../docs/milestones/83-maker-operations.md).

For a definitively finalized failed transaction only, `sandbox-recover-failed config.json` explicitly requests recovery. It requires the exact failed receipt and independently authenticated unchanged maker orders, inventory, sequence, custody and vaults. The failed receipt and fee are archived; no retry or counter reset is authorized. Unknown outcomes, expired-but-absent status and concurrent changes still require manual incident review. `inspect` checks that the configured domain matches the session.

Use `status config.json` for read-only pinned-genesis signature observation, including context slot, finalized block height and blockhash-expiry indication, even with execution disabled. An unknown or expired status does not authorize release or resubmission. The funded harness now runs the underlying CLI in independent child processes; signed wire is passed in private files rather than command-line arguments or CI logs.
