import test from "node:test";
import assert from "node:assert/strict";
import {PublicKey} from "@solana/web3.js";
import {decodeOwnedBookPage,combineOwnedBookPages,verifyOwnedOrderTransition} from "./book.js";
const maker=new PublicKey(new Uint8Array(32).fill(7)).toBase58();
function raw(index,entries=[]){
 const p=Buffer.alloc(1552),o=Buffer.alloc(1040);
 for(const b of [p,o]){b.writeUInt16LE(entries.length,0);b.writeUInt32LE(index,2);
  b.writeUInt32LE(index===0?0xffffffff:0,6);b.writeUInt32LE(index===0?1:0xffffffff,10);}
 entries.forEach((e,i)=>{const x=16+i*48,y=16+i*32;
  p.writeBigUInt64LE(BigInt(e.price),x);p.writeBigUInt64LE(1n,x+16);
  p.writeBigUInt64LE(BigInt(e.qty),x+32);p.writeBigUInt64LE(BigInt(e.seq),x+40);
  o.fill(7,y,y+32);});
 return [p,o];
}
test("decodes 48-byte orders and 32-byte owner sidecars",()=>{
 const a=decodeOwnedBookPage(...raw(0,[{price:2,qty:90,seq:7}]),"ask",0);
 const b=decodeOwnedBookPage(...raw(1),"ask",1);
 const orders=combineOwnedBookPages([a,b],"ask");
 assert.deepEqual(orders.map(x=>x.sequence),["7"]);
 assert.equal(orders[0].owner,maker);
 assert.equal(verifyOwnedOrderTransition([],orders,"place","7",maker,0).bookIdentityChecked,true);
 assert.equal(verifyOwnedOrderTransition(orders,[],"cancel","7",maker,0).ownerVerified,true);
 assert.throws(()=>verifyOwnedOrderTransition([],orders,"place","7",PublicKey.default.toBase58(),0),/owner/);
});
test("rejects owner mismatch, corrupt links, duplicate sequence, and unrelated changes",()=>{
 const [p,o]=raw(0,[{price:1,qty:3,seq:4}]);
 o.writeUInt16LE(0,0);assert.throws(()=>decodeOwnedBookPage(p,o,"ask",0),/count/);
 const x=decodeOwnedBookPage(...raw(0,[{price:2,qty:3,seq:4}]),"ask",0);
 const y=decodeOwnedBookPage(...raw(1,[{price:1,qty:3,seq:4}]),"ask",1);
 assert.throws(()=>combineOwnedBookPages([x,y],"ask"),/Duplicate/);
 const z=decodeOwnedBookPage(...raw(1,[{price:3,qty:3,seq:5}]),"ask",1);
 assert.equal(combineOwnedBookPages([x,z],"ask").length,2);
 assert.throws(()=>verifyOwnedOrderTransition([x.orders[0]],[z.orders[0]],"cancel","4",maker,0),/Unrelated/);
});
