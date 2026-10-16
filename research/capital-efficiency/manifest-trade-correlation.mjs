import {inspectManifestSwapReceipt} from "./manifest-swap-attribution.mjs";
import {auditManifestFill} from "./manifest-fill-audit.mjs";
// Correlates the same finalized receipt, program opcode and book/account deltas.
// Not an execution-price proof: explicitly never returns verified:true.
export function correlateManifestTrade({receipt,signature,programId,market,taker,baseMint,quoteMint,makerOwner,before,after}){
 if(before?.market!==market||after?.market!==market)throw Error("market snapshot identity mismatch");
 if(!Number.isSafeInteger(before.slot)||!Number.isSafeInteger(after.slot)||!(before.slot<receipt?.slot&&receipt.slot<=after.slot))throw Error("receipt slot not bracketed by market snapshots");
 const instruction=inspectManifestSwapReceipt({receipt,signature,programId,market,taker});
 if(instruction.opcodeAuthenticated!==true||instruction.matchingSwaps!==1)throw Error("Manifest swap opcode not authenticated");
 const accounting=auditManifestFill({before,after,baseMint,quoteMint,makerOwner,takerOwner:taker,receipt});
 if(accounting.consistencyChecked!==true||accounting.verified!==false)throw Error("unexpected accounting proof state");
 return {kind:"manifest-swap-and-partial-fill-correlated",verified:false,opcodeAuthenticated:true,tokenAndBookConsistencyChecked:true,slot:receipt.slot,signature,market,filledBaseAtoms:accounting.filledBaseAtoms,limitations:["No signature authorization check for taker yet","No full SwapParams decode or token-vault/seat conservation proof","Snapshots may span intervening transactions; not causal proof of fill","Do not publish as completed benchmark evidence"]};
}
