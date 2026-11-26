import test from "node:test";
import assert from "node:assert/strict";
import {loadSettlementLookupTable,planLookupExtension,assertSettlementWire,simulateSettlement,
  readSettlementClaimJournal} from "../src/settlement-lifecycle.mjs";
import {mkdtemp,writeFile,rm} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join} from "node:path";
const k=n=>Buffer.alloc(32,n);
const max=(1n<<64n)-1n;
test("lookup extension deduplicates required addresses and enforces capacity",()=>{
  const p=planLookupExtension([k(1)],Array.from({length:42},(_,i)=>k(i+1)));
  assert.deepEqual(p.batches.map(b=>b.length),[20,20,1]);
  assert.throws(()=>planLookupExtension([k(1),k(1)],[k(2)]));
  assert.throws(()=>planLookupExtension(Array.from({length:256},(_,i)=>k(i)),[
    Buffer.concat([Buffer.alloc(31),Buffer.from([1])])]));
});
test("lookup table ownership, activation, authority and required addresses",async()=>{
  const state={deactivationSlot:max,lastExtendedSlot:10,authority:k(1),addresses:[k(2),k(3)]};
  const adapter={deserializeLookupTable:()=>state};
  const response={context:{slot:11},value:{owner:"AddressLookupTab1e1111111111111111111111111",data:Buffer.alloc(1)}};
  const connection={getAccountInfoAndContext:async()=>response};
  const request={adapter,address:k(4),authority:k(1),requiredAddresses:[k(3)],minContextSlot:11};
  assert.deepEqual((await loadSettlementLookupTable(connection,request)).indices,[1]);
  await assert.rejects(loadSettlementLookupTable(connection,{...request,authority:k(5)}));
  await assert.rejects(loadSettlementLookupTable(connection,{...request,requiredAddresses:[k(6)]}));
  state.lastExtendedSlot=11;
  await assert.rejects(loadSettlementLookupTable(connection,request));
  state.lastExtendedSlot=10;state.deactivationSlot=10n;
  await assert.rejects(loadSettlementLookupTable(connection,request));
  state.deactivationSlot=max;response.value.owner="foreign";
  await assert.rejects(loadSettlementLookupTable(connection,request));
});
test("simulation binds exact wire and instruction, fails closed",async()=>{
  const ix={programId:k(9),keys:[{pubkey:k(1),isSigner:true,isWritable:true}],data:Buffer.from([45,1])};
  let wire=Buffer.alloc(100,1);
  const transaction={serialize:()=>wire};
  let decoded=[ix];
  const adapter={decodeSignedInstructions:()=>decoded};
  const prepared={mode:"unsigned-only",slot:10,instruction:ix};
  const connection={simulateTransaction:async(_,config)=>{
    assert.equal(config.sigVerify,true);assert.equal(config.replaceRecentBlockhash,false);
    return {context:{slot:11},value:{err:null,unitsConsumed:194262}};
  }};
  assert.equal((await simulateSettlement(connection,{adapter,transaction,prepared,currentSlot:11})).unitsConsumed,194262);
  decoded=[{...ix,data:Buffer.from([44,1])}];
  assert.throws(()=>assertSettlementWire(adapter,transaction,ix));
  decoded=[ix,ix];assert.throws(()=>assertSettlementWire(adapter,transaction,ix));
  decoded=[ix];wire=Buffer.alloc(1233);
  assert.throws(()=>assertSettlementWire(adapter,transaction,ix));
  wire=Buffer.alloc(100,1);
  await assert.rejects(simulateSettlement(connection,{adapter,transaction,prepared,currentSlot:20}));
  await assert.rejects(simulateSettlement({simulateTransaction:async()=>{
    wire=Buffer.alloc(100,2);return {context:{slot:11},value:{err:null,unitsConsumed:1}};
  }},{adapter,transaction,prepared,currentSlot:11}));
});
test("corrupt or missing journal never authorizes recovery",async()=>{
  const dir=await mkdtemp(join(tmpdir(),"hybrid-claim-"));
  try{
    const path=join(dir,"intent.json");
    await assert.rejects(readSettlementClaimJournal(path));
    await writeFile(path,JSON.stringify({record:{version:1},digest:"bad"}));
    await assert.rejects(readSettlementClaimJournal(path));
  }finally{await rm(dir,{recursive:true,force:true});}
});
