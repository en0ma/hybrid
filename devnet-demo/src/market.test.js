import test from "node:test";
import assert from "node:assert/strict";
import {PublicKey} from "@solana/web3.js";
import {decodeMarketHeader,verifyMarketActivation} from "./market.js";
test("decode Rust market header and catch inactive or mismatched owner",()=>{
 const b=new Uint8Array(128);
 b.set(new TextEncoder().encode("HYBRID01"));b[8]=1;
 const v=new DataView(b.buffer);v.setUint16(10,1,true);v.setBigUint64(32,1n,true);
 v.setBigUint64(80,100n,true);v.setBigUint64(88,100n,true);
 b[96]=7;
 const owner=new PublicKey(b.slice(96,128)).toBase58();
 assert.equal(verifyMarketActivation(decodeMarketHeader(b),owner),true);
 assert.throws(()=>verifyMarketActivation(decodeMarketHeader(b),PublicKey.default.toBase58()));
 v.setUint16(10,0,true);
 assert.throws(()=>verifyMarketActivation(decodeMarketHeader(b),owner));
 b[0]=0;assert.throws(()=>decodeMarketHeader(b));
});
