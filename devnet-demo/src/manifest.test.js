import test from "node:test";
import assert from "node:assert/strict";
import {PublicKey} from "@solana/web3.js";
import {deriveMarketAccounts} from "./manifest.js";
import {PROGRAM_ID} from "./abi.js";
const program=new PublicKey(PROGRAM_ID);
const markets=[new PublicKey(new Uint8Array(32).fill(3)),new PublicKey(new Uint8Array(32).fill(4))];
const maker=new PublicKey(new Uint8Array(32).fill(5));
test("manifest derives canonical Solana PDAs for each side and page",()=>{
  const m=markets[0],actual=deriveMarketAccounts(m.toBase58(),maker.toBase58());
  const expected=(seed,index)=>PublicKey.findProgramAddressSync([
    Buffer.from(seed),m.toBuffer(),Buffer.from([index,0,0,0])
  ],program)[0].toBase58();
  assert.equal(actual.askPage0,expected("ask-page",0));
  assert.equal(actual.bidPage1,expected("bid-page",1));
  assert.equal(actual.askOwnerPage1,expected("ask-owner-page",1));
  assert.equal(actual.bidOwnerPage1,expected("bid-owner-page",1));
  assert.ok(actual.makerBalance);
  assert.equal(actual.programId,PROGRAM_ID);
  assert.ok(!("askOwnerPage0" in actual));
  assert.ok(!("bidOwnerPage0" in actual));
});
test("different markets and makers cannot reuse account derivations",()=>{
  const a=deriveMarketAccounts(markets[0].toBase58(),maker.toBase58());
  const b=deriveMarketAccounts(markets[1].toBase58(),maker.toBase58());
  const otherMaker=new PublicKey(new Uint8Array(32).fill(6));
  const c=deriveMarketAccounts(markets[0].toBase58(),otherMaker.toBase58());
  for(const key of ["custody","vaultAuthority","askPage0","bidPage0","makerBalance"])assert.notEqual(a[key],b[key]);
  assert.notEqual(a.makerBalance,c.makerBalance);
  assert.notEqual(a.askPage0,a.bidPage0);
  assert.notEqual(a.askPage0,a.askPage1);
});
test("missing maker does not invent a maker collateral account",()=>{
  const a=deriveMarketAccounts(markets[0].toBase58());
  assert.ok(!("makerBalance" in a));
});
