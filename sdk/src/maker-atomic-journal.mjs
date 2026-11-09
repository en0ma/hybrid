// Experimental, non-submitting *single-record* transaction identity journal.
// Unlike the legacy Rust HYBMMJ01 plus JSON companion, each state and its
// exact transaction identity are committed together in one atomic replacement.
// The entire workflow MUST hold withMakerJournalLock. No automatic migration,
// sending, signing, or legacy record reconciliation is provided.
import {open,readFile,rename} from "node:fs/promises";
import {dirname,basename,join} from "node:path";
import {createHash} from "node:crypto";
import {assertSimulatedBytes} from "./simulation-preflight.mjs";

const KEYS=["genesisHash","lastConfirmedSlot","lastValidBlockHeight","phase",
  "signature","snapshotSlot","transactionSha256","version"];
const BASE58=/^[1-9A-HJ-NP-Za-km-z]+$/;
const HEX=/^[a-f0-9]{64}$/;
const isSlot=n=>Number.isSafeInteger(n)&&n>=0;
function canonical(value){
  if(!value||typeof value!=="object"||Array.isArray(value)||
      JSON.stringify(Object.keys(value).sort())!==JSON.stringify(KEYS))
    throw new Error("Unexpected or missing atomic journal fields");
  if(value.version!==2||!["prepared","submitted","finalized","expired"].includes(value.phase)||
      !isSlot(value.lastConfirmedSlot)||!isSlot(value.snapshotSlot)||
      !isSlot(value.lastValidBlockHeight)||value.snapshotSlot===0||
      value.lastValidBlockHeight===0||
      (["prepared","submitted"].includes(value.phase)&&
       value.snapshotSlot<value.lastConfirmedSlot)||
      typeof value.signature!=="string"||value.signature.length<80||
      value.signature.length>90||!BASE58.test(value.signature)||
      typeof value.genesisHash!=="string"||value.genesisHash.length<32||
      value.genesisHash.length>50||!BASE58.test(value.genesisHash)||
      typeof value.transactionSha256!=="string"||!HEX.test(value.transactionSha256))
    throw new Error("Invalid atomic journal state");
  return value;
}
function checksum(bytes){return createHash("sha256").update(bytes).digest("hex");}
function envelope(state){
  const data=Buffer.from(JSON.stringify(canonical(state)));
  return JSON.stringify({state,digest:checksum(data)})+"\n";
}
export async function readAtomicMakerJournal(path){
  const raw=await readFile(path,"utf8");
  if(Buffer.byteLength(raw)>4096)throw new Error("Oversized maker journal");
  let parsed;
  try{parsed=JSON.parse(raw);}catch{throw new Error("Corrupt maker journal");}
  if(!parsed||JSON.stringify(Object.keys(parsed).sort())!==
      JSON.stringify(["digest","state"])||typeof parsed.digest!=="string")
    throw new Error("Corrupt maker journal envelope");
  const state=canonical(parsed.state);
  if(checksum(Buffer.from(JSON.stringify(state)))!==parsed.digest)
    throw new Error("Maker journal integrity failure");
  return Object.freeze({...state});
}
/** Durably replace one complete record. A .pending file is never ignored;
 * it blocks writes until a trusted operator reconciles interrupted work.
 * Do not call without the exclusive maker lock for the whole operation.
 */
async function persist(path,state,{initialize=false}={}){
  const pending=join(dirname(path),basename(path)+".pending");
  const handle=await open(pending,"wx",0o600);
  try{
    await handle.writeFile(envelope(state));
    await handle.sync();
  }finally{await handle.close();}
  if(initialize){
    // create_new on the final path prevents explicit initialization from
    // overwriting any prior state; leave .pending on any failure.
    const reserved=await open(path,"wx",0o600);
    await reserved.close();
  }
  await rename(pending,path);
  const directory=await open(dirname(path),"r");
  try{await directory.sync();}finally{await directory.close();}
  return Object.freeze({...state});
}
/** Initialize ONLY after independent proof that no old maker state exists.
 * Caller is responsible for checking legacy Rust journals and stray intents.
 */
export async function initializeAtomicMakerJournal(path,{
  transaction,simulationEvidence,signature,genesisHash,
  lastValidBlockHeight,snapshotSlot,lastConfirmedSlot=0,
}){
  assertSimulatedBytes(transaction,simulationEvidence);
  if(simulationEvidence.snapshotSlot!==snapshotSlot||
      simulationEvidence.signaturesVerifiedByRpc!==true)
    throw new Error("Unverified signed transaction snapshot");
  return persist(path,{
    version:2,phase:"prepared",signature,genesisHash,
    transactionSha256:simulationEvidence.transactionSha256,
    lastValidBlockHeight,snapshotSlot,lastConfirmedSlot,
  },{initialize:true});
}
/** The caller must independently attest the same signed wire, exact cluster,
 * simulation and a pre-broadcast durable signature. This only updates disk.
 */
export async function advanceAtomicMakerJournal(path,{
  from,to,signature,genesisHash,transaction,simulationEvidence,
  finalizedSlot=null,authoritativeAbsent=false,currentBlockHeight=null,
}){
  const old=await readAtomicMakerJournal(path);
  if(old.phase!==from||old.signature!==signature||old.genesisHash!==genesisHash)
    throw new Error("Maker journal phase, signature or cluster mismatch");
  assertSimulatedBytes(transaction,simulationEvidence);
  if(old.transactionSha256!==simulationEvidence.transactionSha256||
      old.snapshotSlot!==simulationEvidence.snapshotSlot)
    throw new Error("Maker journal transaction fingerprint mismatch");
  const transitions={prepared:"submitted",submitted:"finalized"};
  if(!(transitions[from]===to||(from==="submitted"&&to==="expired")))
    throw new Error("Unsafe maker journal transition");
  const next={...old,phase:to};
  if(to==="finalized"){
    if(!isSlot(finalizedSlot)||finalizedSlot<old.snapshotSlot)
      throw new Error("Invalid finalized transaction slot");
    next.lastConfirmedSlot=finalizedSlot;
  }
  if(to==="expired"&&(!authoritativeAbsent||!isSlot(currentBlockHeight)||
      currentBlockHeight<=old.lastValidBlockHeight))
    throw new Error("Expiry requires proven absence beyond blockhash height");
  return persist(path,next);
}
