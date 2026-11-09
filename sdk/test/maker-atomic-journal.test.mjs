import test from "node:test";
import assert from "node:assert/strict";
import {mkdtemp,rm,readFile,writeFile,stat} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join} from "node:path";
import {createHash} from "node:crypto";
import {
  initializeAtomicMakerJournal,advanceAtomicMakerJournal,readAtomicMakerJournal,
} from "../src/maker-atomic-journal.mjs";
import {withMakerJournalLock} from "../src/maker-journal-guard.mjs";
const signatureBytes=Buffer.alloc(64,3);
const bytes=Buffer.concat([Buffer.from([1]),signatureBytes,Buffer.from([1,2,3,4,5])]);
function encode58(bytes){
  const alphabet="123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
  let n=0n;for(const byte of bytes)n=n*256n+BigInt(byte);
  let value="";while(n>0n){value=alphabet[Number(n%58n)]+value;n/=58n;}
  for(const b of bytes){if(b!==0)break;value="1"+value;}
  return value;
}
const transaction={serialize(){return Buffer.from(bytes);}};
const simulationEvidence={
  mode:"simulation-only",transactionSha256:createHash("sha256").update(bytes).digest("hex"),
  snapshotSlot:100,simulationSlot:102,signaturesVerifiedByRpc:true,
};
const identity={transaction,simulationEvidence,signature:encode58(signatureBytes),
  genesisHash:"4".repeat(44)};
const initial={...identity,lastValidBlockHeight:500,snapshotSlot:100,lastConfirmedSlot:90};
async function fixture(callback){
  const folder=await mkdtemp(join(tmpdir(),"hybrid-atomic-test-"));
  try{await callback(join(folder,"maker.state"),join(folder,"maker.lock"));}
  finally{await rm(folder,{recursive:true,force:true});}
}
test("one durable record includes exact transaction identity throughout transitions",async()=>{
  await fixture(async(path,lock)=>{
    await withMakerJournalLock(lock,async()=>{
      const prepared=await initializeAtomicMakerJournal(path,initial);
      assert.equal(prepared.phase,"prepared");
      assert.equal((await readAtomicMakerJournal(path)).transactionSha256,simulationEvidence.transactionSha256);
      await assert.rejects(initializeAtomicMakerJournal(path,initial),/EEXIST/);
      // Failed duplicate initialization deliberately leaves an unresolved pending
      // record; explicit recovery is necessary, so perform transition in
      // a separate clean fixture instead.
    });
  });
});
test("submitted remains blocked until finality, and finality persists across restart",async()=>{
  await fixture(async(path,lock)=>{
    await withMakerJournalLock(lock,async()=>{
      await initializeAtomicMakerJournal(path,initial);
      await advanceAtomicMakerJournal(path,{...identity,from:"prepared",to:"submitted"});
      await assert.rejects(advanceAtomicMakerJournal(path,{
        ...identity,from:"submitted",to:"expired",authoritativeAbsent:true,
        currentBlockHeight:500,
      }),/Expiry requires/);
      await assert.rejects(advanceAtomicMakerJournal(path,{
        ...identity,from:"submitted",to:"expired",currentBlockHeight:501,
      }),/Expiry requires/);
      const done=await advanceAtomicMakerJournal(path,{
        ...identity,from:"submitted",to:"finalized",finalizedSlot:105,
      });
      assert.equal(done.lastConfirmedSlot,105);
      assert.equal((await readAtomicMakerJournal(path)).phase,"finalized");
      await assert.rejects(advanceAtomicMakerJournal(path,{
        ...identity,from:"finalized",to:"submitted",
      }),/Unsafe/);
    });
  });
});
test("proven expiry needs both positive height and authoritative absence",async()=>{
  await fixture(async(path,lock)=>{
    await withMakerJournalLock(lock,async()=>{
      await initializeAtomicMakerJournal(path,initial);
      await advanceAtomicMakerJournal(path,{...identity,from:"prepared",to:"submitted"});
      await advanceAtomicMakerJournal(path,{
        ...identity,from:"submitted",to:"expired",authoritativeAbsent:true,
        currentBlockHeight:501,
      });
      assert.equal((await readAtomicMakerJournal(path)).phase,"expired");
    });
  });
});
test("wrong signature, cluster, fingerprint, phase or torn record fail closed",async()=>{
  await fixture(async(path,lock)=>{
    await withMakerJournalLock(lock,async()=>{
      await initializeAtomicMakerJournal(path,initial);
      await assert.rejects(advanceAtomicMakerJournal(path,{
        ...identity,signature:"5".repeat(87),from:"prepared",to:"submitted",
      }),/mismatch/);
      await assert.rejects(advanceAtomicMakerJournal(path,{
        ...identity,genesisHash:"5".repeat(44),from:"prepared",to:"submitted",
      }),/mismatch/);
      await assert.rejects(advanceAtomicMakerJournal(path,{
        ...identity,simulationEvidence:{...simulationEvidence,transactionSha256:"0".repeat(64)},
        from:"prepared",to:"submitted",
      }),/differs/);
      await writeFile(path,"{torn");
      await assert.rejects(readAtomicMakerJournal(path),/Corrupt/);
    });
  });
});
test("stale .pending record blocks any replacement and lock is retained on failure",async()=>{
  await fixture(async(path,lock)=>{
    await assert.rejects(withMakerJournalLock(lock,async()=>{
      await initializeAtomicMakerJournal(path,initial);
      await writeFile(path+".pending","interrupted update");
      await advanceAtomicMakerJournal(path,{...identity,from:"prepared",to:"submitted"});
    }),/EEXIST/);
    assert.ok((await stat(lock)).size>0);
    assert.equal((await readAtomicMakerJournal(path)).phase,"prepared");
    assert.equal((await readFile(path+".pending","utf8")),"interrupted update");
  });
});
