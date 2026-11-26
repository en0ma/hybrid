// Non-submitting settlement transaction lifecycle. The caller supplies a
// trusted Solana adapter; no private keys or send methods are used here.
import {createHash} from "node:crypto";
import {open,readFile} from "node:fs/promises";
import {dirname} from "node:path";
import {keyBytes,buildSettlementClaim} from "./settlement.mjs";
import {loadSettlementEpochs} from "./settlement-rpc.mjs";
import {simulateMakerTransaction,assertSimulatedBytes} from "./simulation-preflight.mjs";
import {assertFirstWireSignature,withMakerJournalLock} from "./maker-journal-guard.mjs";
import {observeSignatureStatus} from "./signature-status.mjs";

const MAX64=(1n<<64n)-1n;
const hex=k=>keyBytes(k).toString("hex");
const sha=b=>createHash("sha256").update(b).digest("hex");
const slotOk=n=>Number.isSafeInteger(n)&&n>0;
const HEX=/^[a-f0-9]{64}$/;
const B58=/^[1-9A-HJ-NP-Za-km-z]+$/;
const ALT_PROGRAM="AddressLookupTab1e1111111111111111111111111";

// Decode via the trusted web3 adapter, but independently authenticate the
// owner, activation boundary, authority and exact lookup indices.
export async function loadSettlementLookupTable(connection,{adapter,address,authority,
  requiredAddresses,minContextSlot=0,commitment="confirmed"}){
  if(!Number.isSafeInteger(minContextSlot)||minContextSlot<0||
    !Array.isArray(requiredAddresses)||!requiredAddresses.length||
    requiredAddresses.length>256||new Set(requiredAddresses.map(hex)).size!==requiredAddresses.length)
    throw new Error("Invalid lookup request");
  const response=await connection.getAccountInfoAndContext(address,{minContextSlot,commitment});
  const slot=response?.context?.slot,info=response?.value;
  if(!slotOk(slot)||slot<minContextSlot||!info||info.executable||
    String(info.owner)!==ALT_PROGRAM||!(info.data instanceof Uint8Array))
    throw new Error("Missing, foreign or stale lookup table");
  const state=adapter.deserializeLookupTable(info.data);
  if(BigInt(state.deactivationSlot)!==MAX64||
    !Number.isSafeInteger(state.lastExtendedSlot)||state.lastExtendedSlot>=slot||
    !Array.isArray(state.addresses)||state.addresses.length>256||
    new Set(state.addresses.map(hex)).size!==state.addresses.length)
    throw new Error("Inactive or warming lookup table");
  if(authority===null?state.authority!=null:state.authority==null||hex(state.authority)!==hex(authority))
    throw new Error("Lookup authority mismatch");
  const indices=requiredAddresses.map(k=>state.addresses.findIndex(a=>hex(a)===hex(k)));
  if(indices.some(i=>i<0))throw new Error("Missing lookup address");
  return {address,slot,state,indices,requiredAddresses:[...requiredAddresses]};
}

// Return an advisory lifecycle plan, not create/extend/deactivate instructions
// or authorization. Newly appended entries require a later context slot.
export function planLookupExtension(existing,requiredAddresses){
  if(!Array.isArray(existing)||!Array.isArray(requiredAddresses))throw new Error("Invalid lookup addresses");
  const seen=new Set(existing.map(hex));
  if(seen.size!==existing.length)throw new Error("Duplicate lookup entry");
  const missing=[];
  for(const k of requiredAddresses)if(!seen.has(hex(k))){seen.add(hex(k));missing.push(k);}
  if(seen.size>256)throw new Error("Lookup table capacity exceeded");
  const batches=[];
  for(let i=0;i<missing.length;i+=20)batches.push(missing.slice(i,i+20));
  return {mode:"advisory-only",batches,requiresActivationSlot:true};
}

// The trusted adapter must parse the actual serialized message, including
// resolved v0 keys, rather than reading caller-supplied instruction metadata.
export function assertSettlementWire(adapter,transaction,instruction,lookupTables=[]){
  const bytes=Buffer.from(transaction.serialize());
  if(!bytes.length||bytes.length>1232)throw new Error("Settlement packet exceeds wire limit");
  const decoded=adapter.decodeSignedInstructions(bytes,lookupTables);
  if(!Array.isArray(decoded))throw new Error("Invalid decoded transaction");
  const expected=decoded.filter(ix=>hex(ix.programId)===hex(instruction.programId));
  if(expected.length!==1)throw new Error("Expected exactly one settlement instruction");
  const ix=expected[0];
  if(!Buffer.from(ix.data).equals(Buffer.from(instruction.data))||
    ix.keys.length!==instruction.keys.length||ix.keys.some((k,i)=>{
      const e=instruction.keys[i];
      return hex(k.pubkey)!==hex(e.pubkey)||(e.isSigner&&!k.isSigner)||(e.isWritable&&!k.isWritable);
    }))throw new Error("Signed settlement instruction mismatch");
  // Only compute-budget instructions may accompany settlement.
  if(decoded.some(x=>hex(x.programId)!==hex(instruction.programId)&&
    String(x.programId)!=="ComputeBudget111111111111111111111111111111"))
    throw new Error("Unexpected companion instruction");
  return sha(bytes);
}
export async function simulateSettlement(connection,{adapter,transaction,prepared,lookupTables=[],
  currentSlot,maxLagSlots=4,maxUnits=400000}){
  if(!slotOk(prepared?.slot)||prepared.mode!=="unsigned-only")throw new Error("Invalid prepared settlement");
  if(lookupTables.some(t=>!slotOk(t.slot)||t.slot<prepared.slot||t.slot>currentSlot))
    throw new Error("Stale lookup table snapshot");
  const digest=assertSettlementWire(adapter,transaction,prepared.instruction,lookupTables);
  const evidence=await simulateMakerTransaction(connection,{transaction,minContextSlot:prepared.slot,
    currentSlot,maxLagSlots,maxUnits});
  if(evidence.transactionSha256!==digest||
    assertSettlementWire(adapter,transaction,prepared.instruction,lookupTables)!==digest)
    throw new Error("Settlement wire changed");
  return evidence;
}

