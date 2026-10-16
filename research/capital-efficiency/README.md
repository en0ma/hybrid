# P2 — Capital efficiency and advanced orders

**Research, with executable safety models; not deployed protocol functionality.**

## Working baseline

Hybrid currently has market-scoped collateral buckets in the 64-byte maker-balance account. Limit placement locks inventory and cancellation releases it; fills debit locked amounts. No program-verified cross-market shared collateral exists. See [security invariants](./threat-model.md).

Manifest's [public docs](https://github.com/Bonasa-Tech/manifest/blob/main/README.md) describe global orders (just-in-time shared inventory) and reverse orders (opposite-side replenishment using fill proceeds). These are comparison targets, not implemented Hybrid capabilities. See [baseline protocol](./manifest-baseline.md).

## Run the model

```bash
node --test research/capital-efficiency/model.test.mjs
node research/capital-efficiency/run.mjs research/capital-efficiency/two-market-scenario.json
```

The simulation intentionally demonstrates that advertising two fully funded markets against the same single pool can **overcommit**, and that a serialized conservative fill model must reject the second incompatible fill. It does not measure compute units or model Solana's actual account locks.

## Candidate designs

| Mechanism | Benefit to evaluate | Principal concern |
| --- | --- | --- |
| Per-market locked (current) | Explicit solvency | Fragmented inventory |
| Shared global reserve | Reuse deposited assets | Contention, order reliability, cross-market insolvency |
| Reverse order | Reduce requote traffic | Authorization, price/balance rounding, adverse inventory |
| Atomic cancel/replace | Less maker downtime | Account fan-out and compute |
| Passive settlement extension | More executed liquidity | Fees, redeemability, pricing, supply invariant |

## Measurement stages

1. **Model:** prove reserve and concurrency invariants under adversarial scenarios (this PR).
2. **Prototype:** implement a separate disposable SBF experiment with no production ABI change.
3. **Benchmark:** identical workload across pinned Hybrid/Manifest revisions; compare compute, transaction count, rent, metas and settlement correctness.
4. **Decision:** select a narrowly scoped order type only when it improves usable liquidity per unit of *actually protected* capital.

See [experiment template](./experiment-template.json), [decision record](./decision-record.md), and [threat model](./threat-model.md). **Do not insert estimated compute or throughput into the experiment result fields.**

## Paired benchmark result validation

When **actual executed** Hybrid and Manifest measurement records exist, populate the template and run:

```bash
node --test research/capital-efficiency/validate-results.test.mjs
node research/capital-efficiency/validate-results.mjs ./completed-experiment.json
```

The result validator requires five paired workloads, pinned 40-character commits, matching environment identity, receipt evidence, conservation checks, and measured metrics for each implementation. The empty template is intentionally rejected. **Schema validation does not independently authenticate signatures or prove measurement truthfulness**; collectors must perform RPC receipt and token-account verification.
\n## Collect on-chain receipt metrics\n\nUse [receipt collection](./receipts.md) to query real finalized transaction compute units, slots, account-meta usage and fees. This supplies only partial benchmark evidence and cannot replace independent settlement/accounting audits.\n