import test from "node:test";
import assert from "node:assert/strict";
import {planSettlement} from "../src/settlement-plan.mjs";
import {canonicalSqrtPriceX64} from "../src/quote-orders.mjs";
const Q=1n<<64n;
const row=(i,qty,p)=>({owner:Buffer.alloc(32,i),baseQty:BigInt(qty),priceX64:p,
  sqrtPriceX64:canonicalSqrtPriceX64(p),sequence:BigInt(i)});
test("funded three-maker ASK vector matches opcode 45",()=>{
  const s={side:"ask",pages:[{},{}],orders:[row(1,100,Q),row(2,200,Q),row(3,100,4n*Q)]};
  const p=planSettlement(s,{baseFill:350n,quoteLimit:505n});
  assert.equal(p.quoteTotal,505n);
  assert.deepEqual(p.ticks.map(t=>t.totals),[[300n,0n,0n,300n,0n,3n],[50n,0n,0n,200n,0n,2n]]);
  assert.throws(()=>planSettlement(s,{baseFill:350n,quoteLimit:504n}));
  assert.throws(()=>planSettlement(s,{baseFill:150n,quoteLimit:151n}));
});
test("BID per-tick base fees and fractional collateral",()=>{
  const s={side:"bid",pages:[{},{}],orders:[row(1,100,Q),row(2,200,Q),row(3,100,Q/2n)]};
  const p=planSettlement(s,{baseFill:350n,quoteLimit:325n});
  assert.equal(p.quoteTotal,325n);
  assert.deepEqual(p.ticks.map(t=>t.totals),[[0n,297n,300n,0n,3n,0n],[0n,50n,25n,0n,0n,0n]]);
  assert.throws(()=>planSettlement(s,{baseFill:350n,quoteLimit:326n}));
});
