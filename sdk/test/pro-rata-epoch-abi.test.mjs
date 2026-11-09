import test from "node:test";
import assert from "node:assert/strict";
import {encodeFrozenEpochV2,hashFrozenEpochV2} from "../src/pro-rata-epoch-abi.mjs";
const makers=[{id:"A",weight:"1"},{id:"B",weight:"2"},{id:"C",weight:"3"}];
const totals={baseDebit:"2",baseCredit:"0",quoteDebit:"0",quoteCredit:"7",
  baseFee:"0",quoteFee:"1"};
const fixture={marketHex:"11".repeat(32),epochId:"7",
  tickSqrtPriceX64:(1n<<64n).toString(),side:"ask",makers,totals};
test("v2 canonical frozen epoch bytes and hash match Rust fixed vector",()=>{
  const bytes=encodeFrozenEpochV2(fixture);
  assert.equal(bytes.length,172);
  assert.equal(hashFrozenEpochV2(fixture),
    "dd4d8af7657c633c808fb709c8d30971099c72a5d4828228784637a6eb011ac1");
  assert.equal(hashFrozenEpochV2({
    ...fixture,totals:{quoteFee:"1",quoteCredit:"7",baseFee:"0",
      quoteDebit:"0",baseDebit:"2",baseCredit:"0"},
  }),hashFrozenEpochV2(fixture));
});
test("market, price, epoch, roster order, fees and direction cannot be substituted",()=>{
  const expected=hashFrozenEpochV2(fixture);
  const tampered=[
    {...fixture,marketHex:"22".repeat(32)},
    {...fixture,epochId:"8"},
    {...fixture,tickSqrtPriceX64:(2n<<64n).toString()},
    {...fixture,makers:[makers[1],makers[0],makers[2]]},
    {...fixture,totals:{...totals,quoteFee:"2"}},
  ];
  for(const value of tampered)assert.notEqual(hashFrozenEpochV2(value),expected);
  assert.throws(()=>hashFrozenEpochV2({...fixture,side:"bid"}),/Cross-direction/);
});
test("reject invalid or ambiguous roster, amounts and market",()=>{
  assert.throws(()=>hashFrozenEpochV2({...fixture,makers:[]}),/roster/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,makers:[makers[0],makers[0]]}),/duplicate/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,makers:[{id:"A",weight:"0"}]}),/Out-of-range/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,marketHex:"aa"}),/market/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,tickSqrtPriceX64:"0"}),/Out-of-range/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,epochId:"07"}),/Noncanonical/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,totals:{...totals,quoteFee:"-1"}}),/Noncanonical/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,totals:{...totals,quoteFee:(1n<<64n).toString()}}),/Out-of-range/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,totals:{...totals,baseDebit:"7"}}),/filled base/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,totals:{...totals,extra:"5"}}),/Missing asset totals/);
});
