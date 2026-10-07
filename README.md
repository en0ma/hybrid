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



## Custody and collateralized maker balances

Hybrid now has a custody foundation for backed active liquidity.

### Custody state

The canonical custody PDA is:

`["custody", market]`

It stores:

- market public key
- base mint
- quote mint
- base vault
- quote vault
- base and quote decimals
- total accounted base
- total accounted quote

The vault authority PDA is:

`["vault-authority", market]`

The base and quote token vaults must be SPL Token accounts owned by this vault-authority PDA.

### Maker balance state

The canonical maker-balance PDA is:

`["maker-balance", market, maker]`

Each maker balance stores:

- free base
- locked base
- free quote
- locked quote

Resting asks lock base collateral.

Resting bids lock the full quote notional required by the order price and base quantity.

Cancellation returns locked collateral to the maker's free balance.

### Initialize custody

Opcode: `11`

Instruction data:

- byte 0: opcode
- byte 1: base mint decimals
- byte 2: quote mint decimals

Accounts:

1. Hybrid-owned market
2. writable custody PDA
3. writable payer signer
4. base mint
5. quote mint
6. base token vault
7. quote token vault
8. system program
9. SPL Token program

The program validates mint decimals, vault mints, and vault authority before creating the custody PDA.

### Initialize maker balance

Opcode: `12`

Accounts:

1. Hybrid-owned market
2. custody PDA
3. writable maker-balance PDA
4. maker signer
5. writable payer signer
6. system program

### Deposit

Opcode: `13`

Instruction data:

- byte 0: opcode
- byte 1: asset selector, `0 = base`, `1 = quote`
- bytes 2..10: amount, little-endian `u64`

Accounts:

1. market
2. writable custody
3. writable maker balance
4. maker signer
5. writable maker source token account
6. writable market vault
7. mint
8. SPL Token program

The token transfer and internal credit happen atomically.

### Withdraw

Opcode: `14`

Instruction data uses the same asset selector and amount encoding as deposit.

Accounts:

1. market
2. writable custody
3. writable maker balance
4. maker signer
5. writable market vault
6. writable maker destination token account
7. mint
8. vault-authority PDA
9. SPL Token program

Only free collateral can be withdrawn.

### Active-order ABI change

Ask and bid placement/cancellation now require the maker-balance PDA in addition to the existing market, page, owner sidecar and maker signer.

This is an alpha ABI change.

The purpose is to remove the previous unbacked active-order path. New resting orders cannot be created unless the maker has enough free collateral to lock the position.

## Current execution boundary

This PR establishes custody and collateralization, but it does not yet expose the final Jupiter swap instruction.

The next execution step can atomically consume locked maker collateral, credit maker proceeds, mutate active pages and move taker tokens through the custody vaults without introducing unsecured accounting.
