# Milestone PR #56: Complete hybrid liquidity execution

> Status: implementation workstream opened. This document defines engineering scope and acceptance, **not evidence that the scope is implemented**. PR remains draft until code, tests and exact-head gates meet every criterion.

## Baseline and dependencies

- Repository: `en0ma/hybrid`, baseline merge of PR #55 at `21d44ccb685360348e4dd74b0d21f1a50be51190`.
- Existing active collateralized bid/ask swaps, linked maker lifecycle, host-only two-page auditor, passive LP principal infrastructure.
- This branch is created independently from main to support sequential merges. Rebase after preceding milestone(s) merge and reverify full exact-head CI.
- Do not relax 262,144-byte on-chain ELF limit, CU gates, ownership/PDA/signature checks or vault accounting.

## Architectural workstreams

1. **Workstream 1.** Design and implement canonical range positions, liquidity minting validation, fee growth accounting and tick/range crossing semantics.
2. **Workstream 2.** Wire passive exact-in/out swap execution to authenticated SPL custody with explicit vault coverage and LP obligation checks.
3. **Workstream 3.** Enforce deterministic priority and boundary behavior between active price-time orders and passive executable liquidity on both trade directions.
4. **Workstream 4.** Define precise fee distribution, rounding, dust, protocol fees (if any) and LP redeemability under partial position usage.
5. **Workstream 5.** Cover multi-position overlapping ranges, zero-liquidity boundaries, adverse price movement, stale account feeds and insolvency attacks.
6. **Workstream 6.** Preserve the strict 262144-byte deployed ELF cap; if implementation does not fit, split architectural components or remove nonessential on-chain code rather than raising limits.

## Concrete code touchpoints

crates/engine, crates/state/passive, crates/settlement, program passive-pool paths, SPL tests, docs

## Exit and reviewer gates

- [ ] Funded LPs can deposit, execute actual swaps against passive liquidity, accrue attributable fees and redeem correct liabilities.
- [ ] Crossed active/passive execution exactly conserves tokens under fuzz and transaction-level SBF tests.
- [ ] Deterministic quote/execution parity for both directions and exact-in/out variants.
- [ ] External security review required before production activation.
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
2. PR #53: Market-maker operational integration
3. PR #54: Trader and router integration
4. PR #55: Competitive performance benchmarking
5. PR #56: Complete hybrid liquidity execution **(this PR)**

Merge ordering is recommended rather than proof of technical independence; resolve any affected interfaces by rebase and review.
