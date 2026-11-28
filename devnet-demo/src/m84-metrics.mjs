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

// Poll only HTTP methods, never signatureSubscribe/confirmTransaction.
 // The caller must use a transport with an independent per-request deadline.
 // A missing signature or receipt is uncertainty, not authorization to resend.
export async function waitForFinalized(connection,signature,{receiptRequired=true,attempts=120,
 delay=()=>new Promise(resolve=>setTimeout(resolve,500))}={}){
 if(!Number.isSafeInteger(attempts)||attempts<=0||attempts>120)throw Error("Invalid finality polling budget");
 for(let i=0;i<attempts;i++){
  const response=await connection.getSignatureStatuses([signature],{searchTransactionHistory:true});
  if(!Array.isArray(response?.value)||response.value.length!==1)throw Error("Invalid signature status response");
  const status=response.value[0];
  if(status?.confirmationStatus==="finalized"){
   if(status.err!==null||!validInt(status.slot)||status.slot===0)throw Error("Finalized failure or invalid status: "+signature);
   const finalizedSlot=await connection.getSlot("finalized");
   if(!validInt(finalizedSlot))throw Error("Invalid finalized bank slot");
   if(finalizedSlot>=status.slot){
    if(!receiptRequired)return {slot:status.slot};
    const receipt=await connection.getTransaction(signature,{commitment:"finalized",maxSupportedTransactionVersion:0});
    if(receipt){
     if(receipt.slot!==status.slot||receipt.meta?.err!==null||
       receipt.transaction?.signatures?.[0]!==signature)throw Error("Finalized status/receipt mismatch");
     return receipt;
    }
   }
  }
  if(i+1<attempts)await delay();
 }
 throw Error("Finalized "+(receiptRequired?"receipt":"status")+" unavailable within polling budget: "+signature);
}

// A CI shard is evidence for exactly one paired workload on one runtime.
export function validatePairedShard(report,{hybridCommit,manifestCommit,makers}){
 if(![1,8].includes(makers)||!report||report.complete!==true||report.error!==undefined||
  report.kind!=="preliminary-paired-full-fill"||report.network!=="offline-surfpool"||
  report.hybridCommit!==hybridCommit||report.manifestCommit!==manifestCommit||
  !/^[0-9a-f]{40}$/.test(hybridCommit)||manifestCommit!==MANIFEST_PIN||
  !Array.isArray(report.selectedMakers)||report.selectedMakers.length!==1||report.selectedMakers[0]!==makers||
  !Array.isArray(report.cases)||report.cases.length!==1||report.cases[0].makers!==makers)
  throw Error("Incomplete or wrong-head paired shard");
 const row=report.cases[0];
 if(row.hybrid?.pendingAttempt||row.manifest?.pendingAttempt||
   row.hybrid?.programId!=="US517G5965aydkZ46HS38QLi7UQiSojurfbQfKCELFx"||
   row.manifest?.programId!=="MNFSTqtC93rEfYHB6hF82sKdZpUDFWkViLByLd1k1Ms"||
   row.hybrid.programId!==report.hybridBuild?.programId||row.manifest.programId!==report.manifestBuild?.programId||
   ![report.hybridBuild?.elfSha256,report.manifestBuild?.elfSha256,report.toolchain?.surfpoolBinarySha256]
     .every(x=>typeof x==="string"&&/^[0-9a-f]{64}$/.test(x))||
   typeof report.genesis!=="string"||!report.genesis||report.toolchain?.caseMakers!==String(makers))
  throw Error("Missing pinned runtime or completed receipt evidence");
 return validateFullFillPair(row);
}
