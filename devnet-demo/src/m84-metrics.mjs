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
 for(const r of receipts)if(!["initialize","deposit","place","execute","payout","cancel","replace"].includes(r.stage)||
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
 const price=pair.priceAtoms??"1";
 if(!["1","2"].includes(price))throw Error("Unexpected paired price");
 if(![1,8].includes(makers)||orderBaseAtoms!=="100"||initialMakerBaseAtoms!=="1000000000")
  throw Error("Unexpected paired fixture");
 const fill=BigInt(makers)*BigInt(orderBaseAtoms),gross=fill*BigInt(price);
 for(const v of [hybrid,manifest]){
  if(v?.verified!==true||v.grossBaseAtoms!==String(fill)||v.grossQuoteAtoms!==String(gross)||
    v.payouts?.length!==makers||v.payouts.some(x=>x!==String(BigInt(orderBaseAtoms)*BigInt(price)))||
    v.inventoryUtilization?.numerator!==String(fill)||
    v.inventoryUtilization?.denominator!==String(BigInt(makers)*BigInt(initialMakerBaseAtoms))||
    !validInt(v.footprint?.bytes)||v.footprint.bytes===0||
    !validInt(v.footprint?.lamports)||v.footprint.lamports===0)
   throw Error("Unmatched gross fill, payout, inventory or footprint evidence");
  if(v.receipts.filter(x=>x.stage==="payout").length!==makers)throw Error("Missing maker payout receipt");
  const summary=summarizeReceipts(v.receipts);
  if(JSON.stringify(summary)!==JSON.stringify(v.metrics))throw Error("Receipt metric aggregate mismatch");
 }
 if(hybrid.protocolFeeQuoteAtoms!==String(gross/100n)||manifest.protocolFeeQuoteAtoms!=="0")
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
 if(![1,8,2,4].includes(makers)||!report||report.complete!==true||report.error!==undefined||
  report.kind!=="preliminary-paired-full-fill"||report.network!=="offline-surfpool"||
  report.hybridCommit!==hybridCommit||report.manifestCommit!==manifestCommit||
  !/^[0-9a-f]{40}$/.test(hybridCommit)||manifestCommit!==MANIFEST_PIN||
  !Array.isArray(report.selectedMakers)||report.selectedMakers.length!==1||report.selectedMakers[0]!==makers||
  !Array.isArray(report.cases)||report.cases.length!==1||report.cases[0].makers!==(makers===2?1:makers===4?2:makers))
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
 return makers===2?validateTwoMarketPair(row):makers===4?validatePartialPair(row):validateFullFillPair(row);
}

// Two simultaneous local markets share one funded external maker wallet.
export function validateTwoMarketPair(row){
 if(row.name!=="two-market-inventory"||row.makers!==1||row.initialMakerBaseAtoms!=="1000000000"||
  row.orderBaseAtoms!=="100"||row.makerOwners?.length!==1||typeof row.taker!=="string")
  throw Error("Unexpected two-market workload");
 const signatures=[],markets=[],wallets=[];
 for(const [protocol,fee] of [[row.hybrid,"3"],[row.manifest,"0"]]){
  const equalState=(state,expected)=>Array.isArray(state)&&state.length===2&&
   state.every((a,i)=>String(a.base)===expected[i][0]&&String(a.quote)===expected[i][1]);
  if(protocol?.verified!==true||protocol.protocolFeeQuoteAtoms!==fee||
   protocol.inventoryUtilization?.numerator!=="200"||protocol.inventoryUtilization?.denominator!=="1000000000"||
   protocol.markets?.length!==2||protocol.concurrentOpen?.length!==2||protocol.walletKeys?.length!==2||
   protocol.walletKeys[0].owner!==row.makerOwners[0]||protocol.walletKeys[1].owner!==row.taker||
   !equalState(protocol.initialWallets,[["1000000000","1000000000"],["1000000000","1000000000"]])||
   !equalState(protocol.concurrentWallets,[["999999800","1000000000"],["1000000000","1000000000"]])||
   !equalState(protocol.finalWallets,[["999999800","1000000300"],["1000000200",String(999999700n-BigInt(fee))]]))
   throw Error("Two-market shared wallet or collateral evidence mismatch");
  for(const [i,market] of protocol.markets.entries()){
   const open=protocol.concurrentOpen[i],price=String(i+1);
   if(market.pendingAttempt||market.programId!==protocol.programId||typeof market.market!=="string"||
    market.priceAtoms!==price||open.market!==market.market||open.priceAtoms!==price||
    open.lockedBaseAtoms!=="100"||open.vaultBaseAtoms!=="100"||
    !validInt(open.slot)||open.slot===0||open.slot>Math.min(...protocol.markets.flatMap(x=>x.receipts.filter(r=>r.stage==="execute").map(r=>r.slot))))
    throw Error("Missing simultaneous market collateral proof");
   markets.push(market.market);signatures.push(...market.receipts.map(r=>r.signature));
  }
  wallets.push(...protocol.walletKeys.flatMap(x=>[x.base,x.quote]));
 }
 if(new Set(markets).size!==4||new Set(wallets).size!==8||new Set(signatures).size!==signatures.length)
  throw Error("Two-market account or receipt alias");
 for(let i=0;i<2;i++)validateFullFillPair({...row,priceAtoms:String(i+1),hybrid:row.hybrid.markets[i],manifest:row.manifest.markets[i]});
 return true;
}

