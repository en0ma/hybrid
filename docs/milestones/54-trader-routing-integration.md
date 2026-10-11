# Milestone PR #54: Trader and router integration

> Status: implementation workstream opened. This document defines engineering scope and acceptance, **not evidence that the scope is implemented**. PR remains draft until code, tests and exact-head gates meet every criterion.

## Baseline and dependencies

- Repository: `en0ma/hybrid`, baseline merge of PR #51 at `82a469d4c90ec51489c30608a3dbdfa72e1c14dc`.
- Existing active collateralized bid/ask swaps, linked maker lifecycle, host-only two-page auditor, passive LP principal infrastructure.
- This branch is created independently from main to support sequential merges. Rebase after preceding milestone(s) merge and reverify full exact-head CI.
- Do not relax 262,144-byte on-chain ELF limit, CU gates, ownership/PDA/signature checks or vault accounting.

## Architectural workstreams

1. **Workstream 1.** Expose bidirectional exact-in and exact-out quotes with explicit precision/rounding, slippage, fee and active-liquidity exhaustion.
2. **Workstream 2.** Build account resolution for linked pages, sidecars, custody, vaults and up to eight touched maker balance PDAs.
3. **Workstream 3.** Provide typed executable transaction plans with priority-fee/CU hints, simulation validation, blockhash expiry and fresh-slot safeguards.
4. **Workstream 4.** Implement a routing-adapter interface with deterministic quote->build->simulate parity; do not claim deployed Jupiter support before integration.
5. **Workstream 5.** Create minimal wallet-facing trade examples and user-visible preflight errors for insufficient liquidity, maker fanout and invalid vaults.
6. **Workstream 6.** Add multi-maker and multi-page adversarial acceptance tests, including adverse price moves and transaction expiry.

## Concrete code touchpoints

host quote API, account resolver, simulator, client examples, routing adapter interface, SPL integration tests

## Exit and reviewer gates

- [ ] Independent test client performs buy and sell exact-in/out through simulated funded books.
- [ ] Quote vs execution deltas are bounded and surfaced, with deterministic slippage errors.
- [ ] No passive liquidity is represented as executable until on-chain passive settlement is enabled.
- [ ] Document account footprint, tested CU and RPC failure behavior.
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
3. PR #54: Trader and router integration **(this PR)**
4. PR #55: Competitive performance benchmarking
5. PR #56: Complete hybrid liquidity execution

Merge ordering is recommended rather than proof of technical independence; resolve any affected interfaces by rebase and review.
