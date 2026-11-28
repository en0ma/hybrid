import test from "node:test";
import assert from "node:assert/strict";
import {summarizeReceipts,validateFullFillPair,auditExecutedReceipt,waitForFinalized,validatePairedShard,validateTwoMarketPair,MANIFEST_PIN} from "./m84-metrics.mjs";
import {Keypair,TransactionMessage,VersionedTransaction,PublicKey} from "@solana/web3.js";
import bs58 from "bs58";
test("aggregate execution and all payout costs, never setup-only costs",()=>{
 const row=(stage,signature,computeUnits)=>({stage,signature,computeUnits,packetBytes:200,accountKeys:5,instructionMetas:3,feeLamports:5000});
 const rows=[row("initialize","a",100),row("execute","b",20),row("payout","c",30),row("payout","d",40)];
 const s=summarizeReceipts(rows);
 assert.equal(s.settlementComputeUnits,90);assert.equal(s.allComputeUnits,190);
 assert.equal(s.settlementTransactionCount,3);assert.equal(s.settlementPacketBytesTotal,600);
 assert.throws(()=>summarizeReceipts([...rows,rows[1]]),/duplicate/);
 assert.throws(()=>summarizeReceipts(rows.slice(0,2)),/payout/);
});
test("finalized receipt must reproduce the entire signed wire and bind target market",()=>{
 const payer=Keypair.generate(),programId=Keypair.generate().publicKey,market=Keypair.generate().publicKey;
 const message=new TransactionMessage({payerKey:payer.publicKey,recentBlockhash:new PublicKey(Buffer.alloc(32,8)).toBase58(),
  instructions:[{programId,keys:[{pubkey:market,isWritable:true,isSigner:false}],data:Buffer.from([4])}]}).compileToV0Message();
 const tx=new VersionedTransaction(message);tx.sign([payer]);const wire=Buffer.from(tx.serialize());
 const signature=bs58.encode(tx.signatures[0]);
 const receipt={version:0,slot:1,meta:{err:null,computeUnitsConsumed:123,fee:5000},transaction:{signatures:[signature],message}};
 const opts={signature,wire,programId,market,bs58};
 assert.equal(auditExecutedReceipt(receipt,opts).packetBytes,wire.length);
 assert.throws(()=>auditExecutedReceipt(receipt,{...opts,market:Keypair.generate().publicKey}),/market/);
 const bad=Buffer.from(wire);bad[bad.length-1]^=1;
 assert.throws(()=>auditExecutedReceipt(receipt,{...opts,wire:bad}),/wire/);
 assert.throws(()=>auditExecutedReceipt({...receipt,meta:{...receipt.meta,err:{InstructionError:[0,1]}}},opts),/receipt/);
});
test("paired aggregate rejects missing or unequal state evidence",()=>{
 const result=(prefix,fee)=>{const receipts=["execute","payout"].map((stage,i)=>({stage,signature:prefix+i,computeUnits:10,packetBytes:200,accountKeys:5,instructionMetas:3,feeLamports:5000}));
  return {verified:true,grossBaseAtoms:"100",grossQuoteAtoms:"100",payouts:["100"],protocolFeeQuoteAtoms:fee,
   inventoryUtilization:{numerator:"100",denominator:"1000000000"},footprint:{bytes:100,lamports:500},receipts,metrics:summarizeReceipts(receipts)};};
 const pair={makers:1,orderBaseAtoms:"100",initialMakerBaseAtoms:"1000000000",hybrid:result("h","1"),manifest:result("m","0")};
 assert.equal(validateFullFillPair(pair),true);
 assert.throws(()=>validateFullFillPair({...pair,manifest:{...pair.manifest,payouts:["99"]}}),/Unmatched/);
 assert.throws(()=>validateFullFillPair({...pair,manifest:{...pair.manifest,protocolFeeQuoteAtoms:"1"}}),/fee/);
});

test("HTTP-only finality waits for status, finalized bank and exact receipt",async()=>{
 let polls=0,reads=0,delays=0;
 const receipt={slot:9,meta:{err:null},transaction:{signatures:["sig"]}};
 const connection={
  async getSignatureStatuses(keys,opts){
   assert.deepEqual(keys,["sig"]);assert.equal(opts.searchTransactionHistory,true);
   polls++;return {value:[polls===1?null:{slot:9,confirmationStatus:"finalized",err:null}]};
  },
  async getSlot(commitment){assert.equal(commitment,"finalized");return polls===2?8:9;},
  async getTransaction(signature,opts){assert.equal(signature,"sig");assert.equal(opts.commitment,"finalized");reads++;return reads===1?null:receipt;}
 };
 assert.equal(await waitForFinalized(connection,"sig",{attempts:4,delay:async()=>{delays++;}}),receipt);
 assert.equal(polls,4);assert.equal(reads,2);assert.equal(delays,3);
});
test("airdrop status-only finality does not require a transaction receipt",async()=>{
 const connection={getSignatureStatuses:async()=>({value:[{slot:8,confirmationStatus:"finalized",err:null}]}),
 getSlot:async()=>8};
 assert.deepEqual(await waitForFinalized(connection,"funding",{receiptRequired:false,attempts:1}),{slot:8});
});
test("missing, failed and mismatched finality never release the polling gate",async()=>{
 let polls=0;
 const opts={attempts:3,delay:async()=>{}};
 await assert.rejects(waitForFinalized({getSignatureStatuses:async()=>{polls++;return {value:[null]};}},"sig",opts),/unavailable within polling budget/);
 assert.equal(polls,3);
 const finalized={getSignatureStatuses:async()=>({value:[{slot:8,confirmationStatus:"finalized",err:null}]}),getSlot:async()=>8};
 await assert.rejects(waitForFinalized({...finalized,getTransaction:async()=>({slot:9,meta:{err:null},transaction:{signatures:["sig"]}})},"sig",opts),/status\/receipt mismatch/);
 await assert.rejects(waitForFinalized({getSignatureStatuses:async()=>({value:[{slot:8,confirmationStatus:"finalized",err:{InstructionError:[0,1]}}]})},"sig",opts),/Finalized failure/);
});