// A deliberate fault injection discards the acknowledgement after one real
// submission. No second submission is allowed, including on real RPC error.
export async function submitAndReconcileOnce({signature,simulateTimeout=false,sendOnce,reconcile}){
 const injectedTimeout=new Error("Injected acknowledgement timeout");
 let acknowledgementLost=false;
 try{
  const response=await sendOnce();
  if(response!==signature)throw Error("Send signature mismatch");
  if(simulateTimeout)throw injectedTimeout;
 }catch(error){
  // Only this exact fault injection is recoverable here. Unexpected transport
  // errors stop the benchmark, retaining its pending-attempt checkpoint.
  if(error!==injectedTimeout)throw error;
  acknowledgementLost=true;
 }
 const receipt=await reconcile(signature);
 return {receipt,attempts:1,acknowledgementLost};
}
export function validatePartialPair(row){
 if(row.name!=="partial-fill-cancel-replace-retry"||row.makers!==2||row.makerOwners?.length!==2||
  new Set(row.makerOwners).size!==2||row.orderBaseAtoms!=="100"||
  row.initialMakerBaseAtoms!=="1000000000")throw Error("Unexpected partial fixture");
 const specs={
  before:{base:"200",quote:"0",quantities:["100","100"],prices:["1","2"],free:["0","0"],credit:"0",fill:"0",paid:"0"},
  partial:{base:"150",quote:"50",quantities:["50","100"],prices:["1","2"],free:["0","0"],credit:"50",fill:"50",paid:"0"},
  cancelled:{base:"150",quote:"50",quantities:["0","100"],prices:["0","2"],free:["50","0"],credit:"50",fill:"50",paid:"0"},
  replaced:{base:"150",quote:"50",quantities:["50","100"],prices:["3","2"],free:["0","0"],credit:"50",fill:"50",paid:"0"},
  final:{base:"150",quote:"0",quantities:["50","100"],prices:["3","2"],free:["0","0"],credit:"0",fill:"50",paid:"50"}
 };
 const signatures=[];
 for(const v of [row.hybrid,row.manifest]){
  if(v?.verified!==true||v.pendingAttempt||v.grossBaseAtoms!=="50"||v.grossQuoteAtoms!=="50"||
   v.protocolFeeQuoteAtoms!=="0"||v.payouts?.join(",")!=="50,0"||
   v.inventoryUtilization?.numerator!=="200"||v.inventoryUtilization?.denominator!=="2000000000")
   throw Error("Partial outcome mismatch");
  const stages=v.receipts.map(r=>r.stage).join(",");
  if(stages!=="initialize,initialize,deposit,place,initialize,deposit,place,execute,cancel,replace,payout")
   throw Error("Missing ordered mutation receipts");
  if(v.receipts.some((r,i)=>!validInt(r.slot)||r.slot===0||(i>0&&r.slot<v.receipts[i-1].slot)))
   throw Error("Invalid mutation receipt ordering");
  const receipt=v.receipts.find(r=>r.stage==="replace");
  if(v.timeout?.kind!=="injected-lost-submission-acknowledgement"||v.timeout.signature!==receipt.signature||
    v.timeout.finalizedSlot!==receipt.slot||v.timeout.sendAttempts!==1||v.timeout.canRetry!==false||
    v.timeout.reconciled!==true||v.timeout.acknowledgementLost!==true)throw Error("Missing one-send signature reconciliation");
  for(const [name,spec] of Object.entries(specs)){
   const state=v.states?.[name],makers=state?.makers;
   const phaseReceipt=v.receipts[name==="before"?6:name==="partial"?7:name==="cancelled"?8:name==="replaced"?9:10];
   if(state?.vaultBase!==spec.base||state.vaultQuote!==spec.quote||makers?.length!==2||
     !validInt(state.footprint?.slot)||state.footprint.slot<phaseReceipt.slot||
     state.wallets?.length!==3||!validInt(state.footprint?.bytes)||state.footprint.bytes===0||
     !validInt(state.footprint?.lamports)||state.footprint.lamports===0)
    throw Error("Partial state mismatch: "+name);
   for(let i=0;i<2;i++){
    const a=makers[i],order=a.orders?.[0],qty=spec.quantities[i];
    if(a.owner!==row.makerOwners[i]||a.freeBase!==spec.free[i]||a.lockedBase!==qty||
     a.freeQuote!==(i===0?spec.credit:"0")||a.lockedQuote!=="0"||
     a.orders?.length!==(qty==="0"?0:1)||
     (qty!=="0"&&(order.base!==qty||order.price!==spec.prices[i]||!/^[0-9]+$/.test(order.sequence))))
     throw Error("Partial maker mismatch: "+name);
    const wallet=state.wallets[i];
    if(String(wallet.base)!=="999999900"||String(wallet.quote)!==String(1000000000n+(i===0?BigInt(spec.paid):0n)))
     throw Error("Partial maker wallet mismatch: "+name);
   }
   if(String(state.wallets[2].base)!==String(1000000000n+BigInt(spec.fill))||
     String(state.wallets[2].quote)!==String(1000000000n-BigInt(spec.fill)))
    throw Error("Partial taker wallet mismatch: "+name);
   if(name!=="before"&&makers[1].orders[0].sequence!==v.states.before.makers[1].orders[0].sequence)
    throw Error("Unrelated maker order changed");
  }
  if(v.states.partial.makers[0].orders[0].sequence!==v.states.before.makers[0].orders[0].sequence||
   v.states.replaced.makers[0].orders[0].sequence===v.states.before.makers[0].orders[0].sequence||
   v.states.final.makers[0].orders[0].sequence!==v.states.replaced.makers[0].orders[0].sequence)
   throw Error("Replacement order sequence mismatch");
  if(JSON.stringify(summarizeReceipts(v.receipts))!==JSON.stringify(v.metrics))throw Error("Partial receipt aggregate mismatch");
  signatures.push(...v.receipts.map(r=>r.signature));
 }
 if(row.hybrid.market===row.manifest.market||new Set(signatures).size!==signatures.length)throw Error("Partial pair alias");
 return true;
}
