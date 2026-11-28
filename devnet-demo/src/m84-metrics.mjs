// M84 exact-receipt and aggregation primitives, no signing or sending.
export const MANIFEST_PIN="d04b7aa90b098ba64ebcdf398e9a24d89a9114d8";
const validInt=n=>Number.isSafeInteger(n)&&n>=0;
export function auditExecutedReceipt(receipt,{signature,wire,programId,market,bs58}){
 if(!receipt||receipt.version!==0||!validInt(receipt.slot)||receipt.slot===0||
   receipt.meta?.err!==null||!validInt(receipt.meta.computeUnitsConsumed)||
   receipt.meta.computeUnitsConsumed===0||!validInt(receipt.meta.fee)||
   receipt.transaction?.signatures?.[0]!==signature)
  throw Error("Missing or invalid finalized metered receipt");
 const signatures=receipt.transaction.signatures;
 if(!Array.isArray(signatures)||signatures.length===0||signatures.length>3||
   !Buffer.isBuffer(wire)||wire[0]!==signatures.length||wire.length>1232)
  throw Error("Unsupported signed wire");
 const message=receipt.transaction.message;
 const reconstructed=Buffer.concat([Buffer.from([signatures.length]),
  ...signatures.map(x=>{const sig=Buffer.from(bs58.decode(x));if(sig.length!==64)throw Error("Invalid receipt signature");return sig;}),
  Buffer.from(message.serialize())]);
 if(!wire.equals(reconstructed))throw Error("Finalized receipt differs from exact signed wire");
 if(message.addressTableLookups?.length)throw Error("ALT evidence not supported in this initial fixture");
 const keys=message.staticAccountKeys;
 const target=message.compiledInstructions.filter(ix=>keys[ix.programIdIndex]?.equals(programId));
 if(target.length!==1||!target[0].accountKeyIndexes.some(i=>keys[i]?.equals(market)))
  throw Error("Missing target program/market instruction");
 return {signature,slot:receipt.slot,computeUnits:receipt.meta.computeUnitsConsumed,
  packetBytes:wire.length,accountKeys:keys.length,instructionMetas:target[0].accountKeyIndexes.length,
  feeLamports:receipt.meta.fee};
}
export function summarizeReceipts(receipts){
 if(!Array.isArray(receipts)||!receipts.length||new Set(receipts.map(r=>r.signature)).size!==receipts.length)
  throw Error("Missing or duplicate receipt");
 for(const r of receipts)if(!["initialize","deposit","place","execute","payout"].includes(r.stage)||
   !["computeUnits","packetBytes","accountKeys","instructionMetas","feeLamports"].every(k=>validInt(r[k]))||
   r.computeUnits===0||r.packetBytes===0||r.packetBytes>1232)throw Error("Invalid receipt metrics");
 const measured=receipts.filter(r=>r.stage==="execute"||r.stage==="payout");
 if(measured.filter(r=>r.stage==="execute").length!==1||!measured.some(r=>r.stage==="payout"))
  throw Error("Execution plus payout receipts required");
 const sum=(rows,k)=>rows.reduce((a,r)=>a+r[k],0);
 const max=(rows,k)=>Math.max(...rows.map(r=>r[k]));
 return {allTransactionCount:receipts.length,allComputeUnits:sum(receipts,"computeUnits"),
  settlementTransactionCount:measured.length,settlementComputeUnits:sum(measured,"computeUnits"),
  executeComputeUnits:sum(measured.filter(r=>r.stage==="execute"),"computeUnits"),
  payoutComputeUnits:sum(measured.filter(r=>r.stage==="payout"),"computeUnits"),
  settlementPacketBytesTotal:sum(measured,"packetBytes"),settlementPacketBytesMax:max(measured,"packetBytes"),
  settlementAccountKeysMax:max(measured,"accountKeys"),settlementInstructionMetasMax:max(measured,"instructionMetas"),
  settlementFeeLamports:sum(measured,"feeLamports")};
}
export function validateFullFillPair(pair){
 const {makers,hybrid,manifest,orderBaseAtoms,initialMakerBaseAtoms}=pair;
 if(![1,8].includes(makers)||orderBaseAtoms!=="100"||initialMakerBaseAtoms!=="1000000000")
  throw Error("Unexpected paired fixture");
 const fill=BigInt(makers)*BigInt(orderBaseAtoms);
 for(const v of [hybrid,manifest]){
  if(v?.verified!==true||v.grossBaseAtoms!==String(fill)||v.grossQuoteAtoms!==String(fill)||
    v.payouts?.length!==makers||v.payouts.some(x=>x!==orderBaseAtoms)||
    v.inventoryUtilization?.numerator!==String(fill)||
    v.inventoryUtilization?.denominator!==String(BigInt(makers)*BigInt(initialMakerBaseAtoms))||
    !validInt(v.footprint?.bytes)||v.footprint.bytes===0||
    !validInt(v.footprint?.lamports)||v.footprint.lamports===0)
   throw Error("Unmatched gross fill, payout, inventory or footprint evidence");
  if(v.receipts.filter(x=>x.stage==="payout").length!==makers)throw Error("Missing maker payout receipt");
  const summary=summarizeReceipts(v.receipts);
  if(JSON.stringify(summary)!==JSON.stringify(v.metrics))throw Error("Receipt metric aggregate mismatch");
 }
 if(hybrid.protocolFeeQuoteAtoms!==String(fill/100n)||manifest.protocolFeeQuoteAtoms!=="0")
  throw Error("Unaccounted protocol fee difference");
 if(new Set([...hybrid.receipts,...manifest.receipts].map(x=>x.signature)).size!==hybrid.receipts.length+manifest.receipts.length)
  throw Error("Cross-protocol receipt reuse");
 return true;
}
