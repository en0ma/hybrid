// Read-only RPC-to-journal reconciliation for the isolated v2 maker journal.
// This module never signs, constructs or broadcasts an order, and never infers
// proof of absence from a null RPC result. Caller MUST hold the exclusive
// journal lock, use a trusted RPC, and verify post-finalization account state.
import {readAtomicMakerJournal,advanceAtomicMakerJournal} from "./maker-atomic-journal.mjs";
import {observeSignatureStatus} from "./signature-status.mjs";
import {assertSimulatedBytes} from "./simulation-preflight.mjs";
import {assertFirstWireSignature} from "./maker-journal-guard.mjs";

/** If a submitted transaction finalized successfully, durably persist the
 * exact matched transaction's finalized slot. Failure or uncertainty leaves
 * the existing Submitted journal untouched, preventing unverified retry.
 * Do not call without the exclusive journal lock.
 */
export async function reconcileFinalizedAtomicMakerJournal(connection,{
  journalPath,transaction,simulationEvidence,expectedGenesisHash,
}){
  const record=await readAtomicMakerJournal(journalPath);
  if(record.phase!=="submitted")
    throw new Error("Journal is not awaiting a submitted transaction");
  if(record.genesisHash!==expectedGenesisHash)
    throw new Error("Maker journal cluster mismatch");
  assertSimulatedBytes(transaction,simulationEvidence);
  assertFirstWireSignature(transaction,record.signature);
  if(record.transactionSha256!==simulationEvidence.transactionSha256||
      record.snapshotSlot!==simulationEvidence.snapshotSlot)
    throw new Error("Journal signed transaction identity mismatch");

  const observation=await observeSignatureStatus(connection,{
    signature:record.signature,
    expectedGenesisHash:record.genesisHash,
    lastValidBlockHeight:record.lastValidBlockHeight,
  });
  if(observation.state==="unknown"||observation.state==="pending")
    return Object.freeze({state:observation.state,changed:false,canRetry:false});
  if(observation.state==="finalized-failure")
    return Object.freeze({state:"finalized-failure",changed:false,canRetry:false});
  if(observation.state!=="finalized-success"||observation.canConfirm!==true)
    throw new Error("Untrusted transaction finality status");
  // The finalized slot must not precede the authenticated snapshot.
  if(observation.slot<record.snapshotSlot)
    throw new Error("Finalized slot older than the submitted snapshot");
  const next=await advanceAtomicMakerJournal(journalPath,{
    from:"submitted",to:"finalized",
    signature:record.signature,genesisHash:record.genesisHash,
    transaction,simulationEvidence,finalizedSlot:observation.slot,
  });
  return Object.freeze({
    state:"finalized-success",changed:true,finalizedSlot:next.lastConfirmedSlot,
    canRetry:false,requiresFreshMakerState:true,
  });
}
