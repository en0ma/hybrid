import test from "node:test";import assert from "node:assert/strict";import {auditManifestFill} from "./manifest-fill-audit.mjs";
const bal=(accountIndex,mint,owner,amount)=>({accountIndex,mint,owner,uiTokenAmount:{amount:String(amount)}});
const example=()=>({baseMint:"base",quoteMint:"quote",makerOwner:"maker",takerOwner:"taker",before:{market:"market",slot:5,asks:[{trader:"maker",baseAtoms:"100",sequence:"1",priceRaw:"1000"}]},after:{market:"market",slot:7,asks:[{trader:"maker",baseAtoms:"60",sequence:"1",priceRaw:"1000"}]},receipt:{slot:6,meta:{err:null,preTokenBalances:[bal(0,"base","maker",100),bal(1,"base","taker",0),bal(2,"quote","maker",0),bal(3,"quote","taker",100)],postTokenBalances:[bal(0,"base","maker",60),bal(1,"base","taker",40),bal(2,"quote","maker",20),bal(3,"quote","taker",80)]}}});
test("requires actual maker ask reduction and balanced SPL transfers",()=>{const x=auditManifestFill(example());assert.equal(x.verified,false);assert.equal(x.consistencyChecked,true);assert.equal(x.filledBaseAtoms,"40")});
test("rejects pure transfers with no maker fill",()=>{const x=example();x.after.asks=x.before.asks;assert.throws(()=>auditManifestFill(x),/exactly one identified maker ask/)});
test("rejects missing token counterpart in receipt",()=>{const x=example();x.receipt.meta.postTokenBalances[1].uiTokenAmount.amount="30";assert.throws(()=>auditManifestFill(x),/conservation failed/)});
test("rejects token account creation without audited state proof",()=>{const x=example();x.receipt.meta.preTokenBalances.pop();assert.throws(()=>auditManifestFill(x),/created\/closed/)});
test("rejects snapshots of different markets",()=>{const x=example();x.after.market="other";assert.throws(()=>auditManifestFill(x),/snapshots/)});

test("rejects disappearance of maker order (cancel indistinguishable from full fill)",()=>{const x=example();x.after.asks=[];assert.throws(()=>auditManifestFill(x),/cannot distinguish fill from cancel/)});
test("rejects wrong order sequence after execution",()=>{const x=example();x.after.asks[0].sequence="9";assert.throws(()=>auditManifestFill(x),/removed|cancel/)});
test("rejects snapshot without receipt slot bracket",()=>{const x=example();x.before.slot=6;assert.throws(()=>auditManifestFill(x),/chronological/) });
test("rejects unrelated maker order mutation",()=>{const x=example();x.before.asks.push({trader:"other",baseAtoms:"5",sequence:"2",priceRaw:"1000"});x.after.asks.push({trader:"other",baseAtoms:"4",sequence:"2",priceRaw:"1000"});assert.throws(()=>auditManifestFill(x),/unrelated maker/) });
