import test from "node:test";
import assert from "node:assert/strict";
import { reconcileMakerOrders, buildMakerReconciliation } from "../src/maker.mjs";
const q=1n<<64n;
const ask=(sequence,baseQty,priceX64=q)=>({side:"ask",sequence,baseQty,priceX64,sqrtPriceX64:q});
const bid=(sequence,baseQty,priceX64=q)=>({side:"bid",sequence,baseQty,priceX64,sqrtPriceX64:q});
const desired=(x)=>({side:x.side,baseQty:x.baseQty,priceX64:x.priceX64,sqrtPriceX64:x.sqrtPriceX64});
const roles=(side)=>({
 market:"market",page:side+"Page",ownerPage:side+"Owner",maker:"maker",makerBalance:"balance",
});
test("preserves unchanged orders; cancels first and places only changed entries",()=>{
 const plan=reconcileMakerOrders({
  existing:[ask(1n,10n),ask(2n,20n),bid(3n,40n)],
  desired:[desired(ask(99n,10n)),desired(bid(100n,40n)),desired(ask(101n,30n))],
 });
 assert.equal(plan.unchanged,2);
 assert.deepEqual(plan.cancellations,[{side:"ask",sequence:2n}]);
 assert.equal(plan.placements.length,1);
 const ix=buildMakerReconciliation("program",{ask:roles("ask"),bid:roles("bid")},plan);
 assert.deepEqual(ix.map(x=>x.data[0]),[7,6]);
 assert.deepEqual(ix.map(x=>x.keys[1].pubkey),["askPage","askPage"]);
});
test("duplicates are matched once and excess duplicate is canceled",()=>{
 const plan=reconcileMakerOrders({
  existing:[ask(1n,10n),ask(2n,10n)],
  desired:[desired(ask(5n,10n))],
 });
 assert.equal(plan.unchanged,1);
 assert.deepEqual(plan.cancellations,[{side:"ask",sequence:2n}]);
});
test("reject duplicate sequences, bad prices, unsafe numbers, and large batch",()=>{
 assert.throws(()=>reconcileMakerOrders({existing:[ask(1n,10n),bid(1n,10n)],desired:[]}),/Duplicate/);
 assert.throws(()=>reconcileMakerOrders({existing:[],desired:[{side:"ask",priceX64:0n,sqrtPriceX64:q,baseQty:1n}]}),/Zero/);
 assert.throws(()=>reconcileMakerOrders({existing:[],desired:[desired(ask(1n,10n))],maxChanges:0}),/maxChanges/);
 assert.throws(()=>reconcileMakerOrders({existing:[ask(1n,Number.MAX_SAFE_INTEGER+1)],desired:[]}),/safe integer/);
 assert.throws(()=>reconcileMakerOrders({existing:[],desired:[],maxChanges:-1}),/maxChanges/);
});
test("full replacement distinguishes sides and builds bid placement",()=>{
 const plan=reconcileMakerOrders({existing:[ask(1n,10n)],desired:[desired(bid(4n,5n))]});
 const ix=buildMakerReconciliation("program",{ask:roles("ask"),bid:roles("bid")},plan);
 assert.deepEqual(ix.map(x=>x.data[0]),[7,8]);
 assert.equal(ix[1].keys[1].pubkey,"bidPage");
});
