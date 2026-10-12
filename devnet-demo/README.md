# Devnet wallet-to-trade integration harness (milestone 59)

This browser-based developer smoke harness exercises the *real* Hybrid
on-chain instruction ABI, not a mock matcher. It does **not** provision a
market, mint tokens, host an indexer, or deploy any program. It only sends
transactions when a compatible browser wallet explicitly approves them.

## Launch locally

Prerequisites: Node.js 20+, a Solana browser wallet configured for devnet,
devnet SOL for transaction fees, and a **previously deployed and initialized**
Hybrid market with SPL-funded maker collateral, initialized linked order-book
pages, owner sidecars, custody and vaults.

```sh
cd devnet-demo
npm install
npm test
npm run build
npm run dev
```

Open the local Vite URL (127.0.0.1) and connect the wallet. Set the devnet
RPC URL and the actual market, account and SPL token addresses. The harness
checks the Solana devnet genesis hash and refuses mainnet/testnet RPCs.
The program ID is pinned to the `declare_id!` in `program/src/lib.rs`;
if a different test deployment has a different program ID, update that
constant and recompile rather than sending instructions to an arbitrary ID.

**Network requirements:** The wallet and browser must reach the RPC, and
the market must already have been initialized against the program ID.
Public RPC may enforce rate limits.

## Generate account manifest

The market public key is sufficient to derive the program custody and
linked-book PDAs, without any RPC access or signing:

```sh
npm run print:manifest -- <MARKET_PUBKEY> [MAKER_PUBKEY]
```

The page-0 owner sidecars are **externally created and bound accounts**;
they cannot safely be guessed from a PDA seed. Obtain them from the
initialization operator or authenticated market state. The base/quote vaults
and mints must also be supplied from the actual funded deployment.

## Read-only market readiness probe

Before opening the browser, verify that the configured market, book pages,
owner sidecars, custody and vault accounts exist on the **actual devnet RPC**,
and that the pinned Hybrid program is executable. This requires no wallet
or private key:

```sh
cd devnet-demo
npm run check:readiness -- <MARKET> <ASK_OWNER> <BID_OWNER> <BASE_VAULT> <QUOTE_VAULT>
```

The command rejects non-devnet RPC endpoints and returns a nonzero exit code
if a required account is missing or unexpectedly owned, or if the decoded\n192-byte custody record has invalid magic/version or mismatches the supplied\nmarket and SPL vault public keys. It reports custody mint addresses and\nrecorded active collateral totals as raw atom strings. It does **not**
prove that the market is funded, linked pages are initialized correctly,
or that swap collateral and liquidity are sufficient.

## Transaction smoke flow

1. **Refresh snapshot.** The app requests multiple Hybrid market/book/
   sidecar/custody accounts in one RPC call, reporting the context slot and
   account owners/sizes, plus token account balances. Inspect for missing
   or wrongly owned accounts. This is raw state inspection; it does not
   yet decode or display a price-time sorted book.
2. **Place an order.** Fund the maker's on-chain free collateral through
   the established protocol deposit path *before* using this harness.
   Supply page index 0 or 1, side, nonzero raw base quantity, Q64 price,
   and Q64 square-root price. The app derives canonical page PDAs and
   maker-balance PDA and builds opcode 30.
3. **Deposit collateral / withdraw unused collateral.** With custody initialized by the market operator, initialize the maker's
   balance PDA (opcode 12) with the new button, then use collateral actions, supply market base/quote SPL
   mint addresses, select the asset and enter a nonzero raw atom amount.
   Opcode 13 transfers funds from the wallet's source token account into
   the market vault and credits the maker's free collateral; opcode 14
   returns **only free collateral** to the wallet. Both simulate before
   wallet approval and report confirmed before/after token balances.
4. **Cancel.** Obtain the confirmed sequence number from an independent
   decoder/indexer or a verified explorer. Enter sequence and matching
   side/page; the app builds opcode 31. It does not guess an order ID.
5. **Trade against the active book.** Configure base/quote vaults and
   wallet-owned SPL token accounts. List all maker public keys touched by
   the intended fill (up to 8) and select 1 or 2 initialized book pages.
   The app derives corresponding maker-balance PDAs, builds opcodes 23–26
   for active exact-in/out buy/sell, and enforces nonzero integer bounds.
   **A quote engine/indexer must select the touched maker set and slippage
   limits**; the harness does not compute a trusted quote.
