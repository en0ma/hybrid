import test from "node:test";
import assert from "node:assert/strict";
import {PublicKey} from "@solana/web3.js";
import {decodeMakerBalance} from "./maker-balance.js";
test("decode Rust maker collateral layout with full u64 precision",()=>{
 const market=new PublicKey(new Uint8Array(32).fill(3)),owner=new PublicKey(new Uint8Array(32).fill(4));
 const b=new Uint8Array(112);b.set(new TextEncoder().encode("HYBBAL01"));b[8]=1;
 b.set(market.toBytes(),16);b.set(owner.toBytes(),48);
 const v=new DataView(b.buffer);v.setBigUint64(80,(1n<<64n)-1n,true);
 v.setBigUint64(88,17n,true);v.setBigUint64(96,26n,true);
 const result=decodeMakerBalance(b,market.toBase58(),owner.toBase58());
 assert.equal(result.freeBase,((1n<<64n)-1n).toString());
 assert.equal(result.lockedBase,"17");
 assert.equal(result.freeQuote,"26");
 assert.throws(()=>decodeMakerBalance(b,owner.toBase58(),market.toBase58()));
 const corrupted=b.slice();corrupted[0]=0;assert.throws(()=>decodeMakerBalance(corrupted,market.toBase58(),owner.toBase58()));
});
