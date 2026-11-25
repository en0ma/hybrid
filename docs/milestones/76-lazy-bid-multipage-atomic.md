# Milestone 76 — bounded atomic multipage BID settlement

**Status:** Experimental draft. New opcode 44 must pass funded SBF, packet-size,
compute/bytecode and adversarial review before it is made ready or merged.

## Opcode 44

Instruction data: `[44, base_fill:u64LE, min_quote_out:u64LE]`.

Fixed accounts in strict order:
`market, custody, lazy_reserve, fresh_epoch_0, fresh_epoch_1,
bid_page_0, bid_owner_0, bid_page_1, bid_owner_1,
taker_signer, taker_quote, taker_base, quote_vault, base_vault,
quote_mint, base_mint, vault_authority, token_program, payer_signer,
system_program`, then one **writable maker balance PDA per canonical
flattened book row**. All roles must be distinct.

The first deployment-shaped subset deliberately requires exactly two
linked BID pages, exactly two executed *distinct* price ticks and no more
than **8 total maker rows**. This cap is more conservative than opcode 42's
16-maker single-page transaction. Opcode 42 is unchanged.

### Execution invariants

1. Authenticate all linked page PDAs, market header binding for the head
   sidecar, canonical later sidecar PDAs, program ownership, linked-page
   order, unique maker owners, account aliases and book-count equality.
2. Independently compute base/quote/fee totals at each price tick, using
   the immutable per-tick roster and cumulative pro-rata rounding.
   Never use a weighted/averaged single price across the two ticks.
3. Validate every maker balance PDA, locked collateral and quote release
   before any CPI. Validate custody, mint ownership and decimals, token
   destination/authority and **historical** reserve floors.
4. Compute compacted linked-page results and exactly two freshly derived
   frozen epoch commitments/claim roots, advancing the reserve epoch
   cursor once per tick. Preflight both epochs and all next vault floors.
5. Initialize the two epoch PDAs, transfer exact base and quote through
   checked SPL CPIs, then write book, owner sidecars, maker balances,
   market/custody and reserve. Any failure aborts the Solana transaction.
6. Retire fully emptied tail pages to canonical empty state while moving
   surviving rows and their owner sidecars to the head page.

Each epoch uses the existing v1 BID pubkey commitment and claim ABI
(opcode 40). Claim seats and Merkle roots are *per tick*, not global
across the entire multipage instruction.

### Test and rollout gates

- [ ] Funded SBF seals two epochs across two pages and distinct prices.
- [ ] Epoch totals, roots, independent maker claims and fee accounting
      remain consistent after page compaction.
- [ ] Hostile sidecars, missing epochs, swapped/duplicated maker PDAs,
      slippage and late SPL CPI failures leave **every** account unchanged.
- [ ] Verify a sendable legacy or v0+ALT transaction with serialized
      wire bytes at or below 1,232 and measured CU below declared budget.
- [ ] Exact-head Rust formatting, Clippy, native, SBF, SDK, manifest,
      bytecode and review gates all pass.
- [ ] Cross-market/epoch replay isolation, multi-epoch claims, exhaustion,
      malformed account inputs and frozen-destination rollback pass SBF.

**Not yet generalized:** More than two ticks/pages, rosters over eight
in one atomic call, or production deployment. The preflight-only opcode
43 remains available and unchanged.

## Follow-up verification — 2026-11-25

The prior head `b5f2196190786c0559503abd7d3077a71f5274c3` passed the
native/SBF/fuzz/CU/bytecode job, Manifest comparison and baseline, mainnet
fork, exact-head verification, documentation, and funded Surfpool workflow.
The review gate rejected draft status; no inline review threads were returned.
These results apply only to that head, not subsequent commits.

Follow-up changes repair malformed YAML in the capital efficiency workflow,
add foreign-market epoch substitution, read-only maker, wrong system-program
and frozen quote destination rejection tests, and add a signed legacy packet
serialization test with all 28 opcode 44 account metas and two signatures.
The maximum-roster packet test uses synthetic additional account keys and
is not evidence of funded eight-maker execution or measured compute usage.

Remaining release blockers: successful exact-head CI, funded eight-maker
execution and measured CU, independent review of collateral/reserve conservation,
full cross-market claim replay and historical reserve cases, and adversarial
review. Keep this PR draft and the production SDK execution builder disabled.
