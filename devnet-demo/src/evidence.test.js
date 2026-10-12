import test from "node:test";
import assert from "node:assert/strict";
import {tokenBalanceDeltas,verifiesTradeMovement} from "./evidence.js";
const entry=(accountIndex,amount,mint=accountIndex<3?"BaseMint":"QuoteMint")=>({
  accountIndex,mint,owner:"WalletOrVault",uiTokenAmount:{amount:String(amount)}
});
test("trade needs debit and credit in two separate mints",()=>{
  const pre=[entry(1,1000),entry(2,100),entry(3,1000),entry(4,100)];
  const post=[entry(1,800),entry(2,300),entry(3,800),entry(4,300)];
  assert.equal(verifiesTradeMovement(pre,post),true);
  assert.deepEqual(tokenBalanceDeltas(pre,post).map(x=>x.delta),[-200n,200n,-200n,200n]);
});
test("no-op, single-sided, or same-mint transfers are not trade evidence",()=>{
  assert.equal(verifiesTradeMovement([],[]),false);
  assert.equal(verifiesTradeMovement([entry(1,100)],[entry(1,100)]),false);
  assert.equal(verifiesTradeMovement([entry(1,100)],[entry(1,99)]),false);
  const pre=[entry(1,100),entry(2,0)];
  const post=[entry(1,50),entry(2,50)];
  assert.equal(verifiesTradeMovement(pre,post),false);
});
test("reject missing counterpart, spoofed mint, owner and invalid amount",()=>{
  const before=[entry(1,100),entry(2,0),entry(3,100),entry(4,0)];
  const after=[entry(1,50),entry(2,50),entry(3,50),entry(4,50)];
  assert.equal(verifiesTradeMovement(before,after),true);
  assert.equal(verifiesTradeMovement(before,after.slice(1)),false);
  assert.equal(verifiesTradeMovement(before,[{...after[0],mint:"Wrong"},...after.slice(1)]),false);
  assert.equal(verifiesTradeMovement(before,[{...after[0],owner:"Different"},...after.slice(1)]),false);
  assert.equal(verifiesTradeMovement(before,[{...after[0],uiTokenAmount:{amount:"NaN"}},...after.slice(1)]),false);
});
test("large atom counts are compared without floating point",()=>{
  const big=(1n<<64n)-1n;
  const before=[entry(1,big),entry(2,0),entry(3,big),entry(4,0)];
  const after=[entry(1,big-1n),entry(2,1),entry(3,big-1n),entry(4,1)];
  assert.equal(verifiesTradeMovement(before,after),true);
});