test("paired shard requires exact revision, case, program hashes, runtime and complete evidence",()=>{
 const h="US517G5965aydkZ46HS38QLi7UQiSojurfbQfKCELFx",m="MNFSTqtC93rEfYHB6hF82sKdZpUDFWkViLByLd1k1Ms";
 const result=(programId,fee)=>{
  const receipts=["execute","payout"].map((stage,i)=>({stage,signature:programId+i,computeUnits:10,packetBytes:200,accountKeys:5,instructionMetas:3,feeLamports:5000}));
  return {programId,verified:true,grossBaseAtoms:"100",grossQuoteAtoms:"100",payouts:["100"],protocolFeeQuoteAtoms:fee,
   inventoryUtilization:{numerator:"100",denominator:"1000000000"},footprint:{bytes:100,lamports:500},receipts,metrics:summarizeReceipts(receipts)};
 };
 const report={kind:"preliminary-paired-full-fill",network:"offline-surfpool",complete:true,hybridCommit:"a".repeat(40),
  manifestCommit:MANIFEST_PIN,selectedMakers:[1],genesis:"fixture",toolchain:{caseMakers:"1",surfpoolBinarySha256:"b".repeat(64)},
  hybridBuild:{programId:h,elfSha256:"c".repeat(64)},manifestBuild:{programId:m,elfSha256:"d".repeat(64)},
  cases:[{makers:1,orderBaseAtoms:"100",initialMakerBaseAtoms:"1000000000",hybrid:result(h,"1"),manifest:result(m,"0")}]};
 const opts={hybridCommit:report.hybridCommit,manifestCommit:MANIFEST_PIN,makers:1};
 assert.equal(validatePairedShard(report,opts),true);
 assert.throws(()=>validatePairedShard(report,{...opts,makers:8}),/wrong-head/);
 assert.throws(()=>validatePairedShard({...report,complete:false},opts),/wrong-head/);
 assert.throws(()=>validatePairedShard({...report,hybridCommit:"e".repeat(40)},opts),/wrong-head/);
 assert.throws(()=>validatePairedShard({...report,toolchain:{caseMakers:"1"}},opts),/pinned runtime/);
 const pending=structuredClone(report);pending.cases[0].hybrid.pendingAttempt={status:"submission-attempted"};
 assert.throws(()=>validatePairedShard(pending,opts),/completed receipt/);
});

test("two-market evidence requires simultaneous distinct markets and a single shared funding baseline",()=>{
 const h="hybrid",m="manifest";
 function protocol(programId,fee){
  const markets=[1,2].map((price,i)=>{
   const receipts=["execute","payout"].map((stage,j)=>({stage,signature:programId+i+j,slot:10+i*2+j,
    computeUnits:10,packetBytes:200,accountKeys:5,instructionMetas:3,feeLamports:5000}));
   return {programId,market:programId+i,priceAtoms:String(price),verified:true,grossBaseAtoms:"100",
    grossQuoteAtoms:String(price*100),payouts:[String(price*100)],protocolFeeQuoteAtoms:String(fee?price:0),
    inventoryUtilization:{numerator:"100",denominator:"1000000000"},footprint:{bytes:100,lamports:500},receipts,metrics:summarizeReceipts(receipts)};
  });
  return {programId,verified:true,protocolFeeQuoteAtoms:String(fee),markets,
   inventoryUtilization:{numerator:"200",denominator:"1000000000"},
   concurrentOpen:markets.map(x=>({market:x.market,priceAtoms:x.priceAtoms,lockedBaseAtoms:"100",vaultBaseAtoms:"100",slot:9})),
   walletKeys:[{owner:"maker",base:programId+"mb",quote:programId+"mq"},{owner:"taker",base:programId+"tb",quote:programId+"tq"}],
   initialWallets:[{base:"1000000000",quote:"1000000000"},{base:"1000000000",quote:"1000000000"}],
   concurrentWallets:[{base:"999999800",quote:"1000000000"},{base:"1000000000",quote:"1000000000"}],
   finalWallets:[{base:"999999800",quote:"1000000300"},{base:"1000000200",quote:String(999999700-fee)}]};
 }
 const row={name:"two-market-inventory",makers:1,orderBaseAtoms:"100",initialMakerBaseAtoms:"1000000000",
  makerOwners:["maker"],taker:"taker",hybrid:protocol(h,3),manifest:protocol(m,0)};
 assert.equal(validateTwoMarketPair(row),true);
 const bad=structuredClone(row);bad.hybrid.concurrentWallets[0].base="999999900";
 assert.throws(()=>validateTwoMarketPair(bad),/shared wallet/);
 const late=structuredClone(row);late.manifest.concurrentOpen[1].slot=13;
 assert.throws(()=>validateTwoMarketPair(late),/simultaneous/);
 const alias=structuredClone(row);alias.manifest.markets[1].market=alias.manifest.markets[0].market;alias.manifest.concurrentOpen[1].market=alias.manifest.markets[0].market;
 assert.throws(()=>validateTwoMarketPair(alias),/alias/);
});
