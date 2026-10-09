# Verified active book snapshots

The RPC loader fetches a market header, a page-zero order page, and its owner
sidecar using one getMultipleAccountsInfoAndContext request. It validates:
- A single RPC context slot, including a caller-required minimum slot.
- Hybrid program ownership for all three accounts.
- V1 state layout, price-time ordering, canonical cached prices, and owners.
- Header order count and exact sidecar binding.
- The restriction to standalone page zero for mutable active trading.

The caller must obtain the market, page PDA, and sidecar addresses from a
trusted source and verify page PDA derivation. The caller must also confirm
that the market account address belongs to the intended market and cluster.
This API does NOT derive a page PDA, verify a market address, or prove that
the RPC node is honest. A single slot is not sufficient for safe execution:
re-read and simulate immediately before submitting a trading transaction.

```js
import { loadActivePageSnapshot } from "./src/rpc.mjs";

const snapshot = await loadActivePageSnapshot(connection, {
  programId, market, page: askPagePda, ownerPage: askOwnerSidecar,
  side: "ask", minContextSlot: previousSlot,
});
// snapshot.orders contains owner bytes in canonical book order
```

No transactions are sent. Token custody and maker collateral are unchanged.
