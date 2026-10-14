# Surfpool local lifecycle testing

Surfpool is the preferred local RPC runtime for exercising the existing
Hybrid Rust program and the wallet harness. This guide configures the
**client tooling**, not a pre-funded fixture. A successful six-stage lifecycle
requires deployment and market provisioning separately.

## 1. Start Surfpool

Install Surfpool following the official Solana docs. In a separate terminal:

```sh
surfpool start --offline --no-tui
```

The default local JSON-RPC URL is `http://127.0.0.1:8899`.
Surfpool can auto-deploy projects with a deployment Runbook, but **confirm**
that the Hybrid SBF binary is deployed at the compiled program ID
`US517G5965aydkZ46HS38QLi7UQiSojurfbQfKCELFx`.
Do not assume Surfpool automatically initialized the Hybrid market,
pages, custody, token mints, vault authority, or owner sidecars.

## 2. Configure the wallet harness

```sh
cd devnet-demo
npm install
npm test
npm run check:runtime   # fails until Hybrid's compiled program ID is deployed
npm run dev
```

In the browser, select **Surfpool (local)**, use
`http://127.0.0.1:8899`, and connect a wallet that supports this local
RPC. The wallet's selected network must match Surfpool; browser and
wallet must both be able to reach loopback. Never use mainnet wallets or
real assets in this harness.

Initialize a collateralized market and the canonical linked book accounts
with an authorized market signer. Then, in order, run maker PDA init,
deposit, place, swap with a funded taker, cancel remaining order quantity,
and withdraw. Record confirmed signatures and before/after program states.

## 3. Readiness and receipts

```sh
export HYBRID_CLUSTER=surfpool
export SOLANA_RPC_URL=http://127.0.0.1:8899
npm run check:readiness -- <MARKET> <ASK_OWNER> <BID_OWNER> <BASE_VAULT> <QUOTE_VAULT>
npm run verify:receipts -- <INIT_SIG> <DEPOSIT_SIG> <PLACE_SIG> <TRADE_SIG> <CANCEL_SIG> <WITHDRAW_SIG>
```

Both commands accept an explicit local Surfpool RPC rather than demanding
Solana's public devnet genesis hash. The readiness checker requires
initialized custody, account ownership and market commitments, SPL mints,
and vault reserves. The receipt checker requires valid signatures and
consistent program, market, maker, side, and supported transaction effects.

**Important:** Receipt checks do not establish exact order fill, order
sequence identity, maker collateral conservation or post-cancel inventory.
A full integration claim requires deterministic fixture provisioning plus
decoded state assertions before and after every instruction. Issue #60
tracks that remaining work. Surfpool provides realistic transaction
execution but is not a substitute for eventual public-cluster deployment
compatibility checks.

For the existing public devnet mode, leave `HYBRID_CLUSTER=devnet` and
use an HTTPS devnet RPC; only that mode enforces devnet genesis identity.

## Capture maker collateral state before and after each transaction

Run `npm run snapshot:maker -- <MARKET> <MAKER>` with `HYBRID_CLUSTER=surfpool` and `SOLANA_RPC_URL=http://127.0.0.1:8899`. The command decodes the actual Rust market header, custody totals, and maker balance (`freeBase`, `lockedBase`, `freeQuote`, `lockedQuote`) at a reported RPC context slot. Capture JSON output before/after init, deposit, placement, swap, cancellation and withdrawal; retain actual confirmed signatures. Snapshots do **not** decode individual orders or prove full conservation on their own.

## Assert exact maker collateral changes

After obtaining `before.json` and `after.json` with `snapshot:maker` around a successful deposit or withdrawal, run `npm run check:transition -- before.json after.json deposit base 1000` (or `withdraw quote 1000`). This command rejects changes to the other three maker collateral fields, including locked funds, and verifies the selected free balance changed by exactly the amount in raw atoms. This does **not** check the SPL wallet/vault token transfers or validate order-fill accounting. Only compare snapshots obtained from the same runtime and market.

The snapshot command also records raw base/quote vault token amounts. The transition assertion requires the correct vault to increase/decrease by the exact deposit/withdraw amount, and the other vault to remain unchanged. This verifies one maker plus corresponding vault deltas, not market-wide conservation across all makers and orders.

The transition checker also requires custody `totalBase`/`totalQuote` accounting to move by exactly the selected amount, an unchanged opposite custody total, matching RPC identity, and nondecreasing snapshot slots. These are three-way maker/custody/vault checks, not an atomic guarantee: take snapshots immediately around isolated transactions, and confirm the signature separately.

For isolated maker order placement or cancellation, use `npm run check:transition -- before.json after.json place ask` or `cancel ask` (for bid orders use `bid`). These checks require an exact free-to-locked collateral transfer in the committed asset, no opposite-asset movement, and stable custody/vault amounts. Cancellation must release nonzero locked collateral, so a fully filled order with nothing left to cancel is not a valid cancellation-stage fixture.

