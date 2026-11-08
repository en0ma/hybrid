import test from "node:test";
import assert from "node:assert/strict";
import {decimalFromQ64, verifiedMidPrice, checkSlotFreshness} from "./live-observer.mjs";
const Q64=1n<<64n;
const book=(side, price, slot=100)=>({side,slot,orders:[{priceX64:price}]});

test("Q64 prices produce deterministic decimal atoms without floats",()=>{
  assert.equal(decimalFromQ64(Q64),"1");
  assert.equal(decimalFromQ64(Q64/2n),"0.5");
  assert.equal(decimalFromQ64(Q64*2n + Q64/4n),"2.25");
  assert.throws(()=>decimalFromQ64(0n),/Invalid/);
});
test("mid price requires same-slot, ordered, two-sided market",()=>{
  const ask=book("ask",2n*Q64);
  const bid=book("bid",Q64);
  assert.equal(verifiedMidPrice(ask,bid),"1.5");
  assert.throws(()=>verifiedMidPrice(ask,{...bid,slot:101}),/slots/);
  assert.throws(()=>verifiedMidPrice(ask,book("bid",2n*Q64)),/Crossed/);
  assert.throws(()=>verifiedMidPrice(ask,book("bid",3n*Q64)),/Crossed/);
  assert.throws(()=>verifiedMidPrice(ask,{...bid,orders:[]}),/Both/);
  assert.throws(()=>verifiedMidPrice({...ask,side:"bid"},bid),/Mixed/);
});
test("slot skew guard rejects stale, future and malformed observations",()=>{
  checkSlotFreshness(100,110,10);
  assert.throws(()=>checkSlotFreshness(100,111,10),/Stale/);
  assert.throws(()=>checkSlotFreshness(101,100,10),/Stale/);
  assert.throws(()=>checkSlotFreshness(0,100,10),/Stale/);
  assert.throws(()=>checkSlotFreshness(100,100,-1),/Stale/);
});
