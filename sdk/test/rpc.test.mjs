import test from "node:test";
import assert from "node:assert/strict";
import { loadActivePageSnapshot } from "../src/rpc.mjs";
const Q=1n<<64n;
const key = (v) => ({toBytes:()=>Buffer.alloc(32,v),toString:()=>String(v),equals(o){return o?.toString()===String(v);}});
function put128(b,i,v){b.writeBigUInt64LE(v&((1n<<64n)-1n),i);b.writeBigUInt64LE(v>>64n,i+8);}
function fixtures(side="ask"){
  const program=key(1),owner=key(9),market=key(3),page=key(4);
  const m=Buffer.alloc(128);
  m.write("HYBRID01",0,"ascii");m[8]=1;m.writeUInt16LE(1,10);
  m.writeUInt32LE(side==="ask"?1:0,12);
  m.writeUInt32LE(side==="bid"?1:0,20);
  m.writeBigUInt64LE(2n,32);put128(m,48,Q);put128(m,64,1n);
  m.writeBigUInt64LE(1n,80);m.writeBigUInt64LE(1n,88);
  if(side==="ask")m.fill(9,96,128);
  else {m.fill(9,24,32);m.fill(9,40,48);}
  const p=Buffer.alloc(1552);p.writeUInt16LE(1,0);
  put128(p,16,Q);put128(p,32,Q);
  p.writeBigUInt64LE(10n,48);p.writeBigUInt64LE(1n,56);
  const owners=Buffer.alloc(1040);owners.writeUInt16LE(1,0);owners.fill(7,16,48);
  const values=[m,p,owners].map(data=>({owner:program,data}));
  let response={context:{slot:100},value:values};
  const connection={async getMultipleAccountsInfoAndContext(_keys,opts){
    assert.equal(opts.minContextSlot,90);
    return response;
  }};
  return {program,owner,market,page,connection,values,setResponse(x){response=x;}};
}
test("returns one-slot authenticated ask and bid snapshots",async()=>{
 for(const side of ["ask","bid"]){
  const f=fixtures(side);
  const snapshot=await loadActivePageSnapshot(f.connection,{
    programId:f.program,market:f.market,page:f.page,ownerPage:f.owner,
    side,minContextSlot:90,
  });
  assert.equal(snapshot.slot,100);
  assert.equal(snapshot.orders[0].baseQty,10n);
  assert.equal(snapshot.orders[0].owner[0],7);
 }
});
test("reject stale RPC results, wrong owner and wrong sidecar binding",async()=>{
 const f=fixtures();
 f.setResponse({context:{slot:89},value:f.values});
 await assert.rejects(loadActivePageSnapshot(f.connection,{
  programId:f.program,market:f.market,page:f.page,ownerPage:f.owner,
  side:"ask",minContextSlot:90,
 }),/stale/);
 const values=f.values.map(x=>({...x}));
 values[1].owner=key(12);
 f.setResponse({context:{slot:100},value:values});
 await assert.rejects(loadActivePageSnapshot(f.connection,{
  programId:f.program,market:f.market,page:f.page,ownerPage:f.owner,
  side:"ask",minContextSlot:90,
 }),/Incorrect program owner/);
 const bound=f.values.map(x=>({...x}));
 bound[0].data=Buffer.from(bound[0].data);
 bound[0].data.fill(8,96,128);
 f.setResponse({context:{slot:100},value:bound});
 await assert.rejects(loadActivePageSnapshot(f.connection,{
  programId:f.program,market:f.market,page:f.page,ownerPage:f.owner,
  side:"ask",minContextSlot:90,
 }),/sidecar binding mismatch/);
});
