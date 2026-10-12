import test from "node:test";
import assert from "node:assert/strict";
import {compareMakerSnapshots,compareOrderLockSnapshots} from "./transitions.js";
const make=(freeBase,lockedBase,freeQuote,lockedQuote)=>({cluster:"surfpool",rpc:"http://127.0.0.1:8899/",slot:1,market:"m",maker:"u",custodyState:{totalBase:String(freeBase),totalQuote:String(freeQuote)},makerState:{
 market:"m",owner:"u",freeBase:String(freeBase),lockedBase:String(lockedBase),
 freeQuote:String(freeQuote),lockedQuote:String(lockedQuote)},vaultBalances:{base:String(freeBase),quote:String(freeQuote)}});
test("deposit and withdrawal only change corresponding free collateral",()=>{
 const a=make(0,0,0,0),b=make(100,0,0,0),c=make(100,0,5,0),d=make(100,0,2,0);
 assert.equal(compareMakerSnapshots(a,b,"deposit","base","100").checked,true);
 assert.equal(compareMakerSnapshots(b,c,"deposit","quote","5").delta.freeQuote,"5");
 assert.equal(compareMakerSnapshots(c,d,"withdraw","quote","3").delta.freeQuote,"-3");
 assert.throws(()=>compareMakerSnapshots(a,make(100,1,0,0),"deposit","base","100"));
 assert.throws(()=>compareMakerSnapshots(a,b,"withdraw","base","100"));
 const forged=structuredClone(b);forged.vaultBalances.base="99";
 assert.throws(()=>compareMakerSnapshots(a,forged,"deposit","base","100"),/vault/i);
 const badCustody=structuredClone(b);badCustody.custodyState.totalBase="99";
 assert.throws(()=>compareMakerSnapshots(a,badCustody,"deposit","base","100"),/Custody/i);
 const stale=structuredClone(b);stale.slot=0;
 assert.throws(()=>compareMakerSnapshots(a,stale,"deposit","base","100"),/slot/i);
});

test("maker ask lock and partial cancellation conserve collateral and keep vaults still",()=>{
 const init=make(100,0,5,0);
 const placed=make(40,60,5,0);
 const canceled=make(75,25,5,0);
 assert.equal(compareOrderLockSnapshots(init,placed,"place","ask").amountAtoms,"60");
 assert.equal(compareOrderLockSnapshots(placed,canceled,"cancel","ask").amountAtoms,"35");
 assert.throws(()=>compareOrderLockSnapshots(init,placed,"place","bid"));
 const corrupted=structuredClone(placed);corrupted.vaultBalances.base="41";
 assert.throws(()=>compareOrderLockSnapshots(init,corrupted,"place","ask"),/vault/i);
});
