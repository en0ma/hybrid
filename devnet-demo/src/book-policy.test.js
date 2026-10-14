import test from "node:test";
import assert from "node:assert/strict";
import {requireBidSidecar,verifyFullBook} from "./book-policy.js";
test("active bids cannot be silently omitted",()=>{
 assert.throws(()=>requireBidSidecar(1,false,false),/Bid owner sidecar required/);
 assert.throws(()=>requireBidSidecar(0,true,false),/Bid owner sidecar required/);
 assert.equal(requireBidSidecar(0,false,false),true);
 assert.equal(requireBidSidecar(4,true,true),true);
});
test("full-book evidence requires both decoded sides",()=>{
 assert.equal(verifyFullBook({ask:[],bid:[]},true),true);
 assert.equal(verifyFullBook({ask:[]},false),false);
 assert.throws(()=>verifyFullBook({ask:[]},true),/both ask and bid/);
});