// Immutable, create-new intent per claim; retain unknown/failed signatures.
// A crash or pre-existing lock requires manual reconciliation.
function claimIdentity(epoch,proof){
  if(!HEX.test(epoch.marketHex)||!HEX.test(epoch.programHex)||!HEX.test(epoch.genesisHex)||
    !HEX.test(epoch.commitment)||!HEX.test(epoch.claimRoot)||!HEX.test(proof.makerHex)||
    !Number.isInteger(proof.seat)||proof.seat<0||proof.seat>=epoch.makerCount)
    throw new Error("Invalid claim identity");
  return {marketHex:epoch.marketHex,programHex:epoch.programHex,genesisHex:epoch.genesisHex,
    epochId:String(epoch.epochId),commitment:epoch.commitment,claimRoot:epoch.claimRoot,
    makerHex:proof.makerHex,seat:proof.seat};
}
export async function journalSettlementClaim(path,{adapter,programId,accounts,epoch,proof,
  transaction,simulationEvidence,signature,genesisHash,lastValidBlockHeight,lookupTables=[]}){
  const instruction=buildSettlementClaim(programId,accounts,epoch,proof);
  assertSimulatedBytes(transaction,simulationEvidence);
  assertFirstWireSignature(transaction,signature);
  if(assertSettlementWire(adapter,transaction,instruction,lookupTables)!==simulationEvidence.transactionSha256||
    simulationEvidence.signaturesVerifiedByRpc!==true||!slotOk(simulationEvidence.snapshotSlot)||
    !slotOk(lastValidBlockHeight)||typeof genesisHash!=="string"||!B58.test(genesisHash)||
    genesisHash.length<32||genesisHash.length>50||
    hex(adapter.fromBase58(genesisHash))!==epoch.genesisHex)
    throw new Error("Unverified claim transaction context");
  const record={version:1,...claimIdentity(epoch,proof),signature,genesisHash,lastValidBlockHeight,
    snapshotSlot:simulationEvidence.snapshotSlot,transactionSha256:simulationEvidence.transactionSha256};
  return withMakerJournalLock(path+".lock",async()=>{
    const bytes=JSON.stringify(record);
    const h=await open(path,"wx",0o600);
    try{await h.writeFile(JSON.stringify({record,digest:sha(bytes)})+"\n");await h.sync();}
    finally{await h.close();}
    const d=await open(dirname(path),"r");try{await d.sync();}finally{await d.close();}
    return Object.freeze(record);
  });
}
export async function readSettlementClaimJournal(path){
  const bytes=await readFile(path,"utf8");
  if(Buffer.byteLength(bytes)>4096)throw new Error("Oversized claim journal");
  const envelope=JSON.parse(bytes),r=envelope.record;
  if(!r||sha(JSON.stringify(r))!==envelope.digest||r.version!==1||
    ![r.marketHex,r.programHex,r.genesisHex,r.commitment,r.claimRoot,r.makerHex,r.transactionSha256].every(x=>HEX.test(x))||
    !/^(0|[1-9][0-9]*)$/.test(r.epochId)||BigInt(r.epochId)>MAX64||
    !Number.isInteger(r.seat)||r.seat<0||r.seat>=4096||
    !slotOk(r.snapshotSlot)||!slotOk(r.lastValidBlockHeight)||
    typeof r.signature!=="string"||!B58.test(r.signature)||r.signature.length<80||r.signature.length>90||
    typeof r.genesisHash!=="string"||!B58.test(r.genesisHash)||r.genesisHash.length<32||r.genesisHash.length>50)
    throw new Error("Corrupt claim journal");
  return Object.freeze(r);
}
export async function recoverSettlementClaim(connection,{adapter,path,programId,market}){
  const r=await readSettlementClaimJournal(path);
  if(hex(programId)!==r.programHex||hex(market)!==r.marketHex||
    hex(adapter.fromBase58(r.genesisHash))!==r.genesisHex)throw new Error("Claim journal domain mismatch");
  const status=await observeSignatureStatus(connection,{signature:r.signature,
    expectedGenesisHash:r.genesisHash,lastValidBlockHeight:r.lastValidBlockHeight});
  const result=await loadSettlementEpochs(connection,{adapter,programId,market,genesisHex:r.genesisHex,
    epochIds:[BigInt(r.epochId)],minContextSlot:Math.max(r.snapshotSlot,status.slot??0),
    commitment:"finalized"});
  const e=result.epochs[0];
  if(e.commitment!==r.commitment||e.claimRoot!==r.claimRoot||r.seat>=e.makerCount)
    throw new Error("Claim journal epoch mismatch");
  const claimed=e.claimed.includes(r.seat);
  if(status.state==="finalized-success"&&!claimed)throw new Error("Finalized claim missing from bitmap");
  return {state:claimed?"claimed":status.state,claimed,slot:result.slot,status,
    canResubmit:false,requiresOperatorReview:!claimed};
}
