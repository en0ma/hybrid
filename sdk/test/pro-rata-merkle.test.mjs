import test from "node:test";
import assert from "node:assert/strict";
import {freezeProRataEpoch,frozenMakerClaim} from "../src/pro-rata-epoch.mjs";
import {
  sealFrozenMakerClaims,proveFrozenMakerClaim,verifyFrozenMakerProof,
} from "../src/pro-rata-merkle.mjs";
const totals={baseDebit:"7",baseCredit:"0",quoteDebit:"0",
  quoteCredit:"29",baseFee:"0",quoteFee:"3"};
function epoch(n=13,id=17){
  return freezeProRataEpoch({
    epochId:id,tickId:"tick_301",
    makers:Array.from({length:n},(_,i)=>({
      id:"maker_"+i,weight:String((i*19)%23+1),
    })),
    totals,
  });
}
test("all maker proofs match frozen allocation across odd Merkle levels",()=>{
  for(const n of [1,2,3,5,13,137]){
    const e=epoch(n);
    const s=sealFrozenMakerClaims(e);
    for(let index=0;index<n;index++){
      const proof=proveFrozenMakerClaim(s,index);
      const id=e.makers[index].id;
      const verified=verifyFrozenMakerProof(proof,{
        expectedRoot:s.root,expectedEpochCommitment:e.commitment,expectedMakerId:id,
      });
      assert.deepEqual(verified.claim,frozenMakerClaim(e,id));
      assert.equal(verified.canWithdraw,false);
    }
  }
});
test("frozen claim proof remains valid after the tick changes",()=>{
  const old=epoch(13,17);
  const first=sealFrozenMakerClaims(old);
  const proof=proveFrozenMakerClaim(first,7);
  const moved=epoch(13,18);
  const second=sealFrozenMakerClaims(moved);
  assert.equal(verifyFrozenMakerProof(proof,{
    expectedRoot:first.root,expectedEpochCommitment:old.commitment,
    expectedMakerId:proof.leaf.makerId,
  }).mode,"membership-only");
  assert.throws(()=>verifyFrozenMakerProof(proof,{
    expectedRoot:second.root,expectedEpochCommitment:moved.commitment,
    expectedMakerId:proof.leaf.makerId,
  }),/Untrusted/);
});
test("wrong maker, changed amount, root, position and sibling fail closed",()=>{
  const e=epoch();
  const seal=sealFrozenMakerClaims(e);
  const p=proveFrozenMakerClaim(seal,5);
  const expected={expectedRoot:seal.root,
    expectedEpochCommitment:e.commitment,expectedMakerId:p.leaf.makerId};
  assert.throws(()=>verifyFrozenMakerProof(p,{...expected,expectedMakerId:"attacker"}),/Wrong maker/);
  assert.throws(()=>verifyFrozenMakerProof(p,{
    ...expected,expectedRoot:"f".repeat(64),
  }),/Untrusted/);
  assert.throws(()=>verifyFrozenMakerProof({
    ...p,leaf:{...p.leaf,claim:{...p.leaf.claim,quoteCredit:"777"}},
  },expected),/Invalid maker Merkle/);
  assert.throws(()=>verifyFrozenMakerProof({
    ...p,leaf:{...p.leaf,index:6},
  },expected),/Invalid maker Merkle/);
  assert.throws(()=>verifyFrozenMakerProof({
    ...p,siblings:[...p.siblings.slice(0,-1)],
  },expected),/Invalid maker Merkle/);
  assert.throws(()=>verifyFrozenMakerProof({
    ...p,siblings:[...p.siblings,"f".repeat(64)],
  },expected),/Invalid maker Merkle/);
  assert.throws(()=>verifyFrozenMakerProof({
    ...p,siblings:["g".repeat(64),...p.siblings.slice(1)],
  },expected),/Invalid maker Merkle/);
});
test("odd terminal leaf cannot forge a different duplicated sibling",()=>{
  const e=epoch(5);
  const seal=sealFrozenMakerClaims(e);
  const proof=proveFrozenMakerClaim(seal,4);
  assert.throws(()=>verifyFrozenMakerProof({
    ...proof,siblings:["a".repeat(64),...proof.siblings.slice(1)],
  },{
    expectedRoot:seal.root,expectedEpochCommitment:e.commitment,
    expectedMakerId:proof.leaf.makerId,
  }),/Noncanonical/);
});
test("the proof is only membership; the root still needs external trust",()=>{
  const e=epoch();
  const seal=sealFrozenMakerClaims(e);
  const proof=proveFrozenMakerClaim(seal,0);
  const result=verifyFrozenMakerProof(proof,{
    expectedRoot:seal.root,expectedEpochCommitment:e.commitment,
    expectedMakerId:proof.leaf.makerId,
  });
  assert.equal(result.canWithdraw,false);
  assert.equal(result.mode,"membership-only");
});
