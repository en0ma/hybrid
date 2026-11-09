import test from "node:test";
import assert from "node:assert/strict";
import {mkdtemp,rm} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join} from "node:path";
import {createHash} from "node:crypto";
import {initializeAtomicMakerJournal,advanceAtomicMakerJournal} from "../src/maker-atomic-journal.mjs";
import {withMakerJournalLock} from "../src/maker-journal-guard.mjs";
import {auditFinalizedMakerState} from "../src/maker-post-finality-audit.mjs";
const Q=1n<<64n;
const key=v=>({toBytes:()=>Buffer.alloc(32,v),toString:()=>String(v),
  equals(other){return other?.toString()===String(v);}});
const [programId,market,maker,askPage,askOwnerPage,bidPage,bidOwnerPage,makerBalance]=
  [1,2,7,3,5,4,6,8].map(key);
const genesisHash="4".repeat(44);
function put128(b,i,v){b.writeBigUInt64LE(v&((1n<<64n)-1n),i);b.writeBigUInt64LE(v>>64n,i+8);}
function accounts(){
  const header=Buffer.alloc(128);
  header.write("HYBRID01",0);header[8]=1;header.writeUInt16LE(1,10);
  header.writeUInt32LE(1,12);header.writeUInt32LE(1,20);
  header.writeBigUInt64LE(3n,32);put128(header,48,Q);put128(header,64,10n);
  header.writeBigUInt64LE(1n,80);header.writeBigUInt64LE(1n,88);
  header.fill(5,96,128);header.fill(6,24,32);header.fill(6,40,48);
  const page=sequence=>{
    const p=Buffer.alloc(1552);p.writeUInt16LE(1,0);
    put128(p,16,Q);put128(p,32,Q);p.writeBigUInt64LE(10n,48);
    p.writeBigUInt64LE(BigInt(sequence),56);return p;
  };
  const owners=()=>{
    const b=Buffer.alloc(1040);b.writeUInt16LE(1,0);b.fill(7,16,48);return b;
  };
  const balance=Buffer.alloc(112);
  balance.write("HYBBAL01",0,"ascii");balance[8]=1;balance[9]=7;
  market.toBytes().copy(balance,16);maker.toBytes().copy(balance,48);
  balance.writeBigUInt64LE(20n,80);balance.writeBigUInt64LE(10n,88);
  balance.writeBigUInt64LE(30n,96);balance.writeBigUInt64LE(15n,104);
  return [header,page(1),owners(),page(2),owners(),balance].map(data=>({owner:programId,data}));
}
const sigBytes=Buffer.alloc(64,3);
const wire=Buffer.concat([Buffer.from([1]),sigBytes,Buffer.from([1,2,3])]);
const transaction={serialize(){return Buffer.from(wire);}};
function encode58(b){
  const abc="123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
  let n=0n;for(const value of b)n=n*256n+BigInt(value);
  let s="";while(n>0n){s=abc[Number(n%58n)]+s;n/=58n;}return s;
}
const signature=encode58(sigBytes);
const simulationEvidence={mode:"simulation-only",snapshotSlot:100,simulationSlot:102,
  signaturesVerifiedByRpc:true,transactionSha256:createHash("sha256").update(wire).digest("hex")};
const identity={transaction,simulationEvidence,signature,genesisHash};
async function fixture(callback,{finalized=true}={}){
  const root=await mkdtemp(join(tmpdir(),"hybrid-post-finality-"));
  try{
    const journalPath=join(root,"state");
    await withMakerJournalLock(join(root,"maker.lock"),async()=>{
      await initializeAtomicMakerJournal(journalPath,{
        ...identity,lastValidBlockHeight:500,snapshotSlot:100,lastConfirmedSlot:90,
      });
      await advanceAtomicMakerJournal(journalPath,{...identity,from:"prepared",to:"submitted"});
      if(finalized)await advanceAtomicMakerJournal(journalPath,{
        ...identity,from:"submitted",to:"finalized",finalizedSlot:105,
      });
      await callback(journalPath);
    });
  }finally{await rm(root,{force:true,recursive:true});}
}
const opts=journalPath=>({journalPath,programId,market,maker,askPage,askOwnerPage,
  bidPage,bidOwnerPage,makerBalance,expectedGenesisHash:genesisHash,maxLagSlots:10});
function rpc({slot=107,now=108,cluster=genesisHash,values=accounts()}={}){
  let requests=0;
  const connection={
    async getGenesisHash(){return cluster;},
    async getMultipleAccountsInfoAndContext(keys,options){
      requests++;assert.equal(keys.length,6);
      assert.deepEqual(options,{commitment:"finalized",minContextSlot:105});
      return {context:{slot},value:values};
    },
    async getSlot(commitment){assert.equal(commitment,"finalized");return now;},
  };
  return {connection,getRequests:()=>requests};
}
test("one finalized six-account batch gives coherent audited inventory, not execution permission",async()=>{
  await fixture(async journalPath=>{
    const {connection,getRequests}=rpc();
    const result=await auditFinalizedMakerState(connection,opts(journalPath));
    assert.equal(result.mode,"post-finality-audit-only");
    assert.equal(result.slot,107);
    assert.equal(result.askOrderCount,1);
    assert.equal(result.bidOrderCount,1);
    assert.equal(result.freeBase,20n);
    assert.equal(result.lockedQuote,15n);
    assert.equal(result.canSubmit,false);
    assert.equal(result.requiresFreshSimulationBeforeSubmission,true);
    assert.equal(getRequests(),1);
  });
});
test("unfinished journal cannot be used for post-finalization audit",async()=>{
  await fixture(async journalPath=>{
    await assert.rejects(auditFinalizedMakerState(rpc().connection,opts(journalPath)),/not finalized/);
  },{finalized:false});
});
test("reject mixed cluster, old account context, old finality, or missing account",async()=>{
  await fixture(async journalPath=>{
    await assert.rejects(auditFinalizedMakerState(rpc({cluster:"5".repeat(44)}).connection,
      opts(journalPath)),/genesis mismatch/);
    await assert.rejects(auditFinalizedMakerState(rpc({slot:104}).connection,
      opts(journalPath)),/stale/);
    await assert.rejects(auditFinalizedMakerState(rpc({now:119}).connection,
      opts(journalPath)),/stale/);
    const missing=accounts();missing[5]=null;
    await assert.rejects(auditFinalizedMakerState(rpc({values:missing}).connection,
      opts(journalPath)),/maker-balance/);
  });
});
test("reject counterfeit page owner, mismatched maker, duplicate account keys",async()=>{
  await fixture(async journalPath=>{
    const bad=accounts();bad[1]={...bad[1],owner:key(99)};
    await assert.rejects(auditFinalizedMakerState(rpc({values:bad}).connection,
      opts(journalPath)),/Incorrect program owner/);
    const badBalance=accounts();badBalance[5]={...badBalance[5],
      data:Buffer.from(badBalance[5].data)};
    badBalance[5].data.fill(9,48,80);
    await assert.rejects(auditFinalizedMakerState(rpc({values:badBalance}).connection,
      opts(journalPath)),/market or owner mismatch/);
    await assert.rejects(auditFinalizedMakerState(rpc().connection,{
      ...opts(journalPath),askPage:bidPage,
    }),/Duplicate/);
  });
});
