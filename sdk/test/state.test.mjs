import test from "node:test";
import assert from "node:assert/strict";
import { decodeMarketHeader,decodeOrderPage,decodeOwnerPage } from "../src/state.mjs";
const q=1n<<64n;
function put128(b,i,v){b.writeBigUInt64LE(v&((1n<<64n)-1n),i);b.writeBigUInt64LE(v>>64n,i+8)}
function market(){
 const b=Buffer.alloc(128);
 b.write("HYBRID01",0,"ascii");b[8]=1;b.writeUInt16LE(1,10);
 b.writeUInt32LE(1,12);b.writeUInt32LE(1,20);
 b.writeBigUInt64LE(4n,32);put128(b,48,q);put128(b,64,100n);
 b.writeBigUInt64LE(1n,80);b.writeBigUInt64LE(1n,88);return b;
}
function page(side,amount=10n){
 const b=Buffer.alloc(1552);b.writeUInt16LE(1,0);
 put128(b,16,q);put128(b,32,q);
 b.writeBigUInt64LE(amount,48);b.writeBigUInt64LE(2n,56);
 return b;
}
test("market header layout matches 128-byte v1 state",()=>{
 const m=decodeMarketHeader(market());
 assert.equal(m.collateralizedActive,true);
 assert.equal(m.bidCount,1);
 assert.equal(m.askCount,1);
 assert.equal(m.sqrtPriceX64,q);
 assert.equal(m.liquidity,100n);
 assert.throws(()=>decodeMarketHeader(Buffer.alloc(128)),/magic/);
 assert.throws(()=>decodeMarketHeader(Buffer.alloc(120)),/128/);
});
test("decode ordered ask and bid page with owner sidecar",()=>{
 for(const side of ["ask","bid"]){
  const p=decodeOrderPage(page(side),side);
  assert.equal(p.entries[0].baseQty,10n);
  assert.deepEqual(p.links,{pageIndex:0,prevPage:null,nextPage:null});
  const owners=Buffer.alloc(1040);owners.writeUInt16LE(1,0);owners.fill(7,16,48);
  const combined=decodeOwnerPage(owners,p);
  assert.equal(combined[0].owner[0],7);
  assert.equal(combined[0].sequence,2n);
 }
});
test("reject malformed orders, owner misalignment and trailing data",()=>{
 const invalid=page("ask");invalid.writeUInt16LE(33,0);
 assert.throws(()=>decodeOrderPage(invalid,"ask"),/length/);
 const trailing=page("ask");trailing[16+48]=1;
 assert.throws(()=>decodeOrderPage(trailing,"ask"),/trailing/);
 const p=decodeOrderPage(page("ask"),"ask");
 assert.throws(()=>decodeOwnerPage(Buffer.alloc(1040),p),/mismatch/);
 const emptyOwner=Buffer.alloc(1040);emptyOwner.writeUInt16LE(1,0);
 assert.throws(()=>decodeOwnerPage(emptyOwner,p),/Empty owner/);
 const badOrder=page("ask");badOrder.writeBigUInt64LE(0n,56);
 assert.throws(()=>decodeOrderPage(badOrder,"ask"),/Invalid order/);
});
test("enforce canonical ask/bid price-time ordering",()=>{
 for(const side of ["ask","bid"]){
  const p=page(side);p.writeUInt16LE(2,0);
  put128(p,16+48,q);put128(p,32+48,q);
  p.writeBigUInt64LE(11n,48+48);
  p.writeBigUInt64LE(1n,56+48);
  assert.throws(()=>decodeOrderPage(p,side),/Out-of-order/);
 }
});

test("reject inconsistent cached sqrt-price even when order quantities are valid",()=>{
 const b=page("ask");
 put128(b,32,q+1n);
 assert.throws(()=>decodeOrderPage(b,"ask"),/Noncanonical/);
});
test("reject duplicate order IDs even when prices are correctly sorted",()=>{
 const b=page("ask");b.writeUInt16LE(2,0);
 put128(b,16+48,q+2n);put128(b,32+48,q+1n);
 b.writeBigUInt64LE(20n,48+48);
 b.writeBigUInt64LE(2n,56+48);
 assert.throws(()=>decodeOrderPage(b,"ask"),/Duplicate order sequence/);
});
