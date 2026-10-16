# P2 decision record

**Status:** Research in progress; no on-chain changes authorized.

## Evidence collected

- Existing Hybrid maker balance: explicit free/locked asset state, 64-byte serialized account.
- Existing maker reconciliation: cancel-first planning with freshness/confirmation boundary.
- Offline multi-market model now reproduces the central global-capital failure case: two markets can both advertise liquidity against one pool, while only a serially protected first fill can consume inventory. This is **not** a measured Solana program or Manifest result.

## Preliminary engineering preference

**Do not build shared global collateral as the first P2 execution change.** Prototype a narrow **reverse-order** mechanism or safe cancel/replace path first *after* live devnet maker UX exists, because these can potentially preserve existing market-scoped solvency. This is a research prioritization, **not** approval of an executable opcode.

## Open research gates

- [x] Record baseline account semantics and failure conditions
- [x] Runnable deterministic cross-market capital model with negative tests
- [ ] Pinned Manifest version plus equivalent-workload benchmark
- [ ] SBF prototype with adversarial concurrent settlement
- [ ] Rent, compute unit, account-meta, and maker inventory utilization figures from executed transactions
- [ ] Migration/ABI/authority and token-program compatibility review
- [ ] Public devnet evidence and UX comparison

## Decision

Pending empirical comparisons and security review. No mechanism should be merged into mainline settlement from this research PR.
