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
