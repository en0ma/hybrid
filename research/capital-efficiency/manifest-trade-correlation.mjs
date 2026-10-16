import {inspectManifestSwapReceipt} from "./manifest-swap-attribution.mjs";
import {auditManifestFill} from "./manifest-fill-audit.mjs";
// Correlates the same finalized receipt, program opcode and book/account deltas.
// Not an execution-price proof: explicitly never returns verified:true.
export function correlateManifestTrade({receipt,signature,programId,market,taker,baseMint,quoteMint,makerOwner,before,after}){
 if(before?.market!==market||after?.market!==market)throw Error("market snapshot identity mismatch");
 if(!Number.isSafeInteger(before.slot)||!Number.isSafeInteger(after.slot)||!(before.slot<receipt?.slot&&receipt.slot<=after.slot))throw Error("receipt slot not bracketed by market snapshots");
 // An unrelated instruction in the same transaction can move tokens or mutate the
 // book; reject those receipts until per-instruction state diffs are available.
 const msg=receipt?.transaction?.message;
 const keys=[...(msg?.accountKeys??[]),...(receipt?.meta?.loadedAddresses?.writable??[]),...(receipt?.meta?.loadedAddresses?.readonly??[])];
 const budget="ComputeBudget111111111111111111111111111111";
 if(!Array.isArray(msg?.instructions)||msg.instructions.filter(ix=>keys[ix.programIdIndex]!==budget).length!==1)
  throw Error("isolated Manifest Swap transaction required; unrelated instructions make attribution ambiguous");
 const instruction=inspectManifestSwapReceipt({receipt,signature,programId,market,taker});
 if(instruction.opcodeAuthenticated!==true||instruction.matchingSwaps!==1)throw Error("Manifest swap opcode not authenticated");
 const accounting=auditManifestFill({before,after,baseMint,quoteMint,makerOwner,takerOwner:taker,receipt});
 if(accounting.consistencyChecked!==true||accounting.verified!==false)throw Error("unexpected accounting proof state");
 const params=instruction.swapParams;
 if(params.isBaseIn)throw Error("base-in Swap cannot be attributed to a maker ask base-out fill");
 const filled=BigInt(accounting.filledBaseAtoms),requestedOut=BigInt(params.outAtoms);
 // Only count the taker's own existing SPL accounts, not protocol vaults.
 // Every pair has already been checked for stable index, mint and owner.
 const total=(rows,mint)=>rows.filter(r=>r.owner===taker&&r.mint===mint).reduce((n,r)=>n+BigInt(r.uiTokenAmount.amount),0n);
 const pre=receipt.meta.preTokenBalances,post=receipt.meta.postTokenBalances;
 const takerBaseGain=total(post,baseMint)-total(pre,baseMint);
 const takerQuoteSpent=total(pre,quoteMint)-total(post,quoteMint);
 if(takerBaseGain<=0n||takerQuoteSpent<=0n)throw Error("taker did not receive base and spend quote");
 if(takerBaseGain!==filled)throw Error("taker base movement differs from maker resting ask reduction");
 if(takerQuoteSpent>BigInt(params.inAtoms))throw Error("taker quote spend exceeds Swap maximum input");
 if(params.isExactIn){
  if(filled<requestedOut)throw Error("actual base fill below Swap minimum output");
 }else if(filled!==requestedOut)throw Error("actual base fill differs from exact-output Swap request");
 return {kind:"manifest-swap-and-partial-fill-correlated",verified:false,opcodeAuthenticated:true,tokenAndBookConsistencyChecked:true,slot:receipt.slot,signature,market,filledBaseAtoms:accounting.filledBaseAtoms,takerQuoteSpentAtoms:takerQuoteSpent.toString(),takerBaseReceivedAtoms:takerBaseGain.toString(),swapParams:params,quantityConstraintChecked:true,isolatedTransactionChecked:true,limitations:["Signer membership is RPC-reported, not independently cryptographically verified","No custody vault/seat verification, protocol fee accounting or comprehensive realized-price proof","Snapshots may span intervening transactions; not causal proof of fill","Do not publish as completed benchmark evidence"]};
}
