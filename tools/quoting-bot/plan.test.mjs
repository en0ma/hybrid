import test from "node:test";
import assert from "node:assert/strict";
import {planQuotes} from "./plan.mjs";

const c=()=>({midPriceAtoms:"1000",spreadBps:50,levels:2,stepBps:50,sizeAtoms:"3",maxBaseAtoms:"6",maxQuoteAtoms:"10000"});
test("quoting is deterministic with per-side inventory bounds",()=>{
  const x=planQuotes(c());
  assert.equal(x.orders.length,4);
  assert.equal(x.reservedBaseAtoms,"6");
  assert.ok(BigInt(x.reservedQuoteAtoms)<=10000n);
  assert.deepEqual(x,planQuotes(c()));
});
test("stale market data blocks planning",()=>assert.throws(()=>planQuotes({...c(),stale:true}),/stale/));
test("insufficient quote collateral omits bid side",()=>assert.ok(planQuotes({...c(),maxQuoteAtoms:"1"}).orders.every(o=>o.side==="ask")));
test("reject invalid sizes",()=>assert.throws(()=>planQuotes({...c(),sizeAtoms:"0"}),/inventory/));
test("fractional quote atoms per base atom remain exact",()=>{
  const result=planQuotes({...c(),midPriceAtoms:"0.15",spreadBps:50,levels:1,sizeAtoms:"1000",maxBaseAtoms:"1000",maxQuoteAtoms:"150"});
  assert.deepEqual(result.orders.map(o=>({side:o.side,priceAtoms:o.priceAtoms})),[
    {side:"bid",priceAtoms:"0.14925"},{side:"ask",priceAtoms:"0.15075"}
  ]);
  assert.equal(result.reservedQuoteAtoms,"150"); // ceil(149.25 atoms), never underreserve
});
test("fractional collateral cost rounds up, without rounding price",()=>{
  const r=planQuotes({...c(),midPriceAtoms:"0.15",levels:1,sizeAtoms:"3",maxBaseAtoms:"3",maxQuoteAtoms:"1"});
  assert.equal(r.reservedQuoteAtoms,"1");
  assert.equal(r.orders[0].priceAtoms,"0.14925");
  assert.ok(r.orders.some(o=>o.side==="ask"));
  assert.ok(planQuotes({...c(),midPriceAtoms:"0.15",levels:1,sizeAtoms:"3",maxBaseAtoms:"3",maxQuoteAtoms:"0"}).orders.every(o=>o.side==="ask"));
});
test("integer prices with fractional bps result remain exact",()=>{
  const r=planQuotes({...c(),midPriceAtoms:"1001",levels:1});
  assert.deepEqual(r.orders.map(o=>o.priceAtoms),["995.995","1006.005"]);
  assert.equal(r.reservedQuoteAtoms,"2988"); // ceil(995.995 * 3)
});
test("invalid decimal inputs fail without float conversion",()=>{
  for(const v of ["0","0.0","-1","0.1e3","1.1234567890123456789","abc","1."]){
    assert.throws(()=>planQuotes({...c(),midPriceAtoms:v}),/invalid/);
  }
  assert.throws(()=>planQuotes({...c(),maxQuoteAtoms:"1.5"}),/invalid/);
});
