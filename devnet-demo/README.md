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
3. **Cancel.** Obtain the confirmed sequence number from an independent
   decoder/indexer or a verified explorer. Enter sequence and matching
   side/page; the app builds opcode 31. It does not guess an order ID.
4. **Trade against the active book.** Configure base/quote vaults and
   wallet-owned SPL token accounts. List all maker public keys touched by
   the intended fill (up to 8) and select 1 or 2 initialized book pages.
   The app derives corresponding maker-balance PDAs, builds opcodes 23–26
   for active exact-in/out buy/sell, and enforces nonzero integer bounds.
   **A quote engine/indexer must select the touched maker set and slippage
   limits**; the harness does not compute a trusted quote.
5. **Verify balances.** Every mutation runs RPC simulation first. Only
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
\`\`\`

The verifier checks the devnet genesis hash, success receipt, distinct
chronologically ordered signatures, invoked program ID, decoded instruction
opcode, and the **same market address** in all three instructions. It prints
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
