# Milestone 75 — multilevel BID preflight across authenticated page-shaped state

**Status: draft read-only native + on-chain preflight. Not an executable settlement instruction.**

PR #73 merged the single-tick BID sealer. PR #74 increased its maker roster
from eight to sixteen seats, with a sendable ALT-backed v0 transaction at the
upper bound. Neither PR provides multi-price settlement or a multi-page
execution-derived sealer.

## This implementation slice

`crates/state/src/lazy_multilevel_bid.rs` adds
`plan_multilevel_bid(pages, sidecars, requested_base, min_quote_out)`, a
**pure, read-only** planner over the already decoded canonical BID book pages
and aligned maker-owner sidecars. Opcode 43 now calls this planner **only**\nafter on-chain authenticated account preflight; opcode 43 never settles.

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

## On-chain read-only preflight (opcode 43)

Instruction data: `[43, page_count:u8, requested_base:u64LE, min_quote_out:u64LE]`.
Account order: market, then canonical ordered `(BID page, BID owner sidecar)`
pairs for page IDs `0..page_count`. Both page and sidecar accounts must be
program owned and disjoint. BID page PDAs must match their market/index
seeds; head sidecar must match the market-header owner tag, and each later
sidecar must be the canonical `bid-owner-page` market/index PDA. Decoded
page/sidecar links and lengths must agree, and the flattened maker-seat
count must equal the authenticated market BID order count. The pure planner
checks all remaining link/order/owner/price/fill invariants.

Funded SBF tests exercise two linked pages with prices 1 and 1/2,
positive cross-price quote output and zero state changes. Malicious
sidecar substitution, duplicated accounts, omitted pages, insufficient
quote output and malformed wire length must all fail without mutations.

**Opcode 43 is validation only:** it does not authenticate custody or SPL
balances, reserve liabilities, maker balance PDAs or epoch destination PDAs.
It creates no epoch and moves no tokens. It cannot be used to execute an
order or claim proceeds. Opcode 42 is unchanged.

## Important separation of trust

A correctly shaped in-memory book is **not proof of authenticity**. Opcode 43 now verifies program ownership, page/sidecar PDA derivation,
market state and the authenticated sidecar chain, **but does not verify**
custody, mint, SPL vault balances or executable maker collateral. These
remain mandatory checks for the eventual mutating instruction.

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
