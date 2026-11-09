// PURE research-only Merkle commitment for frozen pro-rata claim leaves.
// A proof commits exact epoch, roster position and six asset amounts.
// CRITICAL: the root MUST be authenticated from a trusted, fully audited
// epoch sealer; a Merkle proof alone does NOT prove pro-rata correctness,
// maker signer ownership, escrow solvency, or authorize SPL transfers.
import {createHash} from "node:crypto";
import {frozenMakerClaim,auditFrozenProRataEpoch} from "./pro-rata-epoch.mjs";

// Versioned binary Merkle ABI, v1. Never hash JSON for on-chain proofs.
// Leaf preimage: "HYBRID_LAZY_MAKER_LEAF_V1\0" | 32-byte epoch digest |
// u32LE roster index | u8 ASCII maker ID length | maker ID bytes |
// u64LE weight | six u64LE asset amounts in ASSETS order.
// Node preimage: "HYBRID_LAZY_MAKER_NODE_V1\0" | left[32] | right[32].
const ASSETS=["baseDebit","baseCredit","quoteDebit","quoteCredit","baseFee","quoteFee"];
const HEX=/^[0-9a-f]{64}$/;
const ID=/^[A-Za-z0-9_-]{1,128}$/;
const MAX64=(1n<<64n)-1n;
const LEAF_DOMAIN=Buffer.from("HYBRID_LAZY_MAKER_LEAF_V1\0","utf8");
const NODE_DOMAIN=Buffer.from("HYBRID_LAZY_MAKER_NODE_V1\0","utf8");
const sha=(bytes)=>createHash("sha256").update(bytes).digest("hex");
const eq=(a,b)=>JSON.stringify(a)===JSON.stringify(b);
function u64(value,name,{positive=false}={}){
  if(typeof value!=="string"||!/^(0|[1-9][0-9]*)$/.test(value))
    throw new Error("Invalid canonical integer: "+name);
  const n=BigInt(value);
  if(n>MAX64||(positive&&n===0n))throw new Error("Out-of-range u64: "+name);
  const b=Buffer.alloc(8);b.writeBigUInt64LE(n);return b;
}
function safeLeaf(leaf){
  if(!leaf||typeof leaf!=="object"||Array.isArray(leaf)||
    !eq(Object.keys(leaf).sort(),["index","makerId","weight","claim"].sort())||
    !Number.isSafeInteger(leaf.index)||leaf.index<0||leaf.index>4095||
    typeof leaf.makerId!=="string"||!ID.test(leaf.makerId)||
    !leaf.claim||typeof leaf.claim!=="object"||Array.isArray(leaf.claim)||
    !eq(Object.keys(leaf.claim).sort(),[...ASSETS].sort()))
    throw new Error("Invalid Merkle claim leaf");
  u64(leaf.weight,"maker weight",{positive:true});
  for(const asset of ASSETS)u64(leaf.claim[asset],asset);
  return leaf;
}
function leafHash(epochCommitment,index,makerId,weight,claim){
  if(!HEX.test(epochCommitment))throw new Error("Invalid epoch commitment");
  const leaf=safeLeaf({index,makerId,weight,claim});
  const idx=Buffer.alloc(4);idx.writeUInt32LE(leaf.index);
  const maker=Buffer.from(leaf.makerId,"ascii");
  return sha(Buffer.concat([LEAF_DOMAIN,Buffer.from(epochCommitment,"hex"),idx,
    Buffer.from([maker.length]),maker,u64(leaf.weight,"maker weight",{positive:true}),
    ...ASSETS.map(a=>u64(leaf.claim[a],a))]));
}
function nodeHash(left,right){
  if(!HEX.test(left)||!HEX.test(right))throw new Error("Invalid Merkle node");
  return sha(Buffer.concat([NODE_DOMAIN,Buffer.from(left,"hex"),Buffer.from(right,"hex")]));
}
/** Test-vector utilities for the v1 ABI. Cryptographic membership only.
 * These exports never authenticate a canonical Solana epoch or authorize pay.
 */
export function hashFrozenMakerLeaf(epochCommitment,leaf){
  safeLeaf(leaf);
  return leafHash(epochCommitment,leaf.index,leaf.makerId,leaf.weight,leaf.claim);
}
export function hashFrozenMakerNode(left,right){return nodeHash(left,right);}

