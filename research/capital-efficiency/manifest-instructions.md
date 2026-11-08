## Single-maker Manifest instruction construction

`manifest-instructions.mjs` now calls the **actual official Manifest SDK methods** `depositAtomsIx` (exact integer-atom funding) and `placeOrderIx` (wrapper limit ask). This deliberately returns **unsigned instructions only**, not fabricated transaction results or a completed execution adapter.

The SDK's order wrapper accepts `numBaseTokens` and `tokenPrice` as JS numbers. Until an atom-exact order builder is reviewed, this helper accepts only **whole base-token quantities** and positive integer token prices, rejecting fractional amounts that could introduce silent number rounding. These restrictions are suitable for an initial controlled benchmark but are not production quote sizing.

Example after loading a real, pre-provisioned ManifestClient:

```js
import {OrderType} from "@bonasa-tech/manifest-sdk";
import {makeManifestSingleMakerAsk} from "./manifest-instructions.mjs";
const planned=makeManifestSingleMakerAsk({
 client, maker: trader.publicKey,
 baseAtoms:"1000000", // for a 6-decimal base mint: one base token
 priceTokens:100, clientOrderId:1, OrderType
});
// Independently verify funding/mints, then sign and send each instruction
// using the correct wrapper and owner signing accounts. Do not treat this
// object as an executed measurement.
```

**Manifest's wrapper is a separate program from Manifest core.** Successful wrapper order transactions can invoke core through CPI; the existing direct-core-invocation receipt gate intentionally will not recognize CPI-only execution. To benchmark accurately, require verification of the expected *wrapper* top-level instruction as well as decoded inner core invocation and market state transitions. The contract should separately record the core and wrapper program IDs and pinned deployed binaries.

No signing, account provisioning, swap or finality receipt collection is implemented in this module.
