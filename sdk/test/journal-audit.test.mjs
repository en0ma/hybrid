import test from "node:test";
import assert from "node:assert/strict";
import {mkdtemp,rm,writeFile} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join} from "node:path";
import {createHash} from "node:crypto";
import {decodeSubmittedJournal,auditPendingMakerJournal} from "../src/journal-audit.mjs";
import {createPreparedIdentity} from "../src/maker-journal-guard.mjs";
const signatureBytes=Buffer.alloc(64,3);
const transactionBytes=Buffer.concat([Buffer.from([1]),signatureBytes,Buffer.from([9,8,7])]);
const transaction={serialize(){return Buffer.from(transactionBytes);}};
const ALPHABET="123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
function to58(bytes){
  let n=0n;for(const b of bytes)n=n*256n+BigInt(b);
  let s="";while(n>0n){s=ALPHABET[Number(n%58n)]+s;n/=58n;}
  for(const b of bytes){if(b!==0)break;s="1"+s;}return s;
}
const signature=to58(signatureBytes),genesisHash="4".repeat(44);
const simulationEvidence={
  mode:"simulation-only",snapshotSlot:100,simulationSlot:102,
  signaturesVerifiedByRpc:true,
  transactionSha256:createHash("sha256").update(transactionBytes).digest("hex"),
};
function journal({tag=2,height=200,previousSlot=90,signature=signatureBytes}={}){
  const record=Buffer.alloc(104);
  record.write("HYBMMJ01",0,"ascii");
  record.writeBigUInt64LE(BigInt(previousSlot),8);
  record[16]=tag;
  record.writeBigUInt64LE(BigInt(height),17);
  Buffer.from(signature).copy(record,25);
  let hash=0xcbf29ce484222325n;
  for(const b of record.subarray(0,96))
    hash=((hash^BigInt(b))*0x100000001b3n)&((1n<<64n)-1n);
  record.writeBigUInt64LE(hash,96);
  return record;
}
async function fixture(callback){
  const folder=await mkdtemp(join(tmpdir(),"hybrid-journal-audit-"));
  try{await callback(folder);}finally{await rm(folder,{recursive:true,force:true});}
}
test("decode exact Rust pending journal and reject invalid phase or corruption",()=>{
  assert.deepEqual(decodeSubmittedJournal(journal()),{
    signature,lastValidBlockHeight:200,lastConfirmedSlot:90,phase:"submitted",
  });
  assert.throws(()=>decodeSubmittedJournal(journal({tag:1})),/not a submitted/);
  assert.throws(()=>decodeSubmittedJournal(journal({height:0})),/height or slot/);
  assert.throws(()=>decodeSubmittedJournal(journal({signature:Buffer.alloc(64)})),/zero/);
  const corrupt=journal();corrupt[40]^=1;
  assert.throws(()=>decodeSubmittedJournal(corrupt),/checksum/);
  assert.throws(()=>decodeSubmittedJournal(journal().subarray(0,100)),/length/);
});
test("only exact matching journal, companion, cluster and wire identity pass audit",async()=>{
  await fixture(async folder=>{
    const journalPath=join(folder,"maker.journal"),identityPath=join(folder,"intent.json");
    await writeFile(journalPath,journal());
    await createPreparedIdentity(identityPath,{
      transaction,simulationEvidence,signature,genesisHash,
      snapshotSlot:100,lastValidBlockHeight:200,
    });
    const args={journalPath,identityPath,transaction,simulationEvidence,expectedGenesisHash:genesisHash};
    const result=await auditPendingMakerJournal(args);
    assert.equal(result.mode,"audit-only");
    assert.equal(result.signature,signature);
    await assert.rejects(auditPendingMakerJournal({...args,expectedGenesisHash:"5".repeat(44)}),/mismatch/);
    await writeFile(journalPath,journal({height:201}));
    await assert.rejects(auditPendingMakerJournal(args),/expiry mismatch/);
    await writeFile(journalPath,journal({previousSlot:101}));
    await assert.rejects(auditPendingMakerJournal(args),/predates prior finality/);
    await writeFile(journalPath,journal({signature:Buffer.alloc(64,4)}));
    await assert.rejects(auditPendingMakerJournal(args),/signature or cluster mismatch/);
  });
});
