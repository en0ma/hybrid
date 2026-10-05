# Hybrid

Hybrid is a Solana-native hybrid order-book protocol: explicit CLOB orders plus compressed passive liquidity positions.

## V0 active-order lifecycle

Hybrid keeps price-time matching data and maker ownership data in separate pages.

- `AskPage` stays on the matcher hot path.
- `AskOwnerPage` is a parallel sidecar used only by maker mutation instructions.
- The sidecar preserves one 32-byte maker public key for each sorted ask entry.
- Insert and cancel operations update the ask page and owner sidecar together.
- Ordinary state-backed matching does not require the owner sidecar account.

The current mutation prototype supports page index `0` only.

### Place ask

Opcode: `6`

Instruction data is exactly 41 bytes:

- byte 0: opcode
- bytes 1..17: `price_x64`, little-endian `u128`
- bytes 17..33: `sqrt_price_x64`, little-endian `u128`
- bytes 33..41: `base_qty`, little-endian `u64`

Accounts:

1. writable market account, owned by Hybrid
2. writable ask-page PDA
3. writable ask-owner-page PDA
4. maker signer

The ask-page PDA uses `["ask-page", market, page_index_le]`.

The owner-page PDA uses `["ask-owner-page", market, page_index_le]`.

The program allocates the order sequence from `MarketHeader.next_sequence`, inserts by price-time priority, increments `ask_count`, and persists both pages.

### Cancel ask

Opcode: `7`

Instruction data is exactly 9 bytes:

- byte 0: opcode
- bytes 1..9: order sequence, little-endian `u64`

Accounts are the same as placement.

Cancellation succeeds only when the signer public key matches the owner entry parallel to the target order. A successful cancel compacts both pages and decrements `ask_count`.

## Current limits

This is a bounded V0 mutation path.

- Placement and cancellation currently target only ask page index `0`.
- Bid-side active orders are not implemented yet.
- Token custody and maker deposit accounting are not implemented yet.
- Matching remains read-only in the current state-backed probe instructions.
