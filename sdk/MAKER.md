# Maker order reconciliation

`reconcileMakerOrders` compares **only orders owned by one maker** against
that maker's desired page-zero quotes. It generates an ordered intent:
cancellations first, placements second, leaving identical orders unchanged.

```js
import { reconcileMakerOrders, buildMakerReconciliation } from "./src/maker.mjs";

const plan = reconcileMakerOrders({
  existing: myAuthenticatedOrders,
  desired: myNewQuotes,
  maxChanges: 8,
});
const descriptors = buildMakerReconciliation(programId, {
  ask: askAccountRoles,
  bid: bidAccountRoles,
}, plan);
```

The caller must authenticate ownership from the current on-chain owner sidecars,
reload the market immediately before execution, verify available collateral,
check page capacity, simulate the full transaction, and handle stale order
sequences. A plan is not an instruction to submit all operations blindly.
It is not an autonomous market-maker, order indexer, oracle, or transaction
executor. A cancellation and replacement may fail atomically if collateral or
page capacity changes. `maxChanges` limits generated operations but does not
guarantee that Solana transaction size or compute budgets will fit.
