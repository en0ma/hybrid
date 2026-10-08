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

## Native active sell swaps

Collateralized markets also support bounded active-only sells against page-0 bids.
Like active buys, these instructions require all input/output to be supplied
by eligible explicit orders; passive liquidity is not settled.

### Exact-in sell (opcode `17`)

17-byte instruction data: byte 0 is `17`, bytes 1..9 are the base input
(`u64`, little-endian), and bytes 9..17 are the minimum quote output
(`u64`, little-endian). The full base amount must execute.

### Exact-out sell (opcode `18`)

17-byte instruction data: byte 0 is `18`, bytes 1..9 are the requested
minimum quote output (`u64`, little-endian), and bytes 9..17 are the
maximum base input (`u64`, little-endian). Integer rounding may produce
more quote output than requested, but input cannot exceed the specified
maximum.

Both sell instructions use this **distinct** account order:

1. writable market
2. writable custody PDA
3. writable page-0 bid PDA
4. writable bid-owner sidecar
5. taker signer
6. writable taker base token account
7. writable taker quote token account
8. writable base vault
9. writable quote vault
10. vault-authority PDA
11. SPL Token program
12. up to 8 writable maker-balance PDAs for touched resting bid owners

The processor checks bid ordering and the bid-owner tag, validates every
maker-balance PDA and token mint/authority, transfers base from the taker into
the base vault, transfers quote from the vault to the taker using the
vault-authority PDA, and atomically updates bid quantities, owner sidecars,
custody totals and maker balances. Fully consumed bids are removed. Partial
fills release the difference of the old/new rounded-up quote reserve; quote
rounding dust is refunded to the maker's free quote balance.

## Remaining execution limits

- swaps support active-only buys and sells, not executable passive liquidity;
- swaps currently use page-0 asks (buy) or bids (sell) only;
- passive LP inventory accounting and mutable passive fills are not implemented yet;
- maker fan-out is bounded to 8 balance accounts;
- the fixed swap account set is still above the long-term Jupiter account-footprint target;
- passive settlement, multi-page mutable swaps and Jupiter adapter code remain follow-up work.

## Passive LP reserve accounting foundation

The `hybrid_state::passive` module introduces explicit bounded LP positions,
segregated passive base/quote reserves, fee balances, bidirectional checked
swap-reserve transitions, and vault coverage verification against active maker
collateral. Its arithmetic validates all values before committing changes, so
an underflow, overflow, or malformed position does not partially mutate the
in-memory accounting state.

**This is not live passive custody.** The accounting types are pure transition
primitives; they are not yet serialized into PDA accounts or wired to SPL Token
instructions. The protocol must authenticate LP owners and positions, model
per-range fee growth and tick crossing, bind liquidity to actual deposits, and
verify real vault balances before enabling executable passive swaps. Calling
these methods alone cannot authorize a withdrawal or a swap.

## Passive pool and funded LP position instructions

These instructions add real SPL Token deposits. They do not enable passive
swaps or withdrawals.

**Opcode 19: Initialize a passive pool.** Data: one byte (`19`).
Use these accounts in order: (1) market, (2) custody PDA,
(3) writable passive-pool PDA, (4) writable payer signer,
(5) system program. The pool PDA uses seeds
`["passive-pool", market]`. The market must have active collateral
enabled and valid custody.

**Opcode 20: Open and fund an LP position.** Data: 73 bytes.
Byte 0 is `20`. Bytes 1..9 contain a little-endian `u64` position
nonce. Bytes 9..25 and 25..41 contain the lower and upper Q64 sqrt
prices (`u128`). Bytes 41..57 contain liquidity (`u128`).
Bytes 57..65 and 65..73 contain the base and quote deposits (`u64`).

Use the accounts in this order: (1) market, (2) custody PDA,
(3) writable passive-pool PDA, (4) writable new position PDA,
(5) writable LP signer and payer, (6) writable LP base account,
(7) writable LP quote account, (8) writable base vault,
(9) writable quote vault, (10) base mint, (11) quote mint,
(12) SPL Token program, (13) system program.

The position PDA uses
`["passive-position", market, LP owner, nonce_le_bytes]`.
The program checks both pool and position addresses, the LP signer,
SPL Token mints, LP source ownership, and vault authority.
The program creates the position account and transfers the stated
principal to the existing vaults in one transaction. It updates
the passive pool's reserved base, reserved quote, and liquidity.

**Opcode 21: Close and redeem a passive position.** Data: 9 bytes.
Byte 0 is `21`; bytes 1..9 are the position nonce as a little-endian
`u64`. The accounts are: (1) market, (2) custody PDA,
(3) writable passive-pool PDA, (4) writable position PDA,
(5) writable LP signer, (6) writable base vault,
(7) writable quote vault, (8) writable LP base destination,
(9) writable LP quote destination, (10) base mint,
(11) quote mint, (12) vault-authority PDA, (13) SPL Token program.

The program checks both PDA seeds and the LP signature. It checks
that the vault balances cover all active maker collateral, passive
reserves, and accrued passive fees. It transfers the position's
original principal to the LP and invalidates the position account.
A second redemption fails.

**Important restriction:** The supplied liquidity value is declared,
not derived from a verified AMM formula. No swap reads this value
or spends passive vault balances. Redemption returns only deposited
principal; it does not distribute range fees or swap gains.
Do not use these development interfaces for production liquidity
until fee attribution, range accounting, and transaction tests
are complete.

### Vault coverage checks for LP token transfers

LP position deposits and redemptions validate SPL Token vault balances
against both active maker custody totals and passive LP reserves.
Each operation checks the vaults before the transfer. It checks the
balances again after the transfer. If a vault has insufficient assets,
the instruction fails and the whole transaction rolls back.

These checks do not make passive swaps executable. They do not replace
position-specific fee accounting, correct liquidity mint calculations,
or on-chain integration tests for malicious token-account inputs.

### Price-range collateral check for LP positions

An LP position must now contain enough tokens for its declared
liquidity at the market's current sqrt price. The engine calculates
minimum base and quote principal from the position's price limits.
The calculations use Q64 fixed-point arithmetic and round deposits
up. A position below the current price needs quote only. A position
above the current price needs base only. A position that contains the
current price needs both tokens.

The program rejects a position if the declared principal is less
than either minimum. It checks this condition before PDA creation
and token transfers.

This check does not replace exact position minting, tick accounting,
or earned-fee settlement. Deposits above the calculated minimum
remain reserved principal. Passive swaps remain disabled.

### LP account alias and rollback tests

The deposit and redemption instructions reject the same token
account in two different transfer roles. This prevents a source or
destination account from also serving as a custody vault.

The SBF program tests exercise principal redemption, an insolvent
vault, and an invalid destination that aliases a vault. The tests
check that a failed withdrawal does not change the LP position or
pool reserves. Passive swaps and earned-fee distribution remain
disabled.
