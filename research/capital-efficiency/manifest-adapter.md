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

### Swap payer signature authorization

The opcode attribution path now interprets the Solana raw-message `header.numRequiredSignatures`, checks that it matches the receipt signature count, and requires the taker/payer account to be one of the transaction's signed static message keys. This establishes signer **membership as reported by finalized RPC**, not independent Ed25519 cryptographic validation or proof of fill causation.

### Taker-side slippage bounds

The correlated receipt checker now also computes taker-side integer SPL base gain and quote debit and compares them to the exact-out/exact-in Swap constraints: base gain must agree with the persistent maker ask reduction, quote spent cannot exceed the declared input amount, and exact-in minimum-base / exact-out base quantity checks continue to apply. These checks apply to the benchmark's isolated single-taker single-maker case only; complex multi-instruction/multi-vault or fee-bearing scenarios need an independent custody and token-flow reconstruction. No independent on-chain benchmark has been completed.

### Transaction isolation for token-flow attribution

The combined Manifest correlation requires one non-ComputeBudget top-level instruction. Bundled token transfers and other state-changing instructions are rejected because whole-transaction SPL before/after deltas cannot be attributed to a specific Swap within a mixed transaction. ComputeBudget instructions may precede the Swap. This is a conservative condition for the single-maker benchmark, not a general decoding or proof of custody conservation.

### Isolated Manifest vault/token flow gate

`manifest-vault-flow-audit.mjs` checks four **explicit token-account indices** (taker base/quote and market base/quote vaults) against the same transaction's pre/post SPL balances. It requires stable identities, expected token direction, and exact atom conservation between each vault and taker. This strict model is limited to isolated, fee-free four-account swaps and intentionally rejects fee/extra-account flows pending a richer auditor; it does **not** verify market seats, order causality or executable provenance. Its result remains `verified: false`.

### Correlated vault evidence required

The correlated Swap audit now requires explicit four-account indices (`vaultAccounts`) and checks both base and quote vault movements against the taker's debit/credit. A mismatched or missing vault fails the combined audit. Market-seat balances, trade causality, fees, and binary/program deployment attestation are still outstanding. The result remains `verified: false`.

### Claimed-seat isolation

The fee-free single-maker audit also compares every claimed seat across before/after snapshots, rejects new/deleted/duplicate seats and rejects balance mutations for unrelated traders. This restricts the benchmark to an otherwise quiescent market; it does not prove that the snapshots are immediately adjacent to the transaction, and does not support global orders or fee-bearing settlement.

### Swap account-index binding

The `manifest-account-binding.mjs` helper requires explicit SPL balance indices to match the **actual core Swap instruction accounts** at generated Manifest positions traderBase(3), traderQuote(4), baseVault(5), and quoteVault(6). This prevents substitution of unrelated accounts into the vault-flow audit. Integrating the checker into live execution still requires real raw finalized receipt fixtures; independent account identity and market-derived vault pubkeys remain outstanding.

### Mandatory Swap-to-vault account binding

The combined correlated-trade audit now invokes `bindManifestSwapTokenAccounts` before checking vault flows. Its four supplied token-account indices must correspond to the accounts at the generated Swap instruction positions. A substituted index fails the entire audit, even when fabricated SPL balances happen to conserve. This protects the read-only audit path; it does not substitute for a real funded Manifest execution or independent market-vault authority verification.

## Funded local fill execution orchestration

`manifest-local-fill.mjs` now composes the controlled local signer/sender, real before/after Manifest SDK market snapshots, a second independently retrieved **raw JSON finalized receipt**, and the full isolated Swap/order/vault/maker-seat correlation. It fails closed on wrong payer, network, market/mint identity, finality/receipt mismatch or accounting discrepancy, while preserving `verified: false`. The operator must supply a funded local Manifest market, taker keypair, executable SDK Swap instruction, real RPC connection and a market snapshot callback. CI tests safety gates only; they do not run Surfpool or represent real Manifest fills.

### Strict local execution evidence boundaries

Before/after SDK snapshots must retain the same source commit and deployed program ID and strictly bracket the finalized transaction slot. The independently fetched finalized raw receipt must agree with the sender's compute usage and fee. These checks reduce accidental evidence misattribution but are not bytecode provenance or complete benchmark proof.

### Fail-closed execution data sources

Local fill orchestration refuses mismatched sender RPC endpoints, unpinned before-state provenance, and independent receipts lacking integer compute or fee figures. The snapshot callback is operator-supplied and **not** automatically proven to use the same RPC; operators must bind its connection to the sender and retain the raw snapshot evidence.
