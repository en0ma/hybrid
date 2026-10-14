import test from "node:test";
import assert from "node:assert/strict";
import {Keypair} from "@solana/web3.js";
import {validateFixtureConfig,lifecycleInstructions} from "./funded-lifecycle.js";
import {deriveMarketAccounts} from "./manifest.js";
const key=()=>Keypair.generate().publicKey.toBase58();
const config=()=>({market:key(),askOwner:key(),bidOwner:key(),baseVault:key(),quoteVault:key(),baseMint:key(),
 quoteMint:key(),makerBaseToken:key(),makerQuoteToken:key(),takerBaseToken:key(),takerQuoteToken:key(),
 priceX64:(1n<<64n).toString(),sqrtPriceX64:(1n<<64n).toString(),placeBaseAtoms:"1000",
 tradeQuoteAtoms:"100",withdrawBaseAtoms:"100"});
test("fixture validates complete funded trading config",()=>{
 const c=config();assert.equal(validateFixtureConfig(c),c);
 const missing=structuredClone(c);delete missing.baseMint;
 assert.throws(()=>validateFixtureConfig(missing),/baseMint/);
 const overspend=structuredClone(c);overspend.withdrawBaseAtoms="1001";
 assert.throws(()=>validateFixtureConfig(overspend),/Withdrawal/);
});
test("six-stage runner matches on-chain account ABI, signer indices and sequence",()=>{
 const c=config(),maker=Keypair.generate().publicKey,taker=Keypair.generate().publicKey;
 const ix=lifecycleInstructions(c,maker,taker,"9");
 assert.deepEqual(Object.keys(ix),["init","deposit","place","swap","cancel","withdraw"]);
 assert.deepEqual(Object.values(ix).map(x=>x.data[0]),[12,13,30,23,31,14]);
 assert.deepEqual(Object.values(ix).map(x=>x.keys.length),[6,8,7,14,7,9]);
 assert.ok(ix.init.keys[3].isSigner&&ix.place.keys[6].isSigner&&ix.swap.keys[2].isSigner);
 assert.ok(ix.place.keys[6].pubkey.equals(maker)&&ix.swap.keys[2].pubkey.equals(taker));
 assert.equal(ix.cancel.data.readBigUInt64LE(3),9n);
 const manifest=deriveMarketAccounts(c.market,maker.toBase58());
 assert.equal(ix.deposit.keys[2].pubkey.toBase58(),manifest.makerBalance);
 assert.equal(ix.swap.keys[13].pubkey.toBase58(),manifest.makerBalance);
 assert.equal(ix.swap.keys[9].pubkey.toBase58(),manifest.askPage0);
 assert.equal(ix.swap.keys[11].pubkey.toBase58(),manifest.askPage1);
});
