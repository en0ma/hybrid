import test from "node:test";
import assert from "node:assert/strict";
import {mkdtemp,readFile,rm,stat,writeFile} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join} from "node:path";
import {createHash} from "node:crypto";
import {
  withMakerJournalLock,createPreparedIdentity,readPreparedIdentity,
} from "../src/maker-journal-guard.mjs";

const bytes=Buffer.from([1,3,5,7]);
const transaction={serialize(){return Buffer.from(bytes);}};
const simulationEvidence={
  mode:"simulation-only",snapshotSlot:100,simulationSlot:102,
  signaturesVerifiedByRpc:true,
  transactionSha256:createHash("sha256").update(bytes).digest("hex"),
};
const signature="3".repeat(87),genesisHash="4".repeat(44);
const parameters={
  transaction,simulationEvidence,signature,genesisHash,
  lastValidBlockHeight:150,snapshotSlot:100,
};
async function fixture(callback){
  const root=await mkdtemp(join(tmpdir(),"hybrid-lock-test-"));
  try{await callback(root);}finally{await rm(root,{recursive:true,force:true});}
}
test("exclusive lock rejects overlapping maker access and releases after success",async()=>{
  await fixture(async root=>{
    const lock=join(root,"maker.lock");
    const result=await withMakerJournalLock(lock,async()=>{
      await assert.rejects(withMakerJournalLock(lock,async()=>{}),/EEXIST/);
      assert.ok((await stat(lock)).size>0);
      return 7;
    });
    assert.equal(result,7);
    await assert.rejects(stat(lock),/ENOENT/);
    await withMakerJournalLock(lock,async()=>{});
  });
});
test("failed critical operation leaves lock for manual reconciliation",async()=>{
  await fixture(async root=>{
    const lock=join(root,"maker.lock");
    await assert.rejects(withMakerJournalLock(lock,async()=>{
      throw new Error("Unknown broadcast result");
    }),/Unknown broadcast/);
    assert.ok((await stat(lock)).size>0);
    await assert.rejects(withMakerJournalLock(lock,async()=>{}),/EEXIST/);
  });
});
test("immutable prepared intent survives restart and rejects changed signatures",async()=>{
  await fixture(async root=>{
    const path=join(root,"signed-intent.json");
    const record=await createPreparedIdentity(path,parameters);
    assert.equal(record.transactionSha256,simulationEvidence.transactionSha256);
    assert.deepEqual(await readPreparedIdentity(path,parameters),record);
    await assert.rejects(createPreparedIdentity(path,parameters),/EEXIST/);
    await assert.rejects(readPreparedIdentity(path,{
      ...parameters,signature:"5".repeat(87),
    }),/mismatch/);
    await assert.rejects(readPreparedIdentity(path,{
      ...parameters,genesisHash:"5".repeat(44),
    }),/mismatch/);
    await assert.rejects(readPreparedIdentity(path,{
      ...parameters,simulationEvidence:{
        ...simulationEvidence,transactionSha256:"0".repeat(64),
      },
    }),/differs/);
    const {mode,...rest}=simulationEvidence;
    await assert.rejects(createPreparedIdentity(join(root,"second"),{
      ...parameters,simulationEvidence:rest,
    }),/simulation evidence/);
  });
});
test("missing, corrupted, and extended intent records fail closed",async()=>{
  await fixture(async root=>{
    const path=join(root,"intent.json");
    await assert.rejects(readPreparedIdentity(path,parameters),/ENOENT/);
    await writeFile(path,"broken");
    await assert.rejects(readPreparedIdentity(path,parameters),/Corrupt/);
    await writeFile(path,JSON.stringify({
      version:1,transactionSha256:simulationEvidence.transactionSha256,
      signature,genesisHash,lastValidBlockHeight:150,snapshotSlot:100,
      unsafeAllowRetry:true,
    }));
    await assert.rejects(readPreparedIdentity(path,parameters),/Unexpected/);
  });
});
