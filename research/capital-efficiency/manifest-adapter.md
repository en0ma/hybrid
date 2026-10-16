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

## Controlled local signature submission

`manifest-local-send.mjs` can now sign, simulate, submit and confirm a real SDK-built instruction on a **loopback-only** RPC. It rejects remote endpoints, simulation failures, failed/missing receipts, unmetered transactions and mismatched receipt signatures. It requires a caller-provided Solana `Transaction` constructor, funded `Keypair` and `Connection`. It **does not** provision a Manifest market, run taker fills, or authenticate finalized settlement. Its confirmed receipt is not yet publishable benchmark evidence; pair it with the finalized receipt collector and independent custody-state checks.

## Finality and receipt gate

The local-only signed sender now waits for **finalized** confirmation after the initial confirmed inclusion, then queries an independently returned `getTransaction` receipt with `commitment: finalized`. It retries transient receipt indexing gaps and rejects finalized transaction errors, signature mismatches, unexpected program/market instructions or absent compute usage. This still does not authenticate token-account conservation or prove execution semantics; completed benchmark reports require separate before/after state audits.

## Receipt-to-orderbook fill audit

`manifest-fill-audit.mjs` provides a **read-only, fail-closed** cross-check between an executed Manifest transaction and two market snapshots. It demands an ask reduction for the maker and exactly conserved pre/post base and quote SPL balances on stable token-account indices. It rejects missing balances, changed owners, impossible token creation and unchanged maker orders.

This is **partial audit evidence only**: an ask reduction may also come from cancellation, and transfers may be unrelated. A production-quality comparison must also authenticate the exact trade instruction, taker identity, fee-adjusted settlement, custody vault transitions, seat/global balances, and before/after state observations from the same transaction boundary. Never treat this helper alone as a verified Manifest fill.

```sh
node --test research/capital-efficiency/manifest-fill-audit.test.mjs
```

### Strict partial-fill identity gate

The audit now explicitly rejects disappearances of entire resting orders, order sequence/price replacements, other-maker order mutations, and snapshots that do not bracket the receipt's slot. A completely consumed order still requires stronger opcode/event proof; until then, the helper only checks a **single persistent partially filled order**. Its output is `consistencyChecked: true, verified: false`, preventing reports from interpreting token conservation and book movement as conclusive trade authentication.

## Manifest core instruction attribution

The initial attribution helper `manifest-swap-attribution.mjs` examines raw compiled Solana instruction indices and a finalized receipt signature, matching the Manifest program ID and the market account position from upstream's generated `Swap.ts` layout. **It explicitly returns `verified: false`**, as verifying the actual swap discriminator requires decoding the full base58 instruction payload, followed by validating taker signature and decoded swap parameters.

Upstream source: `Bonasa-Tech/manifest/client/ts/src/manifest/instructions/Swap.ts`, whose generated instruction discriminator is 4, market account is second, payer/signer account is first. Do not use a matching program+market alone as proof that the trade executed.

## Correlated Swap and settlement consistency

Run `manifest-trade-correlation.mjs` on the **same finalized receipt** and before/after market snapshots to enforce both opcode attribution and conservative partial-fill/token conservation checks. The result intentionally retains `verified: false`: snapshots can include intervening transactions, and signer authorization, swap parameters, vault/seat accounting and trade price remain unaudited. The combined checker is **not** a real benchmark result.
