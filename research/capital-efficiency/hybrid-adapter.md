# Hybrid audited-lifecycle evidence adapter

This adapter reads output from the repository's **actual** funded Surfpool lifecycle implementation and its independent receipt-verification step. It does not create an untested alternative Solana signing path. It accepts **only** `one-maker-one-level`; other scenarios explicitly fail until implemented and verified.

```sh
cd devnet-demo
npm ci
npm run run:surfpool
```

The above requires the local Surfpool binary/runtime and the repo's SBF deployment prerequisites. The operator must verify the environment is isolated and that the generated evidence corresponds to the intended pinned Hybrid commit.

Environment configuration for the adapter:

```sh
export HYBRID_BENCHMARK_COMMIT=<40-character-actual-Hybrid-commit>
export HYBRID_LIFECYCLE_EVIDENCE=<path-to-fixture/lifecycle-evidence.json>
export HYBRID_VERIFIED_RECEIPTS=<path-to-fixture/verified-receipts.json>
```

The adapter exports `identity()` and `execute()` for use by `run-workload.mjs`. Existing six-stage receipts comprise maker initialization, deposit, place, trade, cancel and withdrawal. Not every benchmark step is a chain transaction: specifically **audit** is a state validation, not a new transaction. The runner now supports read-only audit steps with explicit proof records. The adapter still supports only the single-maker lifecycle, not the complete five-scenario benchmark; do not emit synthetic signatures or treat six lifecycle stages as the five comparison scenarios.

Next changes: revise runner to model non-transaction state audits explicitly, complete legitimate workload step mapping, then implement a real Manifest execution adapter on a pinned release.
