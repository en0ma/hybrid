# Hybrid reference quoting strategy (P1)

This module demonstrates **bounded, deterministic, integer-only target quote planning**. It does not connect to an exchange, sign transactions, manage keys, calculate Q64 order prices, or submit actual orders. Its input price is an **exact decimal string** in quote atoms per base atom (e.g. `"0.15"` when the quote token has 6 decimals and the base token has 9). Output `priceAtoms` may therefore be fractional. Bid quote collateral is conservatively rounded **up** to the next whole quote atom; actual order prices must be converted to protocol-compatible Q64 with audited rounding before submission. This is strategy testing only.

```bash
node --test tools/quoting-bot/plan.test.mjs
```

## Safe integration design

1. Authenticate a sufficiently fresh market snapshot and maker inventory.
2. Use explicit risk budgets (base/quote), spread, levels, and size.
3. Convert target prices into protocol-compatible Q64 fields using audited helpers.
4. Feed authenticated orders and targets to `reconcile_maker`, cancel then confirm, refresh, simulate placements and sign only after maker approval.
5. Journal signatures; never retry an uncertain submission blindly.

**Completion criteria:** live snapshot reader, correct Q64 conversion, integrated maker reconciliation, kill switch, pending-signature journal, simulation and confirmations, independent fill/inventory reconciliation, and long-duration public-devnet tests. Never run this initial reference against funded wallets unattended.

## Authenticated live observation (no trading)

`live-observer.mjs` uses the SDK's `loadActivePageSnapshot` to read ask
and bid page-zero state from Solana RPC with program-owner checks, canonical
book decoding, sidecar bindings and slot consistency. It computes the
midpoint in Q64 using BigInt, checks maximum slot lag and passes a decimal
mid-price to the existing risk-bounded quote planner.

A calling application must configure canonical **derived and verified** market,
page and sidecar PDA addresses, and a trusted RPC connection. This API does not
derive those addresses, authenticate maker balances, or prove RPC honesty.

It returns `mode: "observation-only"` and **never signs or submits a trade**.
The caller must supply the maker identity and canonical maker-balance PDA. The
read-only observer verifies program ownership, market and maker identity,
account encoding, and **exact context-slot equality** with the book. The
operator-configured risk budgets are capped by the authenticated **free**
collateral balances; locked inventory cannot be promised to new quotes.
Read-only observation fails closed if RPC cannot provide coherent snapshots.
The caller is still responsible for independently validating all PDA derivations. Live execution must remain disabled until the
maker inventory account is authenticated, prices converted to valid Q64 and
lots, transaction simulation and confirmation are integrated, transaction
retries journaled, and operator-controlled kill switches are tested.

Run unit tests:

```bash
node --test tools/quoting-bot/plan.test.mjs tools/quoting-bot/live-observer.test.mjs
node --test sdk/test/maker-balance.test.mjs
```
