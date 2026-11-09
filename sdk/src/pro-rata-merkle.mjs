// PURE research-only Merkle commitment for frozen pro-rata claim leaves.
// A proof commits exact epoch, roster position and six asset amounts.
// CRITICAL: the root MUST be authenticated from a trusted, fully audited
// epoch sealer; a Merkle proof alone does NOT prove pro-rata correctness,
// maker signer ownership, escrow solvency, or authorize SPL transfers.
import {createHash} from "node:crypto";
import {frozenMakerClaim,auditFrozenProRataEpoch} from "./pro-rata-epoch.mjs";

const ASSETS=["baseDebit","baseCredit","quoteDebit","quoteCredit","baseFee","quoteFee"];
const HEX=/^[0-9a-f]{64}$/;
const sha=(bytes)=>createHash("sha256").update(bytes).digest("hex");
const digest=(domain,...parts)=>sha(Buffer.from(JSON.stringify([domain,...parts]),"utf8"));
const eq=(a,b)=>JSON.stringify(a)===JSON.stringify(b);
function leafHash(epochCommitment,index,makerId,weight,claim){
  return digest("HYBRID_LAZY_MAKER_LEAF_V1",epochCommitment,index,makerId,weight,
    ...ASSETS.map(a=>claim[a]));
}
function nodeHash(left,right){return digest("HYBRID_LAZY_MAKER_NODE_V1",left,right);}
function safeLeaf(leaf){
  if(!leaf||typeof leaf!=="object"||
    !Number.isSafeInteger(leaf.index)||leaf.index<0||
    typeof leaf.makerId!=="string"||!/^[A-Za-z0-9_-]{1,128}$/.test(leaf.makerId)||
    typeof leaf.weight!=="string"||!/^[1-9][0-9]*$/.test(leaf.weight)||
    !leaf.claim||typeof leaf.claim!=="object"||
    !eq(Object.keys(leaf.claim).sort(),[...ASSETS].sort())||
    ASSETS.some(a=>typeof leaf.claim[a]!=="string"||!/^(0|[1-9][0-9]*)$/.test(leaf.claim[a])))
    throw new Error("Invalid Merkle claim leaf");
  return leaf;
}
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
  const leaves=seal.leaves.map(l=>{
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
      !Array.isArray(ASSETS))
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
