import test from "node:test";
import assert from "node:assert/strict";
import {PublicKey} from "@solana/web3.js";
import {decodeCustody,verifyCustodyLinks} from "./custody.js";
const key=(value)=>new PublicKey(new Uint8Array(32).fill(value));
test("decode canonical custody layout and assert vault binding",()=>{
  const b=new Uint8Array(192);b.set(new TextEncoder().encode("HYBCUST1"));b[8]=1;b[10]=6;b[11]=9;
  for(const [offset,value] of [[16,1],[48,2],[80,3],[112,4],[144,5]])b.set(key(value).toBytes(),offset);
  new DataView(b.buffer).setBigUint64(176,(1n<<64n)-1n,true);
  const c=decodeCustody(b);
  assert.equal(c.totalBase,((1n<<64n)-1n).toString());
  assert.equal(verifyCustodyLinks(c,key(1).toBase58(),key(4).toBase58(),key(5).toBase58()),true);
  assert.throws(()=>verifyCustodyLinks(c,key(1).toBase58(),key(5).toBase58(),key(4).toBase58()));
  const bad=b.slice();bad[0]=0;assert.throws(()=>decodeCustody(bad));
  assert.throws(()=>decodeCustody(b.slice(0,190)));
});
