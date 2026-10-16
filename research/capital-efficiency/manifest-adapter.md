# Manifest read-only market preflight

This is the first **real Manifest-specific** integration surface in the P2 research workstream. It follows the upstream official SDK's `Market.loadFromBuffer`, `baseMint()`, `quoteMint()`, `bids()` and `asks()` APIs. The SDK decodes the market account; the preflight separately checks the account owner, expected token mints, commit provenance and integer atom values.

**Important:** it does not submit Manifest instructions, claim seats, deposit, place global orders, or represent a completed benchmark adapter.

## Setup

Use the published Manifest SDK version compatible with the chosen pinned Manifest Git revision. Upstream repo: https://github.com/Bonasa-Tech/manifest . SDK: https://www.npmjs.com/package/@bonasa-tech/manifest-sdk

From the repository root, with Node 22+:

```bash
cd research/capital-efficiency
npm install --no-save @bonasa-tech/manifest-sdk@0.2.47 @solana/web3.js@1.99.0
node manifest-market-cli.mjs manifest-market-config.json manifest-market-snapshot.json
```

Input example:

```json
{
  "rpc": "https://api.devnet.solana.com",
  "address": "REPLACE_WITH_MANIFEST_MARKET",
  "expectedProgramId": "REPLACE_WITH_DEPLOYED_MANIFEST_PROGRAM",
  "expectedBaseMint": "REPLACE_WITH_BASE_MINT",
  "expectedQuoteMint": "REPLACE_WITH_QUOTE_MINT",
  "commit": "REPLACE_WITH_FULL_40_CHARACTER_SOURCE_COMMIT"
}
```

The RPC network, program deployment and market account must refer to the same environment. The `commit` field identifies the reviewed **source**, but the tool cannot authenticate that the deployed bytecode corresponds to that commit. Operators must separately pin the SDK package/integrity hash and verify deployment authority and program artifact.

Snapshot outputs are **observations** and must not be passed to the workload runner as executed signatures. The actual Manifest execution adapter must use the official wrapper/core transaction ABI, create equivalent funded local-market and global-order setups, and independently validate finalized Solana receipts.

## Manifest SDK maker-side transaction construction

The initial maker-side instruction builder calls **real upstream Manifest SDK** methods: `ManifestClient.depositIx`, `placeOrderIx` with `OrderType.Limit`, and `cancelOrderIx`. It returns unsigned instructions for a separately controlled test harness and never marks them as executed. See `manifest-maker-instructions.mjs`.

Manifest's wrapper-facing SDK uses floating-point token quantities. The builder accepts integer atoms and **rejects** amounts outside its conservative precision guard. A genuine benchmark must use identical atom-accurate amounts and price tick semantics in both protocols; where wrapper float conversion is insufficient, use Manifest's integer core ABI instead.

It currently prepares only a single maker ask. No taker swap, receipt confirmation, market provisioning or independent token-audit logic is provided by this module. It must not be treated as the Manifest `run-workload` adapter yet.

```bash
node --test research/capital-efficiency/manifest-maker-instructions.test.mjs
```
