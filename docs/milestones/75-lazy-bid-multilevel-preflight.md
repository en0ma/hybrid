# Milestone 75 — multilevel BID preflight across authenticated page-shaped state

**Status: draft foundation. Not an executable settlement instruction.**

PR #73 merged the single-tick BID sealer. PR #74 increased its maker roster
from eight to sixteen seats, with a sendable ALT-backed v0 transaction at the
upper bound. Neither PR provides multi-price settlement or a multi-page
execution-derived sealer.

## This implementation slice

`crates/state/src/lazy_multilevel_bid.rs` adds
`plan_multilevel_bid(pages, sidecars, requested_base, min_quote_out)`, a
**pure, read-only** planner over the already decoded canonical BID book pages
and aligned maker-owner sidecars. It is deliberately not called by any
on-chain opcode.

- Cap input at **two linked pages, four executed price ticks, sixteen seats
  per executed tick**. These are **planner caps**, not measured limits for a
  sendable on-chain transaction.
- Validate page ordering, link reciprocity and sequential page IDs using the
  existing `validate_bid_chain` primitive. Require equal page/sidecar
  links and lengths; reject absent, zero, reused or duplicated maker owner
  keys across the entire supplied chain.
- Validate each resting price against the canonical engine sqrt interval.
  Group neighboring orders at **the same price and sqrt**, processing
  higher BID prices before lower ones. Distinct ticks never share a price,
  maker roster, weight distribution or rounding calculation.
- Allocate each tick using `allocate_bid_at_price` and the existing 100-bps
  passive base fee (rounded down per tick). Quote-paid and fee totals are
  checked on accumulation; per-tick claims telescope exactly.
- Reject partial/insufficient liquidity, zero fill, violated minimum quote,
  overfull tick rosters and any price/sqrt inconsistency, without mutating
  either book or sidecars.

The native tests model a full 32-seat head page and one-seat linked tail
with three distinct price levels: 16 seats at 2 quote/base, 16 at 1 and one
at 1/2. They verify crossing two and three price levels, independent
quote rounding/fees, conservation and hostile owner/page/limit rejection.

## Important separation of trust

A correctly shaped in-memory book is **not proof of authenticity**. Only
an on-chain instruction can verify program ownership, PDA derivation,
market/custody ownership, SPL mint and vault balances, the owner sidecar
head market tag and every later sidecar's canonical PDA.

Likewise, these `PlannedBidTick` values are **not Merkle-authenticated
maker credits**, and no new opcode accepts them from users.

## Next executable milestone acceptance

- [ ] Design and document a versioned multi-page execution ABI:
      market/custody/reserve; **one fresh epoch PDA per executed tick**;
      ordered, authenticated page/sidecar pairs; taker/vault accounts;
      canonical writable maker-balance accounts and any ALT requirements.
- [ ] Reject overlapping account metas, skipped or forged page sidecars,
      noncanonical PDAs, corrupted links, duplicated makers and malicious
      cross-market accounts **on-chain**.
- [ ] Preflight **all** tick legs and liabilities before first SPL CPI.
      Seal a separate immutable commitment and claim root per price tick;
      do not use the existing single-tick opcode 42 with averaged pricing.
- [ ] Apply per-tick book reductions and linked-page compaction atomically
      with full whole-transaction rollback on a failing late leg or CPI.
- [ ] Funded SBF integration for crossing at least two prices and two linked
      pages, subsequent maker claims, vault-floor conservation, odd trees,
      malicious accounts and rollback.
- [ ] Prove network-sendable v0/ALT packet size, compute, stack/heap,
      bytecode, SDK hash vectors and all exact-head CI gates before merging
      the eventual execution feature.

**No production enablement and no merge claim** follows from passing
these read-only state-level tests.
