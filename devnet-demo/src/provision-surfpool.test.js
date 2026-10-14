import test from "node:test";
import assert from "node:assert/strict";
import {Keypair,PublicKey} from "@solana/web3.js";
import {marketBytes,detachedBookPage,classicMintBytes,classicTokenBytes,provisioningPlan} from "./provision-surfpool.js";
import {decodeMarketHeader} from "./market.js";
import {decodeOwnedBookPage} from "./book.js";
import {decodeCustody} from "./custody.js";
const actors=["market","maker","taker","askOwner","bidOwner","baseMint","quoteMint","baseVault","quoteVault",
 "makerBaseToken","makerQuoteToken","takerBaseToken","takerQuoteToken"];
const keys=()=>Object.fromEntries(actors.map(n=>[n,Keypair.generate()]));
test("market pre-activation encodes Rust header commitments and sequence",()=>{
 const k=keys(),raw=marketBytes(k.askOwner.publicKey,k.bidOwner.publicKey);
 assert.equal(raw.length,128);
 const h=decodeMarketHeader(raw);
 assert.equal(h.askOwner,k.askOwner.publicKey.toBase58());
 assert.deepEqual([...h.bidOwnerTag],[...k.bidOwner.publicKey.toBytes().slice(0,16)]);
 assert.equal(h.nextSequence,"1");assert.equal(h.collateralizedActive,false);
});
test("canonical detached head/tail layouts can decode as empty pages",()=>{
 const zero=Keypair.generate().publicKey.toBytes();
 for(const side of ["ask","bid"]){
  for(let index=0;index<2;index++){
   const page=decodeOwnedBookPage(detachedBookPage(index),detachedBookPage(index,true),side,index);
   assert.equal(page.orders.length,0);
   assert.equal(page.links.index,index);
  }
 }
});
test("classic SPL mints and seeded vaults keep authority and account byte positions",()=>{
 const k=keys(),mint=classicMintBytes(k.maker.publicKey);
 assert.equal(mint.length,82);assert.equal(mint[44],6);assert.equal(mint[45],1);
 const amount=classicTokenBytes(k.baseMint.publicKey,k.maker.publicKey,123n);
 assert.equal(amount.length,165);assert.equal(amount.readBigUInt64LE(64),123n);
 assert.equal(new PublicKey(amount.subarray(32,64)).toBase58(),k.maker.publicKey.toBase58());
});
test("provisioning plan supplies all required market, book, mint and funded token accounts",()=>{
 const k=keys(),plan=provisioningPlan(k);
 assert.equal(plan.accounts.length,17);
 assert.equal(new Set(plan.accounts.map(x=>x.address)).size,17);
 assert.equal(plan.accounts.find(x=>x.name==="makerBaseToken").data.readBigUInt64LE(64),1000000n);
 assert.equal(plan.accounts.find(x=>x.name==="takerQuoteToken").data.readBigUInt64LE(64),1000000n);
 assert.equal(plan.accounts.find(x=>x.name==="baseVault").data.readBigUInt64LE(64),0n);
});

test("seeded market and classic SPL accounts have nonzero funds and expected byte lengths",()=>{
 const k=keys(),plan=provisioningPlan(k);
 assert.equal(plan.accounts.filter(x=>x.owner!=="US517G5965aydkZ46HS38QLi7UQiSojurfbQfKCELFx").length,6);
 for(const x of plan.accounts)assert.ok(x.data.length===128||x.data.length===1552||x.data.length===1040||x.data.length===82||x.data.length===165);
 const mint=plan.accounts.find(x=>x.name==="baseMint").data;
 assert.equal(mint.readBigUInt64LE(36),1000000n);
});
