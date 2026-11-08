import test from "node:test";
import assert from "node:assert/strict";
import {
  decimalToPriceX64,canonicalSqrtPriceX64,convertQuoteTargets,
} from "../src/quote-orders.mjs";
const Q64=1n<<64n,MAX128=(1n<<128n)-1n;
const spot=s=>(s*s)/Q64;

test("exact quote prices are rounded in the conservative direction",()=>{
  assert.equal(decimalToPriceX64("1","bid"),Q64);
  assert.equal(decimalToPriceX64("1","ask"),Q64);
  assert.equal(decimalToPriceX64("0.5","bid"),Q64/2n);
  const bid=decimalToPriceX64("0.1","bid");
  const ask=decimalToPriceX64("0.1","ask");
  assert.equal(ask,bid+1n);
  assert.ok(bid*10n<=Q64);
  assert.ok(ask*10n>=Q64);
  assert.throws(()=>decimalToPriceX64("1","sell"),/side/);
  for(const bad of ["0","-1","1e5","1.0000000000000000001"," 1","1."]){
    assert.throws(()=>decimalToPriceX64(bad,"bid"));
  }
});
test("cached sqrt exactly meets program's canonical floor inequalities",()=>{
  for(const price of [1n,Q64/10n,Q64,Q64+1n,Q64*2n,MAX128]){
    const sqrt=canonicalSqrtPriceX64(price);
    assert.ok(spot(sqrt)<=price);
    assert.ok(spot(sqrt+1n)>price);
  }
  assert.throws(()=>canonicalSqrtPriceX64(0n),/Invalid/);
  assert.throws(()=>canonicalSqrtPriceX64(MAX128+1n),/Invalid/);
});
test("bid collateral is rounded up and cannot reuse locked maker inventory",()=>{
  const result=convertQuoteTargets({
    orders:[
      {side:"bid",priceAtoms:"0.25",sizeAtoms:"5"},
      {side:"ask",priceAtoms:"1.5",sizeAtoms:"5"},
    ],
    baseLotSizeAtoms:"5",freeBaseAtoms:"5",freeQuoteAtoms:"2",
  });
  assert.equal(result.mode,"preflight-only");
  assert.equal(result.reservedBaseAtoms,5n);
  assert.equal(result.reservedQuoteAtoms,2n);
  assert.equal(result.desired[0].baseQty,5n);
  assert.deepEqual(result.desired.map(x=>x.side),["bid","ask"]);
  assert.throws(()=>convertQuoteTargets({
    orders:[{side:"bid",priceAtoms:"0.25",sizeAtoms:"5"}],
    baseLotSizeAtoms:"5",freeBaseAtoms:"999",freeQuoteAtoms:"1",
  }),/Insufficient free quote/);
});
test("reject partial lots, oversized targets, insufficient base and bad account limits",()=>{
  const base={
    orders:[{side:"ask",priceAtoms:"1",sizeAtoms:"5"}],
    baseLotSizeAtoms:"5",freeBaseAtoms:"5",freeQuoteAtoms:"0",
  };
  assert.throws(()=>convertQuoteTargets({...base,freeBaseAtoms:"4"}),/Insufficient free base/);
  assert.throws(()=>convertQuoteTargets({...base,baseLotSizeAtoms:"2"}),/whole base lot/);
  assert.throws(()=>convertQuoteTargets({...base,maxOrders:0}),/target count/);
  assert.throws(()=>convertQuoteTargets({...base,freeQuoteAtoms:"-1"}),/Invalid/);
  assert.throws(()=>convertQuoteTargets({...base,orders:[
    {side:"ask",priceAtoms:"1",sizeAtoms:"5"},
    {side:"ask",priceAtoms:"1",sizeAtoms:"5"},
  ]}),/Insufficient free base/);
});
