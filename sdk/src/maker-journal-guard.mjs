// Host-only safety primitives for makers. NOT a submission service.
// A lock left after a crash deliberately blocks re-entry until an operator
// has reconciled pending journal state. Never auto-break a stale lock.
import {open,unlink,readFile} from "node:fs/promises";
import {dirname} from "node:path";
import {assertSimulatedBytes} from "./simulation-preflight.mjs";

const HEX64=/^[a-f0-9]{64}$/;
const BASE58=/^[1-9A-HJ-NP-Za-km-z]+$/;
function requireBase58(value,name,min,max){
  if(typeof value!=="string"||value.length<min||value.length>max||!BASE58.test(value))
    throw new TypeError("Invalid "+name);
}
// Decode the journaled base58 signature without relying on a caller-provided
// signature field. Check the first wire signature, which is the Solana tx ID.
const ALPHABET="123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
function decodeBase58(value){
  let n=0n;
  for(const c of value){
    const digit=ALPHABET.indexOf(c);
    if(digit<0)throw new Error("Invalid base58 transaction signature");
    n=n*58n+BigInt(digit);
  }
  const parts=[];
  while(n>0n){parts.push(Number(n&255n));n>>=8n;}
  for(const c of value){if(c!=="1")break;parts.push(0);}
  return Buffer.from(parts.reverse());
}
export function assertFirstWireSignature(transaction,signature){
  requireBase58(signature,"signature",80,90);
  const expected=decodeBase58(signature);
  if(expected.length!==64||expected.every(byte=>byte===0))
    throw new Error("Invalid 64-byte signed transaction signature");
  if(!transaction||typeof transaction.serialize!=="function")
    throw new TypeError("Pre-signed transaction required");
  const wire=Buffer.from(transaction.serialize());
  if(wire.length<65||wire.length>1232)
    throw new Error("Invalid serialized Solana transaction");
  // Solana shortvec compact signature count; reject non-canonical encodings.
  let count=0,shift=0,offset=0;
  while(true){
    if(offset>=3||offset>=wire.length)throw new Error("Invalid signature count");
    const digit=wire[offset++];
    count|=(digit&127)<<shift;
    if(!(digit&128)){
      if(offset>1&&(digit&127)===0)throw new Error("Noncanonical signature count");
      break;
    }
    shift+=7;
  }
  if(count<1||offset+count*64>=wire.length)
    throw new Error("Missing complete signed Solana message");
  const first=wire.subarray(offset,offset+64);
  if(!first.equals(expected))
    throw new Error("Journaled signature differs from first wire signature");
}
async function syncDirectory(path){
  const directory=await open(dirname(path),"r");
  try{await directory.sync();}finally{await directory.close();}
}

/** Hold a new, mode-0600 lock for an entire exclusive async operation.
 * Stale locks MUST be reconciled and removed manually, not stolen by PID/age.
 * Paths must live in a trusted directory and may not be attacker-controlled.
 */
export async function withMakerJournalLock(lockPath,operation){
  if(typeof lockPath!=="string"||!lockPath||typeof operation!=="function")
    throw new TypeError("Lock path and operation required");
  const handle=await open(lockPath,"wx",0o600);
  let complete=false;
  try{
    await handle.writeFile(JSON.stringify({version:1,createdAt:new Date().toISOString()})+"\n");
    await handle.sync();
    await syncDirectory(lockPath);
    const result=await operation();
    complete=true;
    return result;
  }finally{
    await handle.close();
    // Failure, crash or an inability to sync is NOT proof that no
    // operation was attempted; leave lock in place for manual recovery.
    if(complete){
      await unlink(lockPath);
      await syncDirectory(lockPath);
    }
  }
}

/** Write an immutable companion record for a prepared, *already signed*
 * transaction. Must be done under the exclusive journal lock and followed
 * by durable Rust MakerSession journal transitions in a trusted host process.
 * This standalone helper does not itself enforce either condition.
 */
export async function createPreparedIdentity(path,{
  transaction,simulationEvidence,signature,genesisHash,
  lastValidBlockHeight,snapshotSlot,
}){
  if(typeof path!=="string"||!path)throw new TypeError("Identity path required");
  assertSimulatedBytes(transaction,simulationEvidence);
  assertFirstWireSignature(transaction,signature);
  requireBase58(genesisHash,"genesisHash",32,50);
  if(!Number.isSafeInteger(lastValidBlockHeight)||lastValidBlockHeight<=0||
      !Number.isSafeInteger(snapshotSlot)||snapshotSlot<=0||
      simulationEvidence.snapshotSlot!==snapshotSlot||
      simulationEvidence.signaturesVerifiedByRpc!==true){
    throw new Error("Invalid prepared transaction context");
  }
  const record={
    version:1,signature,genesisHash,
    transactionSha256:simulationEvidence.transactionSha256,
    lastValidBlockHeight,snapshotSlot,
  };
  // create_new prevents overwriting an unresolved intent. The record must
  // only be retired after finality/recovery and independent state refresh.
  const handle=await open(path,"wx",0o600);
  try{
    await handle.writeFile(JSON.stringify(record)+"\n");
    await handle.sync();
  }finally{await handle.close();}
  await syncDirectory(path);
  return Object.freeze({...record});
}

/** Exact expected-record equality; malformed/missing files fail closed. */
export async function readPreparedIdentity(path,{
  signature,genesisHash,transaction,simulationEvidence,
}){
  const raw=await readFile(path,"utf8");
  const bytes=Buffer.from(raw,"utf8");
  if(bytes.length>2048||bytes.length===0)
    throw new Error("Invalid prepared identity record length");
  let stored;
  try{stored=JSON.parse(raw);}catch{throw new Error("Corrupt prepared identity record");}
  if(!stored||stored.version!==1||
      !Number.isSafeInteger(stored.lastValidBlockHeight)||stored.lastValidBlockHeight<=0||
      !Number.isSafeInteger(stored.snapshotSlot)||stored.snapshotSlot<=0||
      !HEX64.test(stored.transactionSha256)){
    throw new Error("Corrupt prepared identity record");
  }
  // Prove the saved record is canonical, not an unvalidated JSON extension
  // that another process might interpret differently.
  const keys=Object.keys(stored).sort();
  if(JSON.stringify(keys)!==JSON.stringify([
    "genesisHash","lastValidBlockHeight","signature","snapshotSlot",
    "transactionSha256","version",
  ]))throw new Error("Unexpected prepared identity fields");
  if(stored.signature!==signature||stored.genesisHash!==genesisHash)
    throw new Error("Prepared transaction signature or cluster mismatch");
  assertSimulatedBytes(transaction,simulationEvidence);
  assertFirstWireSignature(transaction,signature);
  if(stored.transactionSha256!==simulationEvidence.transactionSha256||
      stored.snapshotSlot!==simulationEvidence.snapshotSlot){
    throw new Error("Prepared identity differs from simulated transaction");
  }
  return Object.freeze(stored);
}