6. **Verify balances.** Every mutation runs RPC simulation first. Only
   successful simulation is presented to the wallet for signing. After
   the signed transaction is confirmed, the app displays the signature,
   explorer link, pre/post wallet SPL token balances and simulation CU.
   Refresh state before making another decision.

The on-chain program may reject orders whose collateral, tick/price
relationship, page-link state, or maker set is invalid. Such failures
are expected safety checks and **not** evidence of a successful smoke test.

## Security and operating limitations

- **No mainnet support:** explicit devnet-genesis gate; no production wallet
  approval should be given to arbitrary RPC servers or unknown markets.
- **No privileged key custody:** only the injected wallet signs; no
  private key, secret phrase or automated signing is stored.
- **No automatic retry:** an uncertain submitted signature must be
  inspected before creating another transaction.
- **No liquidity fabrication:** account initialization, funding and
  deployment are prerequisites, not silently performed by the UI.
- **No quote oracle:** user-supplied price, size, maker set, and limit are
  untrusted. Compare a fresh authenticated book snapshot and simulate.
- **No full indexer/UI:** the snapshot displays basic account existence,
  ownership, data size, RPC slot, and balances, not decoded order entries.
- **No passive swap buttons:** the smoke flow exercises active maker and
  linked-book swaps only, leaving LP interactions to audited workflows.
- **No on-chain devnet success claimed:** CI runs ABI unit tests and
  bundles the UI; it does **not** send real devnet transactions or prove
  a live market exists.

## Verify actual devnet transaction receipts

After making three **real** successful transactions against a configured
market, verify that the transactions are confirmed and contain the expected
Hybrid place (30), trade (23–26), and cancel (31) opcodes:

\`\`\`sh
cd devnet-demo
npm run verify:receipts -- <PLACE_SIGNATURE> <TRADE_SIGNATURE> <CANCEL_SIGNATURE>

# Six-stage maker lifecycle with real devnet signatures:
npm run verify:receipts -- <INIT_MAKER_SIG> <DEPOSIT_SIG> <PLACE_SIG> <TRADE_SIG> <CANCEL_SIG> <WITHDRAW_SIG>
\`\`\`

Both modes check the devnet genesis hash, success receipt, distinct
chronologically ordered signatures, invoked program ID, decoded instruction
opcode, the **same market address** in all three instructions, and the **same maker\npublic key** in the place and cancel instruction account lists. It prints
slot, fees and pre/post SPL token balance entries for inspection. The trade
receipt additionally requires both a positive and negative SPL token-account
balance delta and prints signed atom deltas. It does
not certify that the place/trade/cancel targeted the same maker order or
conserved every liquidity claim; inspect decoded market, maker balances,
vaults and state transitions before milestone sign-off.

Before marking milestone 59 complete, add a scripted/devnet fixture that
deploys and initializes a reproducible market, funds maker balances and
vaults, places an order, executes a buy/sell, cancels a remaining order,
and archives transaction signatures and before/after token balances.
Require security audit of the deployed build and market parameters.

## Custody initialization boundary

Opcode 11 requires the **market account to sign**, in addition to a funded payer, valid mints, and existing token vaults. A normal connected maker wallet cannot sign as that market account. Custody initialization therefore remains an operator deployment prerequisite. The frontend now supports connected maker balance initialization (12), deposits (13), withdrawals (14), order placement/cancellation (30/31), and active swaps (23–26). Never provide a deployment keypair to the browser UI.

The six-signature form additionally checks initialization (12), deposit (13), and withdrawal (14) instructions with the same authenticated maker. Neither form proves that a specific order sequence was filled/canceled, nor verifies custody reserves without a decoded state audit.

For three- and six-receipt evidence, the trade instruction must also include the placing maker's canonical `maker-balance` PDA among its touched maker accounts. This establishes account linkage, **not** proof that the maker order actually filled; order sequence and collateral balance deltas still require state decoding.

The readiness check also decodes the Rust 128-byte market header, requires collateralized-active mode and nonzero lot/sequence fields, and compares the committed ask-owner sidecar with the operator-supplied sidecar. This is a market-level prerequisite, not proof of correct linked book pages or bid-owner tags.
