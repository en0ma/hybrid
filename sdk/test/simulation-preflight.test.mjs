import test from "node:test";
import assert from "node:assert/strict";
import {
  simulateMakerTransaction,assertSimulatedBytes,
} from "../src/simulation-preflight.mjs";

function setup({
  bytes=Buffer.from([1,2,3]),slot=102,err=null,units=2000,
}={}){
  let wire=Buffer.from(bytes);
  const calls=[];
  const transaction={serialize(){return Buffer.from(wire);}};
  const connection={
    async simulateTransaction(tx,options){
      assert.equal(tx,transaction);
      assert.deepEqual(options,{
        commitment:"confirmed",minContextSlot:100,
        sigVerify:true,replaceRecentBlockhash:false,
      });
      calls.push("simulate");
      return {context:{slot},value:{err,unitsConsumed:units}};
    }
  };
  return {transaction,connection,calls,setWire:x=>{wire=Buffer.from(x);}};
}
function simulate(f,extra={}){
  return simulateMakerTransaction(f.connection,{
    transaction:f.transaction,minContextSlot:100,currentSlot:101,
    maxLagSlots:4,maxUnits:5000,...extra,
  });
}
test("successful RPC signature-verified simulation binds exact serialized bytes",async()=>{
  const f=setup();
  const evidence=await simulate(f);
  assert.equal(evidence.mode,"simulation-only");
  assert.equal(evidence.unitsConsumed,2000);
  assert.equal(evidence.signaturesVerifiedByRpc,true);
  assert.equal(evidence.requiresJournalAndFinality,true);
  assert.equal(evidence.transactionSha256.length,64);
  assert.ok(Object.isFrozen(evidence));
  assert.equal(assertSimulatedBytes(f.transaction,evidence),true);
  f.setWire([1,2,4]);
  assert.throws(()=>assertSimulatedBytes(f.transaction,evidence),/differs/);
  assert.deepEqual(f.calls,["simulate"]);
});
test("RPC simulation rejects error, compute overrun and stale slot",async()=>{
  await assert.rejects(simulate(setup({err:{InstructionError:[0,"Custom"]}})),/failed/);
  await assert.rejects(simulate(setup({units:5001})),/budget/);
  await assert.rejects(simulate(setup({slot:100})),/stale/);
  await assert.rejects(simulate(setup(),{currentSlot:110}),/Stale/);
  await assert.rejects(simulate(setup(),{maxUnits:0}),/compute-unit/);
});
test("reject transaction mutation during RPC request and malformed evidence",async()=>{
  const f=setup();
  f.connection.simulateTransaction=async()=>{
    f.setWire([9,9,9]);
    return {context:{slot:102},value:{err:null,unitsConsumed:10}};
  };
  await assert.rejects(simulate(f),/changed during simulation/);
  assert.throws(()=>assertSimulatedBytes(f.transaction,{}),/Missing/);
  assert.throws(()=>assertSimulatedBytes(f.transaction,{
    mode:"simulation-only",transactionSha256:"0".repeat(64)
  }),/differs/);
});
test("reject unsigned, empty or oversized serialized transaction data",async()=>{
  for(const transaction of [
    null,{}, {serialize(){return Buffer.alloc(0);}},
    {serialize(){return Buffer.alloc(1233);}},
  ]){
    await assert.rejects(simulate(setup(),{transaction}),/pre-signed|length/);
  }
});
