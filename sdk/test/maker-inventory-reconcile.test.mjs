import test from "node:test";
import assert from "node:assert/strict";
import {reconcileMakerInventory} from "../src/maker-inventory-reconcile.mjs";
const first={side:"ask",sequence:"10",priceX64:"18446744073709551616",baseQty:"5"};
const next={side:"bid",sequence:"11",priceX64:"18446744073709551616",baseQty:"4"};
const before={mode:"post-finality-audit-only",canSubmit:false,slot:110,
  finalizedTransactionSlot:100,freeBase:20n,lockedBase:5n,
  freeQuote:30n,lockedQuote:4n,ownedOrders:[first]};
const after={...before,slot:120,finalizedTransactionSlot:115,
  freeBase:18n,lockedBase:7n,freeQuote:28n,lockedQuote:6n,
  ownedOrders:[first,next]};
const expectedAssetDeltas={
  freeBase:"-2",lockedBase:"2",freeQuote:"-2",lockedQuote:"2",
};
const args={before,after,expectedAssetDeltas,expectedOwnedOrders:[first,next]};
test("all four collateral changes and every maker order must match external ledger",()=>{
  const result=reconcileMakerInventory(args);
  assert.equal(result.mode,"inventory-reconciliation-only");
  assert.equal(result.canSubmit,false);
  assert.equal(result.oldOwnedOrders,1);
  assert.equal(result.newOwnedOrders,2);
  assert.equal(result.assetDeltas.freeBase,"-2");
  assert.equal(result.requiresIndependentReceiptAuthentication,true);
});
test("fail closed for missing receipts, unexplained free or locked asset deltas",()=>{
  assert.throws(()=>reconcileMakerInventory({...args,expectedAssetDeltas:null}),/Complete/);
  for(const key of Object.keys(expectedAssetDeltas)){
    assert.throws(()=>reconcileMakerInventory({
      ...args,expectedAssetDeltas:{...expectedAssetDeltas,[key]:"0"},
    }),/Unexplained/);
  }
  assert.throws(()=>reconcileMakerInventory({
    ...args,expectedAssetDeltas:{...expectedAssetDeltas,unknown:"1"},
  }),/Complete/);
  assert.throws(()=>reconcileMakerInventory({
    ...args,expectedAssetDeltas:{...expectedAssetDeltas,freeBase:"-2.0"},
  }),/signed integer/);
});
test("reject unexpected quote replacement, duplicate sequence, and missing orders",()=>{
  assert.throws(()=>reconcileMakerInventory({
    ...args,expectedOwnedOrders:[first],
  }),/order count/);
  assert.throws(()=>reconcileMakerInventory({
    ...args,expectedOwnedOrders:[first,{...next,baseQty:"3"}],
  }),/contents/);
  assert.throws(()=>reconcileMakerInventory({
    ...args,after:{...after,ownedOrders:[first,first]},
  }),/Duplicate/);
  assert.throws(()=>reconcileMakerInventory({
    ...args,after:{...after,ownedOrders:[first,{...next,sequence:"0"}]},
  }),/Invalid maker owned order/);
});
test("reject stale, not-finalized, regression, or unsafe-number snapshots",()=>{
  assert.throws(()=>reconcileMakerInventory({
    ...args,after:{...after,slot:110},
  }),/did not advance/);
  assert.throws(()=>reconcileMakerInventory({
    ...args,after:{...after,finalizedTransactionSlot:100},
  }),/did not advance/);
  assert.throws(()=>reconcileMakerInventory({
    ...args,after:{...after,mode:"observation-only"},
  }),/Invalid finalized/);
  assert.throws(()=>reconcileMakerInventory({
    ...args,after:{...after,freeBase:18},
  }),/Invalid authenticated/);
  assert.throws(()=>reconcileMakerInventory({
    ...args,after:{...after,canSubmit:true},
  }),/Invalid finalized/);
});
