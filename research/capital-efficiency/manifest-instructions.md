# Manifest single-maker transaction construction

`manifest-instructions.mjs` constructs **actual Manifest SDK** instructions for a maker base-token deposit, limit ask placement and taker swap. It uses `ManifestClient.depositAtomsIx`, `ManifestClient.placeOrderIx`, `ManifestClient.swapIx` and `OrderType.Limit` as defined in the upstream Manifest TypeScript SDK.

The function accepts a pre-initialized ManifestClient whose wrapper/seat setup has completed. No program deployment or wallet setup is hidden in the bridge. **It does not sign, broadcast, confirm, or generate receipt evidence.**

```js
import { ManifestClient, OrderType } from "@bonasa-tech/manifest-sdk";
import { buildManifestMakerInstructions } from "./manifest-instructions.mjs";
const client = await ManifestClient.getClientForMarket(connection, marketAddress, makerKeypair);
const plan = buildManifestMakerInstructions({
  client, sdk:{OrderType}, maker:makerKeypair.publicKey,
  baseAtoms:"1000000", priceTokens:100, clientOrderId:1
});
// plan.instructions.deposit and plan.instructions.place are genuine unsigned SDK instructions.
// Send and independently confirm each with actual funded test-network wallets; verify final market state.
```

**SDK precision constraint:** `depositAtomsIx` is integer-based, but `placeOrderIx` takes a JavaScript-number amount and price. We reject base atom sizes outside its safe integer range or quantities that fail a round-trip check; price conversion and final quantization must also be audited against the resulting on-chain price. Restrict benchmark prices to exactly matching post-placement price levels. This bridge is **not** a claim of parity at arbitrary large order sizes.

The remaining steps are to fund a pinned, isolated Manifest environment, run signed instructions with market-level state assertions, independently verify finalized receipts, and map a full single-maker scenario into `run-workload.mjs`. No actual Manifest trade has been run by these tests.