For stronger checking, pass an **expected raw lock/unlock amount** as the optional last argument, e.g. `npm run check:transition -- before.json after.json place ask 600` or `cancel bid 120`. An ask places exactly its base quantity in locked base; a bid locks `quote_for_base_at_price(base_qty, price_x64)` according to the Rust engine's integer rounding. On cancellation use the **remaining** on-book quantity at the original price (after any fills), not the original placed size. This option independently checks the expected value against the observed maker delta; it is not itself a proof of a correct fill or the identity of the canceled sequence. Derive the expected bid amount from Rust math / trusted decoded order data, not a manually guessed quote value.

## Market-header invariants for isolated order transitions

The `place` and `cancel` transition checker now also requires both snapshots' decoded market headers. On isolated placement the selected side's order count must rise by one and `nextSequence` rise by one; on cancellation the selected count falls by one and `nextSequence` remains unchanged. The opposite side's count must remain unchanged. These rules reflect the linked-maker Rust handlers. They reject certain unrelated or mismatched state changes but **do not** prove the actual sequence was inserted/canceled. A future book-entry/owner-sidecar decoder and confirmed transaction instruction linkage are needed for that claim. Run snapshots immediately around one isolated transaction; aggregate header deltas cannot identify concurrent same-slot operations.

## Expected sequence checks

The transition CLI also supports `place|cancel ask|bid [expected-locked-atoms] [expected-sequence]`. In a single isolated placement, the new order's sequence equals the **before** header's `nextSequence`. In cancellation, the canceled sequence must be positive and less than the current `nextSequence`; cancellation itself does not increment it. Example: `npm run check:transition -- before.json after.json place ask 100 7`. These are **sequence plausibility/allocator checks only**: a sequence below `nextSequence` might already have been filled or canceled, and no book membership/ownership proof is implied. A decoded book/sidecar check remains necessary.

## Decode order ownership in snapshots

The snapshot tool now reads two ask book pages and their owner sidecars, and optionally the bid pages when `HYBRID_BID_OWNER0` is set to the full bid page-0 owner sidecar pubkey. The market header commits only a prefix of the bid sidecar address, so it cannot be reconstructed from the header alone. The reader rejects bad page sizes, owner/page link mismatches, duplicate sequences, invalid ordering, and market count disagreements. It exposes `bookOrders` and a separate `bookSlot` (the book RPC batch may be from a different slot). When both transition snapshots include decoded orders, an expected sequence additionally verifies the referenced order was inserted or removed under the supplied maker identity; ask cancellation also verifies its remaining base quantity matches unlocked base. Missing optional bid snapshots do not give bid identity proof. A confirmed receipt and slot-consistent lifecycle trace are still necessary.

## Consistent snapshot and bid quote obligation

The snapshot command now discovers vault and sidecar addresses first, then reads the market, custody, maker balance, both SPL vaults, and all selected linked book pages/sidecars together via one `getMultipleAccountsInfoAndContext` call. It verifies the final custody vault addresses and market sidecar commitments agree with discovery. `slot` and `bookSlot` therefore refer to the **same RPC response**. This avoids the earlier cross-batch slot ambiguity but cannot establish transaction-signature linkage or prevent operations between the preliminary discovery reads and the final account batch; incompatible changes are rejected.

When a decoded expected order is supplied, both asks and bids must match exact collateral movements. The bid obligation is `ceil(baseQty * priceX64 / 2^64)`, matching Rust's `quote_for_base_at_price` semantics for valid inputs. The checker rejects a mismatched order price/quantity or a book slot different from the collateral snapshot slot. The linked-order verifier still requires isolation from unrelated writes and a confirmed receipt for proof of the actual signed instruction.

## Missing sidecar / order evidence

A snapshot with an incomplete `bookOrders` object cannot support a claim about an order on the omitted side. When an expected sequence is supplied and book evidence is included, the transition checker fails closed if either snapshot lacks the selected side's decoded array, instead of silently returning `orderIdentityChecked: false`. Set `HYBRID_BID_OWNER0` to the authenticated full bid head-sidecar address when validating bid identities. Older snapshots without any decoded book data can still provide limited collateral/header checks, but must not be characterized as per-order proof.

## Mandatory full-book evidence for funded lifecycle

For the Issue #60 funded fixture, set `HYBRID_REQUIRE_FULL_BOOK=1` and `HYBRID_BID_OWNER0=<FULL_BID_HEAD_SIDECAR_PUBKEY>` before `snapshot:maker`. Missing the bid sidecar is a hard error when the market has active bids or full-book mode is requested. The JSON `fullBookVerified` field is true only if both ask and bid books were decoded and their counts matched the market header. The full-book flag validates visibility and account structure, **not** execution, solvency, or signatures. For read-only ask-only smoke on an empty bid book, omission of the bid side is still permitted unless full-book mode is requested.

## Readiness mint validation

`check:readiness` validates the classic SPL Token mint's 82-byte layout with decimal precision at byte offset 44 and the initialized flag at byte offset 45. It rejects mismatched decimals and uninitialized mints; the former incorrect comparison against bytes 45/46 has been fixed. This checks mint layout correctness, not the availability of funded maker/taker token accounts.
