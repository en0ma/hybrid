# Milestone PR #55: Competitive performance benchmarking

> Status: implementation workstream opened. This document defines engineering scope and acceptance, **not evidence that the scope is implemented**. PR remains draft until code, tests and exact-head gates meet every criterion.

## Baseline and dependencies

- Repository: `en0ma/hybrid`, baseline merge of PR #54 at `46da2403ebf5ea592fd0456da17411977e3083f2`.
- Existing active collateralized bid/ask swaps, linked maker lifecycle, host-only two-page auditor, passive LP principal infrastructure.
- This branch was reset onto the PR #54 merge commit before benchmark implementation; reverify full exact-head CI after subsequent changes.
- Do not relax 262,144-byte on-chain ELF limit, CU gates, ownership/PDA/signature checks or vault accounting.

## Architectural workstreams

1. **Workstream 1.** Measure maker place, cancel, amend-via-replace, taker buy/sell and depth traversal for 1/2/8 page shapes and varying makers.
2. **Workstream 2.** Capture CU, account metas, transaction bytes, priority fee exposure, rent, failed transaction rates, and per-operation state growth.
3. **Workstream 3.** Use reproducible Solana versions, environment, input books, fee assumptions and identical scenario definitions.
4. **Workstream 4.** Benchmark Manifest, Phoenix and OpenBook V2 only where legally and technically reproducible, clearly labeling unavailable/incommensurate flows.
5. **Workstream 5.** Pin reference workflows and enforce Hybrid non-regression budgets without masking results through increased caps.
6. **Workstream 6.** Publish machine-readable outputs and human-readable tables; explicitly separate measured numbers from theoretical claims.

## Concrete code touchpoints

benchmark harness, CI artifacts, results schema, methodology, comparable protocol baselines

## Exit and reviewer gates

- [ ] A single documented command recreates fixture data and Hybrid baseline results.
- [ ] CI uploads machine-readable reproducible summaries with commit/toolchain provenance.
- [ ] Comparative claims include scenario parity caveats and sources; no unsupported 'fastest' claims.
- [ ] Security/bytecode checks remain required; benchmark gates do not relax existing tests.
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
4. PR #55: Competitive performance benchmarking **(this PR)**
5. PR #56: Complete hybrid liquidity execution

Merge ordering is recommended rather than proof of technical independence; resolve any affected interfaces by rebase and review.
