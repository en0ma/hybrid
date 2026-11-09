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

## Exclusive host lock and immutable signed-intent identity

`sdk/src/maker-journal-guard.mjs` adds two non-submitting primitives:

- `withMakerJournalLock(path, operation)` creates a 0600 lock via exclusive
  file creation, syncs the lock and parent directory, and holds it while the
  callback runs. A concurrent caller fails. A **thrown callback leaves the
  lock behind**, requiring manual recovery instead of an automatic steal
  based on PID, time, or age. Normal completion releases it.
- `createPreparedIdentity(path, ...)` creates (never overwrites) a
  fsynced JSON companion record tying the SHA-256 of the exact signed
  transaction bytes to a caller-provided signature, cluster genesis hash,
  snapshot slot, and last valid block height. `readPreparedIdentity` rejects
  missing/corrupt records, mismatched signature/cluster, transaction changes,
  and unrecognized fields.

**These APIs are not an integrated, atomic transaction journal.** The caller
must hold the lock across all storage transitions, use the existing Rust
`MakerSession` journal, reconcile partial storage failures and crash
windows, independently check the first signature in the wire transaction,
verify the configured cluster and chain blockhash, and never use a companion
identity record as proof of authorization. A successful lock callback is
only exclusive access, **not permission to submit or unlock a pending
signature**. Do not delete the immutable record or a stale lock unless a
trusted operator has conclusively reconciled the exact transaction with
finalized chain history. Account/key provenance, OS directory trust and
single-host filesystem semantics are mandatory preconditions. No funded
or unattended execution has been enabled.

## Cross-format pending journal consistency audit

`sdk/src/journal-audit.mjs` reads the existing Rust `HYBMMJ01`
104-byte journal (including its FNV checksum and reserved fields),
requiring the `Submitted` phase. It derives a Solana base58 signature
from the journal's raw 64 signature bytes and compares them against the
immutable signed intent, exact serialized wire transaction and configured
genesis hash. It also compares last-valid block height and ensures the
intent's snapshot slot is not older than the last confirmed slot.

`auditPendingMakerJournal(...)` returns `mode: "audit-only"`. Its
checks are necessary but **insufficient** for submission: the v1 Rust
journal does not store the originating prepared snapshot or the wire
fingerprint; the companion JSON is not atomically persisted with that
journal. Concurrent processes must hold a separate exclusive lock for
the entire audit and any follow-up transitions. Missing or corrupt
records fail closed. No callback here sends, signs, expires, confirms,
or clears a transaction; the independent trusted owner must still
verify finalized on-chain outcomes and reconcile every crash window.

## Experimental single-record journal (not a migration)

`sdk/src/maker-atomic-journal.mjs` introduces an isolated version-2
**experimental** journal for the *same* signed wire identity and submission
phase in **one** checksummed JSON record. New states use an exclusive
`.pending` creation, fsync, rename, and directory fsync. Unresolved
`.pending` files block further writes. It requires the decoded first
wire signature, transaction SHA-256, cluster identity, positive blockhash
validity height, and consistent snapshot slot. Replacing a prepared state
with submitted, and submitted with finalized or proven-expired, changes
the identity and phase **together** in one record. Tests cover restart,
mismatches, torn files, overlapping lock access, and premature expiry.

**Important:** This module is not connected to the existing Rust
`HYBMMJ01` journal or any live transaction sender; it does not migrate
existing pending transactions, perform RPC checks, or authenticate caller
claims of finality/absence. It must NOT be activated alongside the legacy
journal for the same maker without an audited, explicit, offline migration
and independent reconciliation of old pending signatures. All calls must
be made under `withMakerJournalLock` in a trusted single-host process;
the module itself does not acquire that lock, and a JSON checksum is an
integrity detector, not an authentication MAC. A failed write or a stale
lock always requires operator reconciliation. A `finalizedSlot` or
`authoritativeAbsent` boolean passed by a caller is **not verified here**.

**Unattended execution remains disabled.** The next architecture task is
a single trusted owner that authenticates chain observations, locks all
journal transitions, validates maker accounts and signatures, and never
broadcasts a transaction before a durable prepared/submitted record is
fully committed. This experimental v2 journal is a prototype for that
integration, not a completed production atomic transaction system.

## Finalized-status reconciliation for the experimental v2 journal

`sdk/src/maker-finality-reconcile.mjs` connects the read-only
`observeSignatureStatus` RPC observer to the isolated v2 journal. While
holding an exclusive journal lock, the owner can verify the journal's
stored signed-wire SHA-256, first wire signature, snapshot and expected
cluster, then request historical signature status with finalized commitment.
**Only finalized success** permits a durable transition from `submitted`
to `finalized`. Pending, unknown history, and finalized execution errors
leave the journal in `submitted` and return `canRetry: false`.
A successful transition also returns `canRetry: false` and explicitly
requires an independently verified refresh of maker inventory and book
before any new order planning.

