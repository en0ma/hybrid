import test from "node:test";
import assert from "node:assert/strict";
import {tokenBalanceDeltas,verifiesTradeMovement} from "./evidence.js";
const entry=(accountIndex,amount,mint=accountIndex===1?"BaseMint":"QuoteMint")=>({accountIndex,mint,uiTokenAmount:{amount:String(amount)}});
test("trade requires a signed movement on both sides",()=>{
  const before=[entry(1,1000),entry(2,100)];
  const after=[entry(1,800),entry(2,300)];
  assert.equal(verifiesTradeMovement(before,after),true);
  assert.deepEqual(tokenBalanceDeltas(before,after),[
    {accountIndex:1,delta:-200n},{accountIndex:2,delta:200n}
  ]);
});
test("no-op and single sided changes are not trade evidence",()=>{
  assert.equal(verifiesTradeMovement([],[]),false);
  assert.equal(verifiesTradeMovement([entry(1,100)],[entry(1,100)]),false);
  assert.equal(verifiesTradeMovement([entry(1,100)],[entry(1,99)]),false);
});
test("large atom counts are compared without floating point",()=>{
  const big=(1n<<64n)-1n;
  assert.equal(verifiesTradeMovement([entry(1,big),entry(2,0)],[
    entry(1,big-1n),entry(2,1)
  ]),true);
});

test("same-mint transfers and absent mint metadata are not trades",()=>{
  const pre=[entry(1,100,"SameMint"),entry(2,0,"SameMint")];
  const post=[entry(1,50,"SameMint"),entry(2,50,"SameMint")];
  assert.equal(verifiesTradeMovement(pre,post),false);
  assert.equal(verifiesTradeMovement(
    [{accountIndex:1,uiTokenAmount:{amount:"100"}}],
    [{accountIndex:1,uiTokenAmount:{amount:"99"}}]
  ),false);
});
