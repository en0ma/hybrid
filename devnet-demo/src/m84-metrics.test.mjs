import test from "node:test";
import assert from "node:assert/strict";
import {summarizeReceipts,validateFullFillPair,auditExecutedReceipt,waitForFinalized} from "./m84-metrics.mjs";
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
