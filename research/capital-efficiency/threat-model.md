# Capital reuse threat model

## Scope / observed Hybrid baseline

`crates/settlement/src/lib.rs` defines a 64-byte maker balance with four integer buckets: `base_free`, `base_locked`, `quote_free`, `quote_locked`. Placement reserves collateral; cancellation releases it; active fills debit locked inventory and credit maker proceeds. Existing settlement tests verify non-overdraw and reject invalid multi-maker settlement without partially mutating balances.

`program/src/client_maker.rs` builds cancel-first target reconciliation, explicitly requiring confirmation and a fresh state read before placements. The browser harness is not an automated quoting/signing engine.

**No shared global order collateral pool exists in the verified Hybrid baseline.**

## Trust model for any shared-reserve proposal

### Identity and mint boundaries
- Only positions backed by the **same SPL mint** and token-program semantics can share a fungible reserve. Never combine base and quote across different mints or decimal assumptions.
- A global reserve must be owned/authenticated against an explicit maker and quote mint; market and vault authorities must be validated.
- An authorized maker may create offers, but cannot spend assets already committed by another settled offer.

### Atomicity and concurrency
- Reserve commitment and actual fill must be verified in **one Solana transaction** or enforced by a globally serialized account update with no overspend window.
- If market A consumes inventory, market B's apparently executable quantity may become unfillable; this is a liquidity reliability trade-off, not free capital.
- Account write-lock contention and maker account fan-out may erase theoretical cost savings.
- Idempotence must be keyed to chain state, not an unconfirmed client intention or RPC timeout.

### Withdrawal and settlement
- Withdrawals must deduct unencumbered funds only; unsettled fill attempts cannot permit draining a globally pledged reserve.
- Partial fills, cancellation, failed token CPI, and fee rounding must preserve reserve/vault reconciliation.
- Reject stale sidecars, wrong mints, duplicate makers, mismatched market authorities and unsupported Token-2022 extensions unless audited.

## Minimum adversarial tests before implementation

1. Two markets hit the same full reserve in competing transactions.
2. Maker withdrawal races a taker fill in another market.
3. Retry or replay after unknown transaction inclusion status.
4. Partial fill and cancel interleave while global orders exist elsewhere.
5. Wrong mint/token owner and intentionally swapped vault accounts.
6. Account eviction/upgrade and corrupted reserve layout.
7. Compute budget and account-lock contention with eight-maker fan-out.

**Kill criterion:** if a candidate cannot prove sufficient unspent tokens and deterministic authority in its on-chain settlement path, do not deploy it.
