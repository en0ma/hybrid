import test from "node:test";
import assert from "node:assert/strict";
import {
  freezeProRataEpoch,frozenMakerClaim,newFrozenSettlementLedger,
  claimFrozenMaker,auditFrozenProRataEpoch,
} from "../src/pro-rata-epoch.mjs";
const totals=(overrides={})=>({
  baseDebit:"2",baseCredit:"0",quoteDebit:"0",
  quoteCredit:"7",baseFee:"0",quoteFee:"2",...overrides,
});
const three=()=>freezeProRataEpoch({
  epochId:0,tickId:"tick_42",
  makers:[{id:"A",weight:"1"},{id:"B",weight:"1"},{id:"C",weight:"1"}],
  totals:totals(),
});
test("frozen epoch assigns exact base, proceeds and fee dust once",()=>{
  const e=three();
  assert.equal(auditFrozenProRataEpoch(e).exactlyAllocated,true);
  const claims=e.makers.map(m=>frozenMakerClaim(e,m.id));
  assert.deepEqual(claims.map(x=>x.baseDebit),["0","1","1"]);
  assert.deepEqual(claims.map(x=>x.quoteCredit),["2","2","3"]);
  assert.deepEqual(claims.map(x=>x.quoteFee),["0","1","1"]);
  for(const asset of Object.keys(totals())){
    assert.equal(claims.reduce((n,x)=>n+BigInt(x[asset]),0n).toString(),e.totals[asset]);
  }
});
test("late settlement after new tick is independent of settlement order",()=>{
  const old=three();
  const moved=freezeProRataEpoch({
    epochId:1,tickId:"tick_43",makers:[{id:"A",weight:"2"},{id:"B",weight:"3"}],
    totals:totals({baseDebit:"0",baseCredit:"3",quoteDebit:"11",quoteCredit:"0",quoteFee:"0",baseFee:"1"}),
  });
  let oldLedger=newFrozenSettlementLedger(old);
  for(const id of ["C","A","B"]){
    const result=claimFrozenMaker(old,oldLedger,id);
    assert.deepEqual(result.claim,frozenMakerClaim(old,id));
    oldLedger=result.ledger;
  }
  assert.equal(oldLedger.claimed.length,3);
  assert.throws(()=>claimFrozenMaker(old,oldLedger,"B"),/already claimed/);
  assert.throws(()=>claimFrozenMaker(moved,oldLedger,"A"),/mismatched/);
  const newLedger=newFrozenSettlementLedger(moved);
  assert.equal(claimFrozenMaker(moved,newLedger,"A").claim.baseCredit,"1");
  assert.equal(auditFrozenProRataEpoch(moved).exactlyAllocated,true);
  assert.notEqual(old.commitment,moved.commitment);
});
test("dust priority rotates between immutable epochs",()=>{
  const maker=[{id:"A",weight:"1"},{id:"B",weight:"1"},{id:"C",weight:"1"}];
  const a=freezeProRataEpoch({epochId:0,tickId:"tick_1",makers:maker,totals:totals()});
  const b=freezeProRataEpoch({epochId:1,tickId:"tick_2",makers:maker,totals:totals()});
  assert.equal(frozenMakerClaim(a,"A").baseDebit,"0");
  assert.equal(frozenMakerClaim(b,"B").baseDebit,"0");
  assert.equal(auditFrozenProRataEpoch(a).exactlyAllocated,true);
  assert.equal(auditFrozenProRataEpoch(b).exactlyAllocated,true);
});
test("100+ uneven makers, shuffled settlement, large atom values conserve exactly",()=>{
  const maker=Array.from({length:137},(_,i)=>({
    id:"maker_"+String(i).padStart(3,"0"),weight:String((i*i*17)%101+1),
  }));
  const W=maker.reduce((n,m)=>n+BigInt(m.weight),0n);
  const e=freezeProRataEpoch({
    epochId:6789,tickId:"tick_999",makers:maker,
    totals:totals({baseDebit:String(W-1n),quoteCredit:String((1n<<64n)-1n),
      quoteFee:"17",baseFee:"0"}),
  });
  const totalsSeen=Object.fromEntries(Object.keys(totals()).map(k=>[k,0n]));
  let ledger=newFrozenSettlementLedger(e);
  for(const maker of [...e.makers].reverse()){
    const {claim,ledger:next}=claimFrozenMaker(e,ledger,maker.id);
    for(const [k,v] of Object.entries(claim))totalsSeen[k]+=BigInt(v);
    ledger=next;
  }
  for(const [k,v] of Object.entries(e.totals))assert.equal(totalsSeen[k],BigInt(v));
  assert.equal(auditFrozenProRataEpoch(e).exactlyAllocated,true);
});
test("reject unfinished, malformed, mutated, overfilled and duplicate epochs",()=>{
  const e=three();
  assert.throws(()=>frozenMakerClaim({...e,status:"live"},"A"),/Invalid frozen/);
  assert.throws(()=>frozenMakerClaim({...e,totals:{...e.totals,baseDebit:"1"}},"A"),/commitment mismatch/);
  assert.throws(()=>frozenMakerClaim(e,"X"),/not active/);
  assert.throws(()=>freezeProRataEpoch({
    epochId:0,tickId:"tick_42",makers:[{id:"A",weight:"1"},{id:"A",weight:"2"}],totals:totals(),
  }),/Duplicate/);
  assert.throws(()=>freezeProRataEpoch({
    epochId:0,tickId:"tick_42",makers:[{id:"A",weight:"0"}],totals:totals(),
  }),/Zero/);
  assert.throws(()=>freezeProRataEpoch({
    epochId:0,tickId:"tick_42",makers:[{id:"A",weight:"1"}],totals:totals(),
  }),/exceeds active epoch weight/);
  assert.throws(()=>freezeProRataEpoch({
    epochId:0,tickId:"tick_42",makers:[{id:"A",weight:"3"}],
    totals:totals({baseDebit:"2",quoteDebit:"1"}),
  }),/cannot mix/);
});
test("synthetic claim ledger rejects replay and foreign claims",()=>{
  const e=three();
  const ledger=newFrozenSettlementLedger(e);
  assert.throws(()=>claimFrozenMaker(e,{...ledger,claimed:["A","A"]},"B"),/Invalid/);
  assert.throws(()=>claimFrozenMaker(e,ledger,"not_a_maker"),/not active/);
});
