import test from "node:test";import assert from "node:assert/strict";import {planQuotes} from "./plan.mjs";const c=()=>({midPriceAtoms:"1000",spreadBps:50,levels:2,stepBps:50,sizeAtoms:"3",maxBaseAtoms:"6",maxQuoteAtoms:"10000"});
test("quoting is deterministic with per-side inventory bounds",()=>{const x=planQuotes(c());assert.equal(x.orders.length,4);assert.equal(x.reservedBaseAtoms,"6");assert.ok(BigInt(x.reservedQuoteAtoms)<=10000n);assert.deepEqual(x,planQuotes(c()))});
test("stale market data blocks planning",()=>assert.throws(()=>planQuotes({...c(),stale:true}),/stale/));
test("insufficient quote collateral omits bid side",()=>assert.ok(planQuotes({...c(),maxQuoteAtoms:"1"}).orders.every(o=>o.side==="ask")));
test("reject invalid sizes",()=>assert.throws(()=>planQuotes({...c(),sizeAtoms:"0"}),/inventory/));