The helper deliberately **never auto-expires** a transaction. It cannot
prove historical absence, is not the trusted journal-lock owner itself,
does not authenticate the RPC operator or the market accounts, and does
not sign, assemble, submit or rebroadcast orders. Never invoke it without
`withMakerJournalLock` spanning the entire RPC read and persistence
transition; the v2 journal must not coexist as an authority with the legacy
Rust v1 journal. This is a prototype recovery bridge, not production
approval to trade unattended.

## Read-only maker inventory audit after finalized execution

`sdk/src/maker-post-finality-audit.mjs` adds an independent, non-executable
post-finality state check. It requires the experimental v2 journal to be
`finalized` and validates the expected genesis hash. Its one
`getMultipleAccountsInfoAndContext` request fetches **all six** market,
ask-page, ask-owner, bid-page, bid-owner, and maker-balance accounts at
`commitment: "finalized"` and with `minContextSlot` set to the journaled
finalization slot. Existing SDK decoders authenticate market/page program
ownership, sidecar bindings, maker/market ownership in the balance record
and page formats. The audit rejects stale or mixed context slots, missing
accounts, duplicate account keys, or incorrect program owners. It returns
read-only available and locked collateral, order counts and the verified
observation slot, with `canSubmit: false`.

This helps ensure a maker cannot treat **transaction finalization alone**
as proof of a fresh verified trading state. The caller still must derive
and verify all canonical PDAs and the market's sidecar addresses, trust
the RPC endpoint, check fill accounting/risk exposure and capacity,
and re-read and re-simulate an exact transaction under exclusive journal
ownership before any attempt to trade. No tokens are moved and no
order-submission permit is produced. Never activate the experimental v2
journal alongside the Rust v1 journal for the same maker.

## Independent collateral and resting-order reconciliation

The finalized six-account audit now returns `ownedOrders` containing the
maker's resting order side, sequence, Q64 price and remaining base size
from authenticated owner sidecars. The SDK's
`reconcileMakerInventory({before,after,expectedAssetDeltas,expectedOwnedOrders})`
compares **all four** maker ledger fields (free/locked base and quote)
against explicitly provided signed integer deltas, and compares the exact
maker resting-order identity/price/remaining-size set against an externally
verified expected result. Missing expected deltas, duplicated orders,
stale/non-finalized snapshots, malformed quantities or unexplained
changes fail closed. The result always has `canSubmit: false`.

**Crucial limitation:** these expected deltas and orders must be authenticated
independently from finalized transaction logs, SPL custody accounting and
authorized transfers. This module **does not fetch or validate those
receipts**, model fill fees, establish asset conservation on its own, or
permit trades. A caller cannot claim successful independent reconciliation
by passing unverified expected data derived from the same after snapshot.
All account PDAs and context must already be independently authenticated.

## Research: frozen-epoch pro-rata fills and delayed settlement

`sdk/src/pro-rata-epoch.mjs` is a **pure, non-executing prototype**
for proportional maker allocation without a write to each maker on every
fill. A tick/price step has its own **immutable frozen epoch**: maker
identities, weights, six integer base/quote debit/credit/fee totals and
an epoch commitment. Settlement MUST NOT begin until all fill totals for
that epoch are final. Tick movements begin a different epoch, retaining
the old checkpoint unchanged for makers who settle much later.

For one amount `T`, maker weight `w`, preceding prefix `p`, and
frozen total weight `W`, allocation is
`floor(T * (p + w) / W) - floor(T * p / W)`.
The integer floors telescope across all makers, so exactly `T` atoms
are assigned: none are created, destroyed, or left in a global dust bucket.
A lexically ordered roster is rotated by epoch ID to vary residual
priority. Base debits/credits, quote debits/credits and fee claims each
use their own independently fixed total. Tests cover uneven weights,
dust, tick changes, deferred settlement and 137 makers in reversed
settlement order.

**Important correction:** this formula is *not generally monotone*
as `T` increases. A maker's cumulative allocation may decrease while
an epoch is still filling (e.g. five equal makers, third maker's
allocation for `T=2` is one atom, but zero for `T=3`).
Therefore a maker MUST NOT settle against a live/changing epoch.
Freeze an epoch **before** permitting claims; its maker weights, total
fees, price and fill totals may never be revised after first claim.
If partial fill progress must be claimable sooner, create new immutable
subepochs with separately frozen totals and immutable membership.

The synthetic claim ledger rejects duplicate claims in one in-memory
state transition, but **does not** verify a maker signature, enforce
on-chain atomic ledger-plus-SPL transfers, prove real fill and fee totals,
authenticate PDAs/escrow, enforce vault solvency, or prevent replay
between independently persisted ledger copies. A production processor
needs an authenticated epoch commitment, bounded storage/proof costs,
correct per-price trade pricing, checked reserve accounting, atomic
claim checkpoint plus transfers and an independent conservation audit.
Do not activate for funded matching or replace eager maker settlement.
