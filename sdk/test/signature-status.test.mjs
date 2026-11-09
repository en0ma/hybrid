import test from "node:test";
import assert from "node:assert/strict";
import {observeSignatureStatus} from "../src/signature-status.mjs";
const signature="3".repeat(87),expectedGenesisHash="4".repeat(44);
function fixture(status,{height=201,genesis=expectedGenesisHash,slot=200}={}){
  const calls=[];
  const rpc={
    async getGenesisHash(){calls.push("genesis");return genesis;},
    async getSignatureStatuses(signatures,options){
      assert.deepEqual(signatures,[signature]);
      assert.deepEqual(options,{searchTransactionHistory:true});
      calls.push("status");
      return {context:{slot},value:[status]};
    },
    async getBlockHeight(commitment){
      assert.equal(commitment,"finalized");
      calls.push("height");
      return height;
    }
  };
  return {rpc,calls};
}
function read(rpc,extra={}){
  return observeSignatureStatus(rpc,{
    signature,expectedGenesisHash,lastValidBlockHeight:150,...extra
  });
}
test("only finalized successful transaction is confirmable",async()=>{
  const {rpc,calls}=fixture({slot:197,err:null,confirmationStatus:"finalized"});
  const result=await read(rpc);
  assert.equal(result.state,"finalized-success");
  assert.equal(result.slot,197);
  assert.equal(result.canConfirm,true);
  assert.equal(result.canExpire,false);
  assert.deepEqual(calls,["genesis","status","height"]);
});
test("finalized failed transactions never confirm or auto-expire",async()=>{
  const {rpc}=fixture({slot:197,err:{InstructionError:[0,"Custom"]},confirmationStatus:"finalized"});
  const result=await read(rpc);
  assert.equal(result.state,"finalized-failure");
  assert.equal(result.canConfirm,false);
  assert.equal(result.canExpire,false);
});
test("null history after expiry remains unknown, not proof of absence",async()=>{
  const {rpc}=fixture(null,{height:999});
  const result=await read(rpc);
  assert.equal(result.state,"unknown");
  assert.equal(result.expiredBlockhash,true);
  assert.equal(result.canExpire,false);
  assert.equal(result.canConfirm,false);
});
test("confirmed, processed, and legacy null-confirmation status remain pending",async()=>{
  for(const status of [
    {slot:190,err:null,confirmationStatus:"confirmed"},
    {slot:190,err:null,confirmationStatus:"processed"},
    {slot:190,err:null,confirmations:null},
  ]){
    const {rpc}=fixture(status);
    const result=await read(rpc);
    assert.equal(result.state,"pending");
    assert.equal(result.canConfirm,false);
  }
});
test("reject mismatched cluster or untrusted malformed status",async()=>{
  const mismatch=fixture(null,{genesis:"5".repeat(44)});
  await assert.rejects(read(mismatch.rpc),/genesis hash mismatch/);
  assert.deepEqual(mismatch.calls,["genesis"]);
  for(const status of [
    {slot:201,err:null,confirmationStatus:"finalized"},
    {slot:197,confirmationStatus:"finalized"},
    {slot:-1,err:null,confirmationStatus:"finalized"},
    {slot:197,err:null,confirmationStatus:"imaginary"},
  ]){
    await assert.rejects(read(fixture(status).rpc),/Malformed|exceeds|Missing/);
  }
  await assert.rejects(read(fixture(null).rpc,{signature:"bad"}),/Invalid transaction signature/);
  await assert.rejects(read(fixture(null).rpc,{expectedGenesisHash:"bad"}),/genesis hash/);
});
