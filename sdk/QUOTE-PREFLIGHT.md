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
before it returns a phase *intent*. The policy now requires simulation
evidence bound to the exact serialized signed transaction bytes, a matching
snapshot slot and a non-stale RPC simulation slot. Operator approval remains
caller-provided; this module does not independently prove an approval or
persist a signature journal.

```js
import {admitMakerPhase} from "./src/execution-policy.mjs";
const phase=admitMakerPhase({
  enabled:false,               // read-only by default
  killSwitch:true,              // hard stop
  pendingMutation:true,         // fail closed unless journal reconciled
  operatorApproved:false,
  simulationEvidence:null,    // must bind exact signed bytes
  transaction:null,           // not a submission handle
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

## Read-only transaction-status RPC observer (SDK)

`sdk/src/signature-status.mjs` verifies the configured cluster genesis
hash before asking `getSignatureStatuses` for the exact recorded signature
with `searchTransactionHistory: true`. It also retrieves the finalized
block height and classifies the result as `finalized-success`,
`finalized-failure`, `pending`, or `unknown`. Only a structurally valid
`finalized` status with `err: null` is marked `canConfirm: true`.
A **null history result always returns `canExpire: false`**, even when the
blockhash is past its last valid height. A confirmed but not finalized
transaction cannot be treated as success. Unexpected RPC schemas or
incorrect cluster hashes throw and must leave the journal blocked.

```js
import {observeSignatureStatus} from "./src/signature-status.mjs";
const observation=await observeSignatureStatus(connection,{
  signature:recordedSignature,
  expectedGenesisHash:configuredGenesisHash,
  lastValidBlockHeight:journaledLastValidHeight,
});
// Only a finalized-success result is suitable for authenticated
// confirmation. The owner-side Rust journal must still verify signature
// bytes and persist the terminal state; never unlock on a null result.
```

This is **read-only** and provides no proof of complete historical absence,
no journal storage, no signed-transaction identity binding beyond the caller's
recorded signature, and no submission capability. A trusted RPC endpoint and
an independently validated cluster configuration remain requirements.

## Exact signed-transaction simulation preflight

`sdk/src/simulation-preflight.mjs` accepts an already-signed, serializable
Solana transaction and calls `simulateTransaction` with `sigVerify: true`,
`replaceRecentBlockhash: false`, `commitment: "confirmed"`, and a minimum
context slot. It rejects stale contexts, failed simulations, excessive compute
usage, and transaction mutations while the RPC call is pending. It returns a
SHA-256 fingerprint of the **exact serialized bytes**. The advisory execution
policy calls `assertSimulatedBytes` to reject any altered/substituted signed
transaction and requires snapshot/slot consistency.

The transaction must already be correctly constructed and signed by the
operator. **Simulation evidence is not authorization to broadcast.** The
preflight does not derive PDAs, prove program/account authenticity, enforce
blockhash validity, record signatures to durable storage, or submit a
transaction. A malicious caller can fabricate evidence: only an independently
controlled, trustworthy process combining the simulation RPC with atomic
journal persistence can supply execution authority. The later submission path
must recalculate the fingerprint and check finality, kill switch, risk budgets,
and process-level journal locking immediately before any attempted broadcast.
