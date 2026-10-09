# Hybrid alpha instruction SDK

This is a dependency-free, ESM library for constructing Hybrid instruction
descriptors. Each result contains `programId`, `keys`, and a `Buffer` called
`data`. To use Solana web3.js, pass it to `new TransactionInstruction(result)`.

The caller must supply correct, verified PublicKey values and derive all PDAs.
The SDK does **not** sign transactions, perform price discovery, calculate
slippage, inspect balances, select makers, or verify the on-chain state.
The ABI is alpha and subject to incompatible changes. Do not blindly submit
transactions with keys supplied by untrusted parties.

```js
import { TransactionInstruction } from "@solana/web3.js";
import { swap } from "./src/instructions.mjs";

const descriptor = swap(programId, {
  market, custody, page: askPage, ownerPage: askOwners,
  taker, takerQuote, takerBase, quoteVault, baseVault,
  vaultAuthority, tokenProgram, makerBalances: [makerBalance],
}, { side: "buy", mode: "exact-in", amount: 1000n, limit: 900n });
const instruction = new TransactionInstruction(descriptor);
```

Use `BigInt` for token amounts and fixed-point values to prevent precision loss.

Supported builders: place/cancel ask and bid (page 0, collateralized ABI),
four active swap modes, maker collateral deposit/withdraw, maker balance
initialization, passive pool initialization, LP deposit and principal redemption.

`npm --prefix sdk test` runs ABI layout and negative-input tests using Node's
built-in test runner. This SDK alone is **not** sufficient for production trading:
quote correctness, account discovery, market data, execution simulation,
transactions and signing must be provided by integrating clients.
