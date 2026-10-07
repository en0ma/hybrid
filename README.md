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

This is a bounded V0 execution path.

- Placement and cancellation currently target page index `0` for both asks and bids.
- Custody, maker deposits, withdrawals and collateralized active orders are implemented.
- Native taker execution currently consumes active asks only.
- A swap is rejected when satisfying the request would require passive liquidity.
- Passive LP vault accounting and passive mutable execution remain separate follow-up work.



## Collateralized execution core

Hybrid now separates settlement math from the on-chain entrypoint through the `hybrid-settlement` crate.

The crate defines the canonical maker collateral model:

- free base
- locked base
- free quote
- locked quote

Ask placement can reserve base collateral. Bid placement can reserve quote collateral. Cancellation can release the corresponding locked balance. Active fills consume locked maker collateral and credit the asset received by the maker.

The fixed `MakerBalanceAccount` layout is 64 bytes:

- 32-byte maker public key
- four 64-bit balance fields

The layout has a permanent state-byte budget and round-trip test.

The canonical active-fill plan can also be applied to an `AskPage`. Fully filled orders are removed and compacted. Partial fills reduce quantity in place. Fill indexes must be strictly increasing and cannot overfill an order.

Settlement fuzzing checks multi-maker fill conservation and rejects paths that would spend more collateral than a maker locked.


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

1. writable Hybrid-owned market signer
2. writable custody PDA
3. writable payer signer
4. base mint
5. quote mint
6. base token vault
7. quote token vault
8. system program
9. SPL Token program

The market must sign custody activation. Activation is allowed only when both active books are empty. The program then enables the market's collateralized-active flag.

The custody and maker-balance PDA creation paths support prefunded system-owned PDA addresses. If a PDA already holds lamports but has no data, Hybrid tops it up to rent exemption, allocates the required size, and assigns it to Hybrid instead of relying only on `CreateAccount`.

The program validates mint decimals, vault mints, and vault authority before creating or allocating the custody PDA.

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

### Active-order ABI and migration

Collateralized ask and bid placement/cancellation require the maker-balance PDA in addition to the existing market, page, owner sidecar and maker signer.

This is an alpha ABI change.

Markets created before collateral activation remain in legacy cancel-only mode. In that mode:

- new order placement is rejected;
- existing legacy asks and bids can be canceled with the old four-account ABI;
- legacy cancellation does not unlock maker collateral because those orders never locked collateral.

Custody activation requires both active books to be empty. After activation, the market flag requires the five-account collateralized ABI and new resting orders cannot be created unless the maker has enough free collateral to lock the position.

## Native active buy swaps

Hybrid now exposes native taker execution for collateralized active asks.

### Exact-in buy

Opcode: `15`

Instruction data is exactly 17 bytes:

- byte 0: opcode
- bytes 1..9: quote input, little-endian `u64`
- bytes 9..17: minimum base output, little-endian `u64`

### Exact-out buy

Opcode: `16`

Instruction data is exactly 17 bytes:

- byte 0: opcode
- bytes 1..9: requested base output, little-endian `u64`
- bytes 9..17: maximum quote input, little-endian `u64`

Both instructions use:

1. writable market
2. writable custody PDA
3. writable page-0 ask PDA
4. writable ask-owner sidecar
5. taker signer
6. writable taker quote token account
7. writable taker base token account
8. writable quote vault
9. writable base vault
10. vault-authority PDA
11. SPL Token program
12. up to 8 writable maker-balance PDAs

The program derives the active fill plan from the same engine primitives used for quoting. It then validates every touched maker-balance PDA against the owner sidecar, validates all collateral before mutation, transfers taker quote into the quote vault, transfers base from the base vault to the taker, credits maker quote balances, reduces or removes filled asks, compacts the owner sidecar, updates custody totals, and decrements `ask_count` for fully consumed orders.

The maker-account fan-out is bounded at 8 accounts. One maker account can settle multiple fills.

### Passive-liquidity boundary

Native swaps in this version are intentionally active-only.

The active plan consumes asks only while their price is at or better than the current passive marginal price. If the requested exact-in or exact-out amount would require passive liquidity, the instruction rejects instead of spending assets that do not yet have passive LP vault accounting.

This keeps active execution fully collateralized while preserving the future compressed-passive architecture.

### Slippage and quote parity

Exact-in requires the full requested quote input to be consumed by active asks and requires output to meet `min_base_out`.

Exact-out requires the full requested base amount to be available from active asks and requires quote input to remain at or below `max_quote_in`.

The engine exposes `plan_buy_active_exact_in` and `plan_buy_active_exact_out` so clients can reproduce the same active-only execution plan off-chain.

## Remaining execution limits

- swaps currently buy base with quote; the sell direction is not implemented yet;
- swaps currently use page-0 asks only;
- passive LP inventory accounting and mutable passive fills are not implemented yet;
- maker fan-out is bounded to 8 balance accounts;
- the fixed swap account set is still above the long-term Jupiter account-footprint target;
- exact-in/exact-out sell execution, passive settlement, multi-page mutable swaps and Jupiter adapter code remain follow-up work.
