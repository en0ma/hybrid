import test from "node:test";
import assert from "node:assert/strict";
import * as web3 from "@solana/web3.js";
import {createSolanaSettlementAdapter} from "../src/settlement-solana.mjs";
import {buildSettlementExecution} from "../src/settlement.mjs";
import {assertSettlementWire,simulateSettlement} from "../src/settlement-lifecycle.mjs";
const adapter=createSolanaSettlementAdapter(web3);
const key=()=>web3.Keypair.generate().publicKey;
test("actual signed v0 sixteen-maker BID resolves authenticated lookup keys",async()=>{
  const payer=web3.Keypair.generate(),taker=web3.Keypair.generate();
  const a={market:key(),custody:key(),reserve:key(),epochs:[key(),key()],
    pages:[{index:0,page:key(),ownerPage:key()},{index:1,page:key(),ownerPage:key()}],
    taker:taker.publicKey,takerQuote:key(),takerBase:key(),quoteVault:key(),baseVault:key(),
    quoteMint:key(),baseMint:key(),vaultAuthority:key(),tokenProgram:key(),payer:payer.publicKey,
    systemProgram:web3.SystemProgram.programId,makerBalances:Array.from({length:16},key)};
  const ix=buildSettlementExecution(key(),a,{side:"bid",baseFill:350n,quoteLimit:325n});
  const table={address:key(),slot:11,state:{deactivationSlot:(1n<<64n)-1n,lastExtendedSlot:9,
    lastExtendedSlotStartIndex:0,authority:payer.publicKey,
    addresses:ix.keys.filter(k=>!k.isSigner).map(k=>k.pubkey)}};
  const tx=adapter.compileUnsignedV0({payer:payer.publicKey,recentBlockhash:key().toBase58(),
    instruction:ix,lookupTables:[table],computeBudgetInstructions:[
      web3.ComputeBudgetProgram.setComputeUnitLimit({units:400000})]});
  tx.sign([payer,taker]);
  assert.ok(tx.serialize().length<=1232);
  assert.equal(tx.message.addressTableLookups.length,1);
  assert.match(assertSettlementWire(adapter,tx,ix,[table]),/^[a-f0-9]{64}$/);
  assert.throws(()=>assertSettlementWire(adapter,tx,ix,[]));
  assert.throws(()=>assertSettlementWire(adapter,tx,{...ix,data:Buffer.from([45])},[table]));
  const evidence=await simulateSettlement({simulateTransaction:async()=>({
    context:{slot:12},value:{err:null,unitsConsumed:312759}})},{
    adapter,transaction:tx,prepared:{mode:"unsigned-only",slot:10,instruction:ix},
    lookupTables:[table],currentSlot:11});
  assert.equal(evidence.unitsConsumed,312759);
  const changed={...table,state:{...table.state,addresses:[...table.state.addresses].reverse()}};
  assert.throws(()=>assertSettlementWire(adapter,tx,ix,[changed]));
});
test("actual lookup create and extend descriptors, without signing",()=>{
  const authority=key(),payer=key();
  const {instruction,address}=adapter.buildLookupCreate({authority,payer,recentSlot:10});
  assert.ok(instruction.programId.equals(web3.AddressLookupTableProgram.programId));
  const extend=adapter.buildLookupExtend({address,authority,payer,addresses:[key(),key()]});
  assert.ok(extend.programId.equals(web3.AddressLookupTableProgram.programId));
  assert.throws(()=>adapter.buildLookupExtend({address,authority,payer,addresses:Array.from({length:21},key)}));
});
