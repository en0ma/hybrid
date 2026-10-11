# Milestone PR #52: Protocol security and solvency

> Status: implementation workstream opened. This document defines engineering scope and acceptance, **not evidence that the scope is implemented**. PR remains draft until code, tests and exact-head gates meet every criterion.

## Baseline and dependencies

- Repository: `en0ma/hybrid`, baseline merge of PR #51 at `82a469d4c90ec51489c30608a3dbdfa72e1c14dc`.
- Existing active collateralized bid/ask swaps, linked maker lifecycle, host-only two-page auditor, passive LP principal infrastructure.
- This branch is created independently from main to support sequential merges. Rebase after preceding milestone(s) merge and reverify full exact-head CI.
- Do not relax 262,144-byte on-chain ELF limit, CU gates, ownership/PDA/signature checks or vault accounting.

## Architectural workstreams

1. **Workstream 1.** Global per-asset conservation: SPL vault balances >= sum maker free+locked liabilities + passive principal/fees, with reconciled custody totals.
2. **Workstream 2.** State-machine sequences covering create/deposit/place/partial fill/full fill/cancel/withdraw/page attach/detach/close/recreate and passive LP open/close.
3. **Workstream 3.** Adversarial signer, PDA, account-ownership, aliasing, stale owner-sidecar, duplicate sequence, crossed-page ordering, count and header corruption tests on both bid and ask routes.
4. **Workstream 4.** Check integer/rounding and quote-reserve dust at extremes; preserve maker and taker conservation on success and exact state rollback on failure.
5. **Workstream 5.** Characterize legacy market and page migration; document upgrade authority, trust model, emergency procedures and third-party audit requirements.
6. **Workstream 6.** Measure worst-case CU and ELF size; disallow raising 262144-byte cap to hide failures.

## Concrete code touchpoints

program/src/lib.rs, linked_maker.rs, multipage_buy.rs, multipage_sell.rs, page_lifecycle.rs, crates/state, crates/settlement, program tests

## Exit and reviewer gates

- [ ] Native, SBF, state, fuzz, Manifest, mainnet-fork, CU and bytecode gates pass at exact head.
- [ ] Each supported maker/taker/LP mutation covered by before/after solvency assertions and at least one adversarial rollback scenario.
- [ ] No untriaged critical/high findings; document residual risks and independent audit requirement.
- [ ] No deployable on-chain size-limit increase.
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

1. PR #52: Protocol security and solvency **(this PR)**
2. PR #53: Market-maker operational integration
3. PR #54: Trader and router integration
4. PR #55: Competitive performance benchmarking
5. PR #56: Complete hybrid liquidity execution

Merge ordering is recommended rather than proof of technical independence; resolve any affected interfaces by rebase and review.
