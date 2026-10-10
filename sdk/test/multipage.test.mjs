import test from "node:test";
import assert from "node:assert/strict";
import {loadLinkedBookSnapshot} from "../src/multipage.mjs";
const Q = 1n<<64n;
const key = n => Buffer.alloc(32,n);
function u128(b,i,v){b.writeBigUInt64LE(v&((1n<<64n)-1n),i);b.writeBigUInt64LE(v>>64n,i+8);}
function fixtures(side="ask", badLink=false) {
  const program=key(1),market=key(2),owners=[key(9),key(10)];
  const pairs=[{index:0,page:key(3),ownerPage:owners[0]},{index:1,page:key(4),ownerPage:owners[1]}];
  const m=Buffer.alloc(128);m.write("HYBRID01",0,"ascii");m[8]=1;m.writeUInt16LE(1,10);
  m.writeUInt32LE(side==="ask"?2:0,12);m.writeUInt32LE(side==="bid"?2:0,20);
  m.writeBigUInt64LE(3n,32);u128(m,48,Q);u128(m,64,1n);
  m.writeBigUInt64LE(1n,80);m.writeBigUInt64LE(1n,88);
  if(side==="ask")m.fill(9,96,128);else{m.fill(9,24,32);m.fill(9,40,48);}
  const pages=[0,1].map(i=>{
    const p=Buffer.alloc(1552),o=Buffer.alloc(1040);
    for(const b of [p,o]) {
      b.writeUInt16LE(1,0);
      b.writeUInt32LE(i,2);b.writeUInt32LE(i?0:0xffffffff,6);
      b.writeUInt32LE(i===0?badLink?0xffffffff:1:0xffffffff,10);
    }
    const price=side==="ask"?Q+BigInt(i):Q+BigInt(1-i);
    u128(p,16,price);u128(p,32,Q);p.writeBigUInt64LE(10n,48);
    p.writeBigUInt64LE(BigInt(i+1),56);o.fill(30+i,16,48);
    return [p,o];
  });
  const value=[m,...pages.flat()].map(data=>({owner:program,data}));
  const connection={async getMultipleAccountsInfoAndContext(keys,opts){
    assert.equal(keys.length,5);assert.equal(opts.minContextSlot,100);
    return {context:{slot:101},value};
  }};
  const args={programId:program,market,side,pages:pairs,minContextSlot:100,
    verifyAddresses:async()=>true};
  return {connection,args,value};
}
test("single context loads authenticated two-page ask and bid order chain",async()=>{
  for(const side of ["ask","bid"]) {
    const f=fixtures(side);
    const result=await loadLinkedBookSnapshot(f.connection,f.args);
    assert.equal(result.slot,101);assert.equal(result.orders.length,2);
    assert.deepEqual(result.orders.map(x=>x.pageIndex),[0,1]);
  }
});
test("rejects incomplete links, compromised PDA and cross-page order priority",async()=>{
  const f=fixtures("ask",true);
  await assert.rejects(loadLinkedBookSnapshot(f.connection,f.args),/Broken page/);
  const a=fixtures();a.args.verifyAddresses=async()=>false;
  await assert.rejects(loadLinkedBookSnapshot(a.connection,a.args),/Unverified/);
  const b=fixtures();
  u128(b.value[3].data,16,Q);
  b.value[3].data.writeBigUInt64LE(1n,56);
  await assert.rejects(loadLinkedBookSnapshot(b.connection,b.args),/Cross-page price\/time/);
});
test("rejects unowned sidecars, wrong counts and duplicate indices",async()=>{
  const a=fixtures();a.value[4].owner=key(22);
  await assert.rejects(loadLinkedBookSnapshot(a.connection,a.args),/wrongly owned/);
  const b=fixtures();b.value[0].data.writeUInt32LE(3,12);
  await assert.rejects(loadLinkedBookSnapshot(b.connection,b.args),/total depth mismatch/);
  const c=fixtures();c.args.pages[1].index=0;
  await assert.rejects(loadLinkedBookSnapshot(c.connection,c.args),/Duplicate index/);
});
