# Reading Hybrid market state

`sdk/src/state.mjs` decodes **raw account bytes only**. It does not connect
to Solana RPC or establish that an account actually belongs to the Hybrid
program. Before parsing, the caller must verify account ownership, expected
PDA or sidecar identity, chain commitment, and a coherent slot snapshot.

```js
import { decodeMarketHeader, decodeOrderPage, decodeOwnerPage } from "./src/state.mjs";

// Account data and owner come from a trusted Solana RPC response.
// Confirm expected program ownership and account addresses before decoding.
const header = decodeMarketHeader(marketAccount.data);
const asks = decodeOrderPage(askPageAccount.data, "ask");
const ownedAsks = decodeOwnerPage(askOwnersAccount.data, asks);
```

All token quantities and Q64.64 prices are decoded as `bigint`. The decoder
checks version/magic, fixed state lengths, bounded page lengths, canonical
order sorting, zeroed trailing entries, and owner-sidecar alignment.

The caller must still verify on-chain PDA derivations, maker identity, and
cross-account consistency, including market ask/bid counts and snapshots
loaded at different RPC slots. The current executable swap ABI supports
only page 0, even though pure read-only pages may represent other indexes.

This is not an authoritative indexer, real-time order feed, oracle or
execution quote engine. Do not construct or submit transactions from a
stale or unverified snapshot.
