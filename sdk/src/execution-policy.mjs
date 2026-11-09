import {assertSimulatedBytes} from "./simulation-preflight.mjs";
// Non-signing maker execution admission policy.
// This module cannot send, simulate, approve or sign transactions. The actual
// transaction layer MUST use a durable journal and confirmed settlement before
// advancing from cancels to placements. Defaults fail closed.
export function admitMakerPhase({
  enabled=false,killSwitch=true,pendingMutation=true,
  snapshotSlot,currentSlot,maxLagSlots=4,
  cancellations=[],placements=[],maxChanges=16,
  simulationEvidence=null,transaction=null,operatorApproved=false,
}={}){
  if(!enabled||killSwitch)throw new Error("Maker trading is disabled by operator");
  if(pendingMutation)throw new Error("Pending mutation: reconcile signature journal first");
  if(!operatorApproved)throw new Error("Explicit operator approval required");
  if(!simulationEvidence || !transaction)throw new Error("Fresh transaction simulation required");
  assertSimulatedBytes(transaction,simulationEvidence);
  if(![snapshotSlot,currentSlot,maxLagSlots,maxChanges].every(
    x=>Number.isSafeInteger(x)&&x>=0
  )||snapshotSlot===0||currentSlot<snapshotSlot||
    currentSlot-snapshotSlot>maxLagSlots)throw new Error("Stale or invalid execution snapshot");
  if(simulationEvidence.snapshotSlot!==snapshotSlot||
      !Number.isSafeInteger(simulationEvidence.simulationSlot)||
      simulationEvidence.simulationSlot<currentSlot||
      simulationEvidence.signaturesVerifiedByRpc!==true){
    throw new Error("Simulation evidence does not match fresh snapshot");
  }
  if(!Array.isArray(cancellations)||!Array.isArray(placements)||
    cancellations.length+placements.length>maxChanges)
    throw new Error("Maker mutation cap exceeded");
  // Cancellation and placement MUST run as independent phases with a
  // newly refreshed, authenticated balance/book and new simulation.
  if(cancellations.length>0&&placements.length>0)
    throw new Error("Cancel and place require separate confirmed phases");
  const actions=cancellations.length?cancellations:placements;
  if(actions.length===0)return {phase:"noop",actions:[],slot:snapshotSlot};
  // Intentionally returns an advisory phase plan, not a submission permit.
  return {
    phase:cancellations.length?"cancel":"place",
    actions:[...actions],
    slot:snapshotSlot,
    requiresDurableJournalAndFreshState:true,
  };
}
