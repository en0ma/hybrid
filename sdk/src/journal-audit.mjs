// Read-only audit of the original Rust HYBMMJ01 journal and its immutable
// signed-wire companion. This NEVER alters journal state or permits sending.
import {readFile} from "node:fs/promises";
import {readPreparedIdentity} from "./maker-journal-guard.mjs";
const MAGIC=Buffer.from("HYBMMJ01","ascii");
const ALPHABET="123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const MASK=(1n<<64n)-1n;
function checksum(bytes){
  let hash=0xcbf29ce484222325n;
  for(const byte of bytes)hash=((hash^BigInt(byte))*0x100000001b3n)&MASK;
  return hash;
}
function encodeBase58(bytes){
  let n=0n;
  for(const byte of bytes)n=n*256n+BigInt(byte);
  let out="";
  while(n>0n){out=ALPHABET[Number(n%58n)]+out;n/=58n;}
  for(const b of bytes){if(b!==0)break;out="1"+out;}
  return out;
}
/** Decode HYBMMJ01 from program/src/client_journal.rs.
 * This intentionally accepts ONLY Submitted records; an absent, Prepared,
 * Idle, Expired or Confirmed record is not a pending wire identity.
 */
export function decodeSubmittedJournal(bytes){
  if(!(bytes instanceof Uint8Array)||bytes.length!==104)
    throw new Error("Invalid maker journal length");
  const buffer=Buffer.from(bytes);
  if(!buffer.subarray(0,8).equals(MAGIC))
    throw new Error("Invalid maker journal magic");
  if(buffer.readBigUInt64LE(96)!==checksum(buffer.subarray(0,96)))
    throw new Error("Invalid maker journal checksum");
  if(buffer.subarray(89,96).some(b=>b!==0))
    throw new Error("Invalid maker journal reserved fields");
  if(buffer[16]!==2)
    throw new Error("Maker journal is not a submitted pending transaction");
  const signature=buffer.subarray(25,89);
  if(signature.every(b=>b===0))
    throw new Error("Maker journal signature is zero");
  const height=buffer.readBigUInt64LE(17);
  const confirmedSlot=buffer.readBigUInt64LE(8);
  if(height===0n||height>BigInt(Number.MAX_SAFE_INTEGER)||
      confirmedSlot>BigInt(Number.MAX_SAFE_INTEGER))
    throw new Error("Invalid maker journal height or slot");
  return Object.freeze({
    signature:encodeBase58(signature),
    lastValidBlockHeight:Number(height),
    lastConfirmedSlot:Number(confirmedSlot),
    phase:"submitted",
  });
}

/** Check the on-disk pending Rust journal against the immutable, exact-wire
 * prepared identity before invoking any recovery decisions. Both files must
 * be protected by the same exclusive lock in the trusted host environment.
 * Does NOT prove the originally prepared snapshot (not stored in v1 Rust
 * journal); this is a fail-closed consistency audit, not an execution permit.
 */
export async function auditPendingMakerJournal({
  journalPath,identityPath,transaction,simulationEvidence,expectedGenesisHash,
}){
  const bytes=await readFile(journalPath);
  const submitted=decodeSubmittedJournal(bytes);
  const record=await readPreparedIdentity(identityPath,{
    signature:submitted.signature,
    genesisHash:expectedGenesisHash,
    transaction,simulationEvidence,
  });
  if(record.lastValidBlockHeight!==submitted.lastValidBlockHeight)
    throw new Error("Maker journal and signed intent expiry mismatch");
  if(record.snapshotSlot<submitted.lastConfirmedSlot)
    throw new Error("Signed intent snapshot predates prior finality");
  return Object.freeze({
    phase:"submitted",signature:submitted.signature,
    lastValidBlockHeight:submitted.lastValidBlockHeight,
    snapshotSlot:record.snapshotSlot,
    transactionSha256:record.transactionSha256,
    mode:"audit-only",
  });
}
