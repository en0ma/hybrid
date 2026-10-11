# Milestone PR #53: Market-maker operational integration

> Status: implementation workstream opened. This document defines engineering scope and acceptance, **not evidence that the scope is implemented**. PR remains draft until code, tests and exact-head gates meet every criterion.

## Baseline and dependencies

- Repository: `en0ma/hybrid`, baseline merge of PR #51 at `82a469d4c90ec51489c30608a3dbdfa72e1c14dc`.
- Existing active collateralized bid/ask swaps, linked maker lifecycle, host-only two-page auditor, passive LP principal infrastructure.
- This branch is created independently from main to support sequential merges. Rebase after preceding milestone(s) merge and reverify full exact-head CI.
- Do not relax 262,144-byte on-chain ELF limit, CU gates, ownership/PDA/signature checks or vault accounting.

## Architectural workstreams

1. **Workstream 1.** Read consistent slot-tagged market/custody/maker/page/owner snapshots from RPC and authenticate account data before planning.
2. **Workstream 2.** Extend auditing from two-page slices toward linked chains, detect closed/detached pages and unsafe/stale snapshots.
3. **Workstream 3.** Expose typed maker deposit, withdraw, place, cancel, replace and page-lifecycle transaction plans with true market-authority constraints.
4. **Workstream 4.** Build idempotent reconciliation loop: desired vs resting quotes, owned order sequence/index tracking, balance/lock accounting, confirmation-aware retry and blockhash refresh.
5. **Workstream 5.** Document rate limits, commitment selection, transaction simulation, reorgs, partial failure and operator kill switch.
6. **Workstream 6.** Provide a sample inventory-aware two-sided quoting agent with safe dry-run defaults, metrics and deterministic mock-RPC tests.

## Concrete code touchpoints

host Rust SDK, RPC fetcher, off-chain model, transaction builder, example maker agent, integration harness

## Exit and reviewer gates

- [ ] End-to-end simulated maker can fund, quote both sides, consume fills, cancel, withdraw and reconcile after restart.
- [ ] Transaction retry never duplicates financial intent without explicit reconciliation.
- [ ] Malformed owner/PDA/link/slot snapshots are rejected; no SDK output is treated as on-chain authorization.
- [ ] Example remains dry-run unless explicit operator configuration enables sending.
- [ ] Threat model and interface docs reviewed; implementation changes and hostile fixtures committed.
- [ ] Rust format, Clippy, native unit/integration tests and real SBF execution pass.
- [ ] All GitHub PR Gate jobs, reviewer threads, pinned Manifest and Main Verification pass at exact head *after* ready-for-review transition.
- [ ] Merge only with expected head SHA, one milestone at a time, after independent human review of sensitive pathways.

## Known implementation hazards

- RPC reads are not transaction authorization and may be stale or inconsistent across accounts.
- Market-signature authority is required by page attach/detach; PDA authority cannot be assumed externally signable.
- Active fills cap maker balance account fan-out at eight; do not silently quote deeper executable liquidity.
- Passive principal accounting alone is not an executable AMM. Production marketing must not describe passive swaps until tested on-chain.
- Existing README describes older page-0-only behavior in places; reconcile factual docs as part of the applicable milestone.

## Sequencing

1. PR #52: Protocol security and solvency
2. PR #53: Market-maker operational integration **(this PR)**
3. PR #54: Trader and router integration
4. PR #55: Competitive performance benchmarking
5. PR #56: Complete hybrid liquidity execution

Merge ordering is recommended rather than proof of technical independence; resolve any affected interfaces by rebase and review.
