# Hybrid

Hybrid is a Solana-native hybrid order-book protocol: explicit CLOB orders plus compressed passive liquidity positions.

## V0 active-order lifecycle

Before the first maker order is placed, the client creates a normal account owned by Hybrid with exactly `ASK_OWNER_PAGE_BYTES` bytes. On the first placement, Hybrid binds that account public key into the market header's existing 32-byte reserved field.

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
3. writable Hybrid-owned ask-owner sidecar account
4. maker signer

The ask-page PDA uses `["ask-page", market, page_index_le]`.

The owner sidecar does not use a PDA. A client creates the account with the System Program and sets Hybrid as the owner. The first successful placement binds its public key to the market. Later placement and cancellation instructions require the same bound account.

The program allocates the order sequence from `MarketHeader.next_sequence`, inserts by price-time priority, increments `ask_count`, and persists both pages.

### Cancel ask

Opcode: `7`

Instruction data is exactly 9 bytes:

- byte 0: opcode
- bytes 1..9: order sequence, little-endian `u64`

Accounts are the same as placement.

Cancellation succeeds only when the signer public key matches the owner entry parallel to the target order. A successful cancel compacts both pages and decrements `ask_count`.

## V0 bid-side lifecycle

Hybrid now keeps a page-0 bid book with the same 48-byte order entry shape as asks.

- `BidPage` sorts by descending price and then ascending sequence.
- The bid-page PDA uses `["bid-page", market, page_index_le]`.
- A parallel Hybrid-owned owner sidecar preserves maker authorization.
- `MarketHeader.bid_count()` uses existing reserved header bytes, so the market header remains 128 bytes.
- The bid owner sidecar is bound with a 16-byte public-key tag stored in existing reserved header bytes. This avoids another market-header account or a state-size increase.

### Place bid

Opcode: `8`

Instruction data is exactly 41 bytes and uses the same fields as ask placement:

- byte 0: opcode
- bytes 1..17: `price_x64`, little-endian `u128`
- bytes 17..33: `sqrt_price_x64`, little-endian `u128`
- bytes 33..41: `base_qty`, little-endian `u64`

Accounts:

1. writable market account, owned by Hybrid
2. writable bid-page PDA
3. writable Hybrid-owned bid-owner sidecar account
4. maker signer

The first successful bid placement binds the sidecar tag and increments the shared global order sequence.

### Cancel bid

Opcode: `9`

Instruction data is exactly 9 bytes:

- byte 0: opcode
- bytes 1..9: order sequence, little-endian `u64`

Cancellation requires the maker signer that is stored parallel to the bid entry.

## Deterministic execution planning

The engine exposes `plan_buy_exact_in_levels`.

The planner uses the same matcher path as `quote_buy_exact_in_levels`. It returns the canonical quote plus up to 16 explicit active-fill records. Each record contains:

- ask slice index
- filled base quantity
- quote quantity consumed

This gives future mutable swap execution a deterministic fill plan without re-deriving which explicit orders were touched.

Opcode `10` is a state-backed planner probe for SBF and CU validation. It does not mutate state.

## Current limits

This is a bounded V0 mutation path.

- Placement and cancellation currently target page index `0` for both asks and bids.
- Bid-side passive matching is not implemented yet.
- Token custody, maker deposits, settlement and withdrawals are not implemented yet.
- The state-backed matcher and planner probes remain read-only.
- Mutable swap execution will be added only together with collateralized custody and settlement. Hybrid does not delete third-party maker orders without an atomic asset-transfer path.

