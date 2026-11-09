// Read-only transaction simulation evidence for a PRE-SIGNED Solana transaction.
// No signer, keypair, transaction construction or send function is provided.
// The RPC node must verify signatures against the unchanged recent blockhash.
import {createHash} from "node:crypto";

const slotOk=n=>Number.isSafeInteger(n)&&n>0;
function transactionBytes(transaction){
  if(!transaction || typeof transaction.serialize!=="function") {
    throw new TypeError("A pre-signed serializable transaction is required");
  }
  // No mutation, signing, or blockhash replacement is performed here.
  const bytes=transaction.serialize();
  if(!(bytes instanceof Uint8Array)||bytes.length===0||bytes.length>1232) {
    throw new RangeError("Invalid serialized transaction length");
  }
  return Buffer.from(bytes);
}
function digest(bytes){
  return createHash("sha256").update(bytes).digest("hex");
}

/** Simulate the EXACT signed bytes against a fresh confirmed RPC context.
 * This is advisory evidence only, never an authorization to broadcast.
 * Caller must authenticate all referenced accounts, signer, program,
 * collateral, blockhash validity, and enforce its separate durable journal.
 */
export async function simulateMakerTransaction(connection,{
  transaction,minContextSlot,currentSlot,maxLagSlots=4,maxUnits=200_000,
}){
  if(!connection||typeof connection.simulateTransaction!=="function"){
    throw new TypeError("RPC simulation method required");
  }
  if(!slotOk(minContextSlot)||!slotOk(currentSlot)||
      currentSlot<minContextSlot||!Number.isSafeInteger(maxLagSlots)||
      maxLagSlots<0||currentSlot-minContextSlot>maxLagSlots){
    throw new Error("Stale or invalid transaction simulation snapshot");
  }
  if(!Number.isSafeInteger(maxUnits)||maxUnits<=0||maxUnits>1_400_000){
    throw new RangeError("Invalid compute-unit budget");
  }
  // Capture before/after to reject client-side object mutation while RPC
  // simulation is pending. Important: callers must also bind the same bytes
  // when preparing the durable transaction journal and before broadcasting.
  const before=transactionBytes(transaction);
  const beforeDigest=digest(before);
  const result=await connection.simulateTransaction(transaction,{
    commitment:"confirmed",
    minContextSlot,
    sigVerify:true,
    replaceRecentBlockhash:false,
  });
  const after=transactionBytes(transaction);
  if(!before.equals(after)){
    throw new Error("Transaction bytes changed during simulation");
  }
  if(!slotOk(result?.context?.slot)||
      result.context.slot<minContextSlot||
      result.context.slot<currentSlot){
    throw new Error("Simulation returned stale or invalid RPC context");
  }
  if(!result.value||result.value.err!==null||
      !Number.isSafeInteger(result.value.unitsConsumed)||
      result.value.unitsConsumed<0||
      result.value.unitsConsumed>maxUnits){
    throw new Error("Transaction simulation failed or exceeded compute budget");
  }
  return Object.freeze({
    mode:"simulation-only",
    transactionSha256:beforeDigest,
    snapshotSlot:minContextSlot,
    simulationSlot:result.context.slot,
    unitsConsumed:result.value.unitsConsumed,
    signaturesVerifiedByRpc:true,
    requiresJournalAndFinality:true,
  });
}

/** Reject an altered or substituted transaction after simulation. */
export function assertSimulatedBytes(transaction,evidence){
  if(evidence?.mode!=="simulation-only"||
      typeof evidence.transactionSha256!=="string"||
      !/^[a-f0-9]{64}$/.test(evidence.transactionSha256)){
    throw new Error("Missing valid transaction simulation evidence");
  }
  if(digest(transactionBytes(transaction))!==evidence.transactionSha256){
    throw new Error("Signed transaction differs from simulated bytes");
  }
  return true;
}
