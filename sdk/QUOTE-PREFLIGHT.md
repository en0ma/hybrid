# Maker quote-to-order preflight (non-signing)

The quote planner emits **decimal strings**. The new `sdk/src/quote-orders.mjs`
maps decimal prices to u128 Q64 using exact BigInt arithmetic. Bids round
down, asks round up; the cached sqrt value is canonical to the on-chain
`validate_limit_ask` convention. Base size must be a whole base lot,
and the sum of **new-order** collateral must fit authenticated *free*
base and quote inventory. Locked balances are not reusable.

```js
import {convertQuoteTargets} from "./src/quote-orders.mjs";
const targets=convertQuoteTargets({
  orders: observed.plan.orders,
  baseLotSizeAtoms: "100",
  freeBaseAtoms: observed.availableBaseAtoms,
  freeQuoteAtoms: observed.availableQuoteAtoms,
});
// targets.desired are checked Q64 targets; still NOT executable.
```

`sdk/src/execution-policy.mjs` is a separate fail-closed **advisory**
preflight guard. It defaults to kill switch on, no operator enablement,
pending mutation assumed and no simulation. Explicit allow flags, a
validated fresh RPC slot and phase-isolated cancels/placements are needed
before it returns a phase *intent*. Those flags are supplied by the caller;
this module does not prove a simulation, record an approval or persist a
signature journal.

```js
import {admitMakerPhase} from "./src/execution-policy.mjs";
const phase=admitMakerPhase({
  enabled:false,               // read-only by default
  killSwitch:true,              // hard stop
  pendingMutation:true,         // fail closed unless journal reconciled
  operatorApproved:false,
  simulationPassed:false,
  snapshotSlot:0,currentSlot:0,
  cancellations:[],placements:[],
});
```

The example above deliberately throws. Neither module signs, sends,
derives PDAs, verifies full account provenance or manages transaction
finality. They must never be treated as proof of permission to submit.

**Remaining work before any automatic trading:** canonical PDA derivation;
authenticated full-depth and maker inventory context; actual simulation
results tied to an exact signed transaction and blockhash; persistent
journaled prepared/submitted/confirmed/expired state (reuse existing Rust
`program/src/client_session.rs` machinery or equivalent); authoritative
signature expiration and post-trade reconciliation; manual opt-in, rate and
loss limits; and funded devnet soak/adversarial tests. For active bid
collateral, re-check exact on-chain rounding and page/lot constraints before
building any executable instruction.

Run: `cd sdk && npm test`.

## Durable pending-signature reconciliation (host-only Rust)

`program/src/client_recovery.rs` builds on the existing
`MakerSession` journal; it does **not** introduce a second file format or
transmit transactions. A pending journaled signature stays blocked while
pending, while RPC history is incomplete, on a failed finalized transaction,
or when a submitted signature does not match the observed one.

A successful finalized signature persists the confirmed slot before quoting
can resume; a provably absent signature only expires after the last valid
block height, and the caller must independently establish **complete**
historical absence on the correct cluster. A bare RPC `null` or timeout
is not proof of absence. A `Prepared` journal remains blocked until the
operator separately reconciles the signing/broadcast state. All successful
recovery paths require a fresh verified snapshot before a new mutation.

The recovery API accepts a **trusted observation from an external RPC
adapter**; it does not implement that adapter or prove the authority of its
finality/history assertion. No funded automatic submission is enabled.