function buildLevels(hashes){
  const levels=[hashes];
  while(levels.at(-1).length>1){
    const children=levels.at(-1);
    const parent=[];
    for(let i=0;i<children.length;i+=2){
      // Duplicate the terminal child on odd-length trees; index and exact
      // length checks in the verifier prevent accepting phantom leaves.
      parent.push(nodeHash(children[i],children[i+1]??children[i]));
    }
    levels.push(parent);
  }
  return levels;
}
export function sealFrozenMakerClaims(epoch){
  auditFrozenProRataEpoch(epoch);
  const leaves=epoch.makers.map((maker,index)=>Object.freeze({
    index,makerId:maker.id,weight:maker.weight,
    claim:frozenMakerClaim(epoch,maker.id),
  }));
  const levels=buildLevels(leaves.map(l=>
    leafHash(epoch.commitment,l.index,l.makerId,l.weight,l.claim)));
  const root=levels.at(-1)[0];
  return Object.freeze({
    mode:"research-merkle-only",root,epochCommitment:epoch.commitment,
    leafCount:leaves.length,
    leaves:Object.freeze(leaves),
  });
}
/** Build a bounded O(log N) authentication path from the audited epoch. */
export function proveFrozenMakerClaim(seal,index){
  if(!seal||seal.mode!=="research-merkle-only"||
    !Number.isSafeInteger(index)||index<0||index>=seal.leafCount||
    seal.leafCount!==seal.leaves?.length||seal.leafCount>4096)
    throw new Error("Invalid sealed maker claim index");
  const leaves=seal.leaves.map((l,i)=>{
    if(l.index!==i)throw new Error("Sealed leaf roster index mismatch");
    safeLeaf(l);
    return leafHash(seal.epochCommitment,l.index,l.makerId,l.weight,l.claim);
  });
  const levels=buildLevels(leaves);
  if(levels.at(-1)[0]!==seal.root)throw new Error("Sealed epoch root mismatch");
  const siblings=[];
  let cursor=index;
  for(let level=0;level<levels.length-1;level++){
    const nodes=levels[level];
    siblings.push(nodes[cursor^1]??nodes[cursor]);
    cursor=Math.floor(cursor/2);
  }
  return Object.freeze({
    epochCommitment:seal.epochCommitment,root:seal.root,
    leafCount:seal.leafCount,leaf:seal.leaves[index],
    siblings:Object.freeze(siblings),
  });
}
/** Verify against an independently authenticated expected root and epoch ID.
 * This is cryptographic membership only, NOT settlement authorization.
 */
export function verifyFrozenMakerProof(proof,{
  expectedRoot,expectedEpochCommitment,expectedMakerId,
}){
  if(!proof||!HEX.test(expectedRoot)||!HEX.test(expectedEpochCommitment)||
      proof.root!==expectedRoot||proof.epochCommitment!==expectedEpochCommitment||
      !Number.isSafeInteger(proof.leafCount)||proof.leafCount<1||
      proof.leafCount>4096||!Array.isArray(proof.siblings)||
      typeof expectedMakerId!=="string"||!ID.test(expectedMakerId))
    throw new Error("Untrusted frozen maker proof context");
  const leaf=safeLeaf(proof.leaf);
  if(leaf.index>=proof.leafCount||leaf.makerId!==expectedMakerId)
    throw new Error("Wrong maker or out-of-range leaf");
  let count=proof.leafCount,index=leaf.index;
  let hash=leafHash(proof.epochCommitment,index,leaf.makerId,leaf.weight,leaf.claim);
  for(const sibling of proof.siblings){
    if(count<=1||typeof sibling!=="string"||!HEX.test(sibling))
      throw new Error("Invalid maker Merkle path length or node");
    if(index%2===0){
      if(index+1>=count&&sibling!==hash)
        throw new Error("Noncanonical odd-leaf sibling");
      hash=nodeHash(hash,sibling);
    }else hash=nodeHash(sibling,hash);
    count=Math.ceil(count/2);
    index=Math.floor(index/2);
  }
  if(count!==1||hash!==expectedRoot)
    throw new Error("Invalid maker Merkle inclusion proof");
  return Object.freeze({
    mode:"membership-only",epochCommitment:proof.epochCommitment,
    makerId:leaf.makerId,index:leaf.index,claim:Object.freeze({...leaf.claim}),
    canWithdraw:false,
  });
}
