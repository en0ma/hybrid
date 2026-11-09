import test from "node:test";
import assert from "node:assert/strict";
import {encodeFrozenEpochV2,hashFrozenEpochV2} from "../src/pro-rata-epoch-abi.mjs";
const makers=[{id:"A",weight:"1"},{id:"B",weight:"2"},{id:"C",weight:"3"}];
const totals={baseDebit:"2",baseCredit:"0",quoteDebit:"0",quoteCredit:"7",
  baseFee:"0",quoteFee:"1"};
const fixture={marketHex:"11".repeat(32),programHex:"22".repeat(32),
  genesisHex:"33".repeat(32),epochId:"7",
  tickSqrtPriceX64:(1n<<64n).toString(),side:"ask",makers,totals};
test("v2 canonical frozen epoch bytes and hash match Rust fixed vector",()=>{
  const bytes=encodeFrozenEpochV2(fixture);
  assert.equal(bytes.length,236);
  assert.equal(hashFrozenEpochV2(fixture),
    "fddeaffac0081511043c404a4fca35e3363bc97e8d235c697aa682e239cfb00b");
  assert.equal(hashFrozenEpochV2({
    ...fixture,totals:{quoteFee:"1",quoteCredit:"7",baseFee:"0",
      quoteDebit:"0",baseDebit:"2",baseCredit:"0"},
  }),hashFrozenEpochV2(fixture));
});
test("market, price, epoch, roster order, fees and direction cannot be substituted",()=>{
  const expected=hashFrozenEpochV2(fixture);
  const tampered=[
    {...fixture,marketHex:"44".repeat(32)},
    {...fixture,programHex:"44".repeat(32)},
    {...fixture,genesisHex:"44".repeat(32)},
    {...fixture,epochId:"8"},
    {...fixture,tickSqrtPriceX64:(2n<<64n).toString()},
    {...fixture,makers:[makers[1],makers[0],makers[2]]},
    {...fixture,totals:{...totals,quoteFee:"2"}},
  ];
  for(const value of tampered)assert.notEqual(hashFrozenEpochV2(value),expected);
  assert.throws(()=>hashFrozenEpochV2({...fixture,side:"bid"}),/Invalid epoch filled base|Cross-direction/);
});
test("reject invalid or ambiguous roster, amounts and market",()=>{
  assert.throws(()=>hashFrozenEpochV2({...fixture,makers:[]}),/roster/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,makers:[makers[0],makers[0]]}),/duplicate/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,makers:[{id:"A",weight:"0"}]}),/Out-of-range/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,marketHex:"aa"}),/market/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,programHex:"aa"}),/program or genesis/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,genesisHex:"aa"}),/program or genesis/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,tickSqrtPriceX64:"0"}),/Out-of-range/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,epochId:"07"}),/Noncanonical/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,totals:{...totals,quoteFee:"-1"}}),/Noncanonical/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,totals:{...totals,quoteFee:(1n<<64n).toString()}}),/Out-of-range/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,totals:{...totals,baseDebit:"7"}}),/filled base/);
  assert.throws(()=>hashFrozenEpochV2({...fixture,totals:{...totals,extra:"5"}}),/Missing asset totals/);
});
