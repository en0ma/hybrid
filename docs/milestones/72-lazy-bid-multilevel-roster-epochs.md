# Follow-up to PR #71: bid-side, multilevel, larger rosters and multi-epoch adversarial settlement

**Status: draft implementation plan with first cross-epoch reserve test.**
This document records the scope of the new draft PR; it does **not** claim
that bid-side or multilevel lazy settlement is implemented.

## Baseline and scope

PR #71 introduced opcode 41, a single-tick, single-page ASK execution that
seals an execution-derived immutable epoch with at most eight maker seats.
Opcode 40 authenticates and atomically pays maker claims, while the
market-wide `LazyReserve` tracks unclaimed liabilities across epochs.
Existing `multipage_buy.rs` and `multipage_sell.rs` implement separate
state-backed matching paths; they are **not** equivalent to lazy
execution-derived settlement. Existing `MAX_FROZEN_MAKERS = 4096` is a
bitmap capacity, **not** evidence that a 4096-maker on-chain sealer fits CU,
transaction account limits or the current instruction ABI.

All four workstreams below must have code and funded SBF evidence before this
PR is considered feature-complete. Keep the PR in draft while incomplete.

## 1. Bid-side execution-derived lazy epochs

- Introduce a dedicated bid-side paid execution-and-seal path. Do not
  reinterpret opcode 41's ASK ABI or accept operator-supplied allocations
  or Merkle roots.
- Authenticate program-owned linked bid data and owner sidecars, maker
  identities, market/custody/reserve state, mint and SPL vault authority.
- Use canonical cached-square-root validation and existing engine quote
  arithmetic. Debit locked maker quote collateral **at execution**, credit
  maker base proceeds to a reserved base-vault liability, and segregate
  base-denominated protocol fees. Defer the corresponding maker-balance
  debit until an authenticated claim.
- Seal `side = 1` epoch totals in canonical order:
  `[base_debit, base_credit, quote_debit, quote_credit, base_fee, quote_fee]`;
  the bid-side fill may use only `base_credit`, `quote_debit`, and
  `base_fee` as nonzero components.
- Reuse opcode 40 claim verification and token payout semantics only after
  funded tests prove mint/authority, vault-floor, replay and rollback safety.

**Acceptance:** funded bid fills, partial/full claims, fee conservation, bad
mint/authority/owner/proof and frozen-destination rollback pass SBF.

## 2. Multi-tick, multi-page execution

- Explicitly specify the execution instruction's ordered page/sidecar
  account ABI, the authenticated page chain and per-tick fill boundaries.
  Never derive an untrusted roster from arbitrary account ordering.
- Verify each page link, canonical price interval and owner sidecar; reject
  duplicated pages/makers where the transaction semantics require unique
  seats, malformed links and off-market accounts.
- Define whether each price tick produces a distinct immutable epoch
  (preferred for the current single-tick commitment format), or introduce
  a versioned multi-tick commitment with independent verifier/SDK vectors.
  Do not silently use one tick price for a fill spanning ticks.
- Preflight all executable legs, limits, token balances, vault liabilities
  and remaining account space before transfers or state writes. Ensure
  whole-transaction rollback on any failed leg or CPI.
- Bound pages and total maker seats explicitly according to measured
  Solana transaction account limits, SBF stack/heap and compute budgets.

**Acceptance:** funded crossing of at least two prices and two linked pages,
correct price-by-price proceeds, deterministic roots and owner sidecars,
partial page compaction, and hostile-account/limit/CPI rollback.

## 3. Maker rosters beyond eight

- The existing ASK sealer uses `MAX_LAZY_SEATS = 8`, a fixed-size
  commitment preimage buffer, an eight-leaf hash array and direct account
  lists. Increasing only the constant is **not** sufficient.
- Choose and version an explicit bounded roster/page encoding and
  account-passing strategy, with deterministic seat ordering, unique
  program-authenticated maker public keys and a commitment domain.
- If claiming remains on the 912-byte bitmap-backed epoch format,
  enforce `maker_count <= 4096` plus a **separately measured and smaller**
  maximum per execution transaction. The settlement library currently
  accepts at most 32 weights, which must not be mistaken for an SBF limit.
- Update Rust and SDK root vectors together. Preserve allocation
  telescoping, exact dust/fee conservation, zero-fill handling and proof
  depth checks across odd and boundary roster sizes.

**Acceptance:** funded boundary tests at eight and above eight seats, odd
Merkle trees, 0/duplicate/unowned makers, oversized roster rejection,
deterministic SDK/on-chain commitments and measured CU/bytecode.

## 4. Multi-epoch adversarial SBF integration tests

- Exercise at least three sequential epochs, including interleaved ASK and
  BID settlement, reordered maker claims, partial claims, and independent
  immutable epoch roots/bitmaps.
- Assert the monotonic epoch ID and rejection of reused/cross-market PDAs,
  cross-epoch proof swaps, double claims, excessive totals and reserve
  underflow. Assert no failed transaction changes any program or SPL account.
- Include failures after intermediate book updates, frozen SPL destinations,
  mismatched mints, malformed page chains and near-capacity maker rosters.
- Check vault floors across every intermediate state and prove protocol
  fees remain segregated through final claims.
- Cover transaction budgets, stack diagnostics, bytecode gates, Rust/SDK
  vectors and funded Surfpool SBF execution on the **exact PR head**.

**Initial code in this PR:** a three-epoch, mixed-side, interleaved-claim
`LazyReserve` unit regression. This is *not* a substitute for the funded
SBF cases above.

## Rollout and merge criteria

- [ ] Bid-side atomic execution and claims pass funded SBF.
- [ ] Multi-tick and multi-page atomic execution passes funded SBF.
- [ ] Larger-than-eight-maker execution is implemented and benchmarked;
      ABI, SDK and Merkle vectors match.
- [ ] Multi-epoch adversarial funded SBF tests pass.
- [ ] Existing ASK opcode 41, opcode 40, custody invariants and the SDK
      remain backwards compatible, or changes use a reviewed versioned ABI.
- [ ] Every CI lane (Rust, SBF, fuzz, state bytes, CU, ELF, Manifest comparison,
      funded lifecycle, exact-head and review threads) passes unchanged or
      with a documented, approved budget revision.
- [ ] Security review covers both settlement directions, cross-page
      authenticity, epoch isolation, Merkle proofs and custody solvency.
- [ ] Fixed cluster genesis-domain configuration is in place before
      production deployment; no automated funded trading is enabled early.

The end state is **not achieved** by merely merging this planning document
and initial reserve-state regression test.
