# PR #57: Funded passive settlement, fees and LP redemption

This PR is a security-sensitive implementation workstream continuing incomplete milestone #56. The existing passive pool stores segregated principal and fees but does **not** execute SPL-backed passive swaps; closing positions currently returns deposited principal only.

## Acceptance gates — all must be met before merge

- [ ] Quote/execute passive buy and sell against actual Solana SPL custody, updating authenticated pool and market price.
- [ ] Exact-in/exact-out both directions are tested at execution level with checked slippage and rounding.
- [ ] LP fee growth/distribution is deterministic across overlapping positions and partial withdrawals.
- [ ] LP redemption returns correct post-trade inventory plus earned fees with vault coverage after every CPI.
- [ ] Hostile SBF transaction fixtures demonstrate atomic rollback and reject forged vaults, positions, signatures and stale states.
- [ ] Full existing PR Gate, SBF ELF <= 262144 bytes, pinned Manifest and fork comparisons pass without weakened thresholds.
- [ ] Independent security review of newly executable settlement code before production activation.

Never mark this as complete based on accounting unit tests alone. The existing passive principal bytes cannot be treated as current redeemable assets once trading is enabled; use separately persisted position accounting or a migration, and preserve old positions' claims.
