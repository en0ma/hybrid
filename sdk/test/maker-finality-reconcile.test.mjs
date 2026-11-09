import test from "node:test";
import assert from "node:assert/strict";
import {mkdtemp,rm} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join} from "node:path";
import {createHash} from "node:crypto";
import {initializeAtomicMakerJournal,advanceAtomicMakerJournal,readAtomicMakerJournal} from "../src/maker-atomic-journal.mjs";
import {reconcileFinalizedAtomicMakerJournal} from "../src/maker-finality-reconcile.mjs";
import {withMakerJournalLock} from "../src/maker-journal-guard.mjs";

const sigBytes=Buffer.alloc(64,3);
const bytes=Buffer.concat([Buffer.from([1]),sigBytes,Buffer.from([7,8,9])]);
const transaction={serialize(){return Buffer.from(bytes);}};
function b58(input){
  const alphabet="123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
  let n=0n;for(const b of input)n=n*256n+BigInt(b);
  let out="";while(n>0n){out=alphabet[Number(n%58n)]+out;n/=58n;}
  for(const b of input){if(b!==0)break;out="1"+out;}return out;
}
const signature=b58(sigBytes),genesisHash="4".repeat(44);
const simulationEvidence={
  mode:"simulation-only",transactionSha256:createHash("sha256").update(bytes).digest("hex"),
  snapshotSlot:100,simulationSlot:102,signaturesVerifiedByRpc:true,
};
const identity={transaction,simulationEvidence,signature,genesisHash};
function rpc(status,{cluster=genesisHash}={}){
  return {
    async getGenesisHash(){return cluster;},
    async getSignatureStatuses(signatures,opts){
      assert.deepEqual(signatures,[signature]);
      assert.deepEqual(opts,{searchTransactionHistory:true});
      return {context:{slot:120},value:[status]};
    },
    async getBlockHeight(commitment){
      assert.equal(commitment,"finalized");
      return 550;
    },
  };
}
const success={slot:110,err:null,confirmationStatus:"finalized"};
const args=path=>({journalPath:path,transaction,simulationEvidence,expectedGenesisHash:genesisHash});
async function fixture(callback){
  const folder=await mkdtemp(join(tmpdir(),"hybrid-finality-test-"));
  try{
    const path=join(folder,"journal");
    await withMakerJournalLock(join(folder,"maker.lock"),async()=>{
      await initializeAtomicMakerJournal(path,{...identity,snapshotSlot:100,lastConfirmedSlot:90,
        lastValidBlockHeight:500});
      await advanceAtomicMakerJournal(path,{...identity,from:"prepared",to:"submitted"});
      await callback(path);
    });
  }finally{await rm(folder,{force:true,recursive:true});}
}
test("finalized success advances exact submitted identity durably",async()=>{
  await fixture(async path=>{
    const outcome=await reconcileFinalizedAtomicMakerJournal(rpc(success),args(path));
    assert.deepEqual(outcome,{
      state:"finalized-success",changed:true,finalizedSlot:110,
      canRetry:false,requiresFreshMakerState:true,
    });
    const record=await readAtomicMakerJournal(path);
    assert.equal(record.phase,"finalized");
    assert.equal(record.lastConfirmedSlot,110);
    await assert.rejects(reconcileFinalizedAtomicMakerJournal(rpc(success),args(path)),
      /not awaiting/);
  });
});
test("null status, unfinalized and failed finalized results never release journal",async()=>{
  for(const [status,wanted] of [
    [null,"unknown"],
    [{slot:110,err:null,confirmationStatus:"confirmed"},"pending"],
    [{slot:110,err:{InstructionError:[0,"Custom"]},confirmationStatus:"finalized"},"finalized-failure"],
  ]){
    await fixture(async path=>{
      const result=await reconcileFinalizedAtomicMakerJournal(rpc(status),args(path));
      assert.deepEqual(result,{state:wanted,changed:false,canRetry:false});
      assert.equal((await readAtomicMakerJournal(path)).phase,"submitted");
    });
  }
});
test("wrong cluster, wrong wire bytes and stale finality never mutate journal",async()=>{
  await fixture(async path=>{
    await assert.rejects(reconcileFinalizedAtomicMakerJournal(
      rpc(success,{cluster:"5".repeat(44)}),args(path)),/genesis hash mismatch/);
    await assert.rejects(reconcileFinalizedAtomicMakerJournal(rpc(success),{
      ...args(path),expectedGenesisHash:"5".repeat(44),
    }),/cluster mismatch/);
    await assert.rejects(reconcileFinalizedAtomicMakerJournal(rpc(success),{
      ...args(path),simulationEvidence:{...simulationEvidence,transactionSha256:"0".repeat(64)},
    }),/differs/);
    await assert.rejects(reconcileFinalizedAtomicMakerJournal(rpc({
      ...success,slot:99,
    }),args(path)),/older than/);
    assert.equal((await readAtomicMakerJournal(path)).phase,"submitted");
  });
});
