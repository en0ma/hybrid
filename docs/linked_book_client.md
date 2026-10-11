# Linked-book lifecycle client (host Rust)

The `hybrid_program::client` module provides typed, host-only builders for the implemented market and maker lifecycle. It does **not** change the Solana program ELF. All builders return a complete `Instruction` with explicit writable and signing metadata, and return errors for invalid page ranges, zero amounts and duplicate accounts.

## PDA conventions and legacy sidecars

`canonical_page(market, side, index)` derives `ask-page` / `bid-page` and owner-sidecar PDAs. The initial head owner account is permitted to be **non-PDA** and is authenticated through the market header. Supply this address explicitly to `TwoPageBook::new`. For index 1 and above, the sidecar **must be** the canonical owner PDA. `maker_balance(market, owner)` and `custody(market)` derive the other stable PDAs.

## Full page lifecycle

Create a `TwoPageBook::new(market, legacy_head_sidecar, Side::Ask)` or `Side::Bid`. Then:

1. Call `recreate_detached(payer, system_program)` only after the page-1 pair has been closed (opcode 28).
2. Call `attach()` (opcode 29) with the **market account signing**, while the head and tail are empty. The market must sign; an arbitrary keeper cannot alter links.
3. Use `place(0 | 1, maker, price_x64, sqrt_price_x64, base_qty)` (opcode 30). Supply the maker signature and a funded maker-balance PDA; price and sqrt must satisfy the program's limit-order validation, not just be nonzero.
4. Use `cancel(0 | 1, maker, sequence)` (opcode 31) with the maker signature to return locked collateral. An unrelated maker may not cancel someone else's order.
5. After the tail is empty, `detach()` (opcode 32) requires the market signature. Detached pages can then be closed with `close_detached()` (opcode 27), which sends lamports to the market, **not to the keeper**.

`TwoPageBook` supports **two pages only**; it does not claim automatic multi-page order placement, market governance management or historical rent-funder refund. A market Pubkey is not necessarily an externally controlled signing key; for markets whose PDA cannot sign a top-level instruction, attach/detach are not available through these instructions. Do not treat an offline builder as proof of on-chain authority.

## Linked SPL Token swaps

`LinkedSwap::instruction(side, exact_out, amount, limit)` constructs the existing four linked-book swap opcodes. The builder preserves the direction-specific token account ordering: quote/base for buy (23/24) and base/quote for sell (25/26). Provide canonical pages in head-to-tail order, their authenticated sidecars, and the maker-balance PDA accounts covering execution. At runtime the program still checks link order, remaining depth, swap limits, actual token program ownership and collateral accounting.

The builder is not an RPC client and cannot discover maker PDAs or confirm page balances. Never infer a successful trade from `Ok(Instruction)`: simulate/send and inspect transaction results.

## Integration coverage and non-goals

Unit tests cover both sides, all maker lifecycle opcodes, all four swap opcodes, signer/writable metas, canonical PDA derivation, account order and invalid inputs. The compiled program's existing genuine SPL Token SBF tests cover two-page attach, maker cancellation, collateral changes and linked swaps; additional negative-path SBF tests are included in this PR. Generic Surfpool fork CI is **not** claimed to execute new opcodes unless specific tests demonstrate that.

All changes remain under the pinned CI bytecode (262144 bytes), CU, state-size, Manifest and exact-head review gates. The host module deliberately is not built into the Solana target.
