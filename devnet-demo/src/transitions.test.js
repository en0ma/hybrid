import test from "node:test";
import assert from "node:assert/strict";
import {compareMakerSnapshots,compareOrderLockSnapshots} from "./transitions.js";
const make=(freeBase,lockedBase,freeQuote,lockedQuote)=>({cluster:"surfpool",rpc:"http://127.0.0.1:8899/",slot:1,market:"m",maker:"u",custodyState:{totalBase:String(freeBase),totalQuote:String(freeQuote)},makerState:{
 market:"m",owner:"u",freeBase:String(freeBase),lockedBase:String(lockedBase),
 freeQuote:String(freeQuote),lockedQuote:String(lockedQuote)},vaultBalances:{base:String(freeBase),quote:String(freeQuote)},marketState:{askCount:0,bidCount:0,nextSequence:"1"}});
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
 placed.marketState={askCount:1,bidCount:0,nextSequence:"2"};
 canceled.marketState={askCount:0,bidCount:0,nextSequence:"2"};
 placed.vaultBalances.base="100";canceled.vaultBalances.base="100";
 placed.custodyState.totalBase="100";canceled.custodyState.totalBase="100";
 assert.equal(compareOrderLockSnapshots(init,placed,"place","ask").amountAtoms,"60");
 assert.equal(compareOrderLockSnapshots(placed,canceled,"cancel","ask").amountAtoms,"35");
 assert.throws(()=>compareOrderLockSnapshots(init,placed,"place","bid"));
 const corrupted=structuredClone(placed);corrupted.vaultBalances.base="41";
 assert.throws(()=>compareOrderLockSnapshots(init,corrupted,"place","ask"),/vault/i);
});

test("exact bid quote lock and remaining cancellation require expected raw atoms",()=>{
 const init=make(100,0,1000,0);
 const placed=make(100,0,700,300);
 const cancelled=make(100,0,820,180);
 placed.marketState={askCount:0,bidCount:1,nextSequence:"2"};
 cancelled.marketState={askCount:0,bidCount:0,nextSequence:"2"};
 for(const state of [placed,cancelled]){
  state.vaultBalances.quote="1000";state.custodyState.totalQuote="1000";
 }
 assert.equal(compareOrderLockSnapshots(init,placed,"place","bid","300").amountAtoms,"300");
 assert.equal(compareOrderLockSnapshots(placed,cancelled,"cancel","bid","120").amountAtoms,"120");
 assert.throws(()=>compareOrderLockSnapshots(init,placed,"place","bid","299"),/expected raw amount/i);
 assert.throws(()=>compareOrderLockSnapshots(placed,cancelled,"cancel","bid","300"),/expected raw amount/i);
 assert.throws(()=>compareOrderLockSnapshots(init,placed,"place","bid","0"),/positive/i);
 const badOpposite=structuredClone(placed);badOpposite.makerState.freeBase="99";
 assert.throws(()=>compareOrderLockSnapshots(init,badOpposite,"place","bid","300"),/opposite/i);
 const badCustody=structuredClone(placed);badCustody.custodyState.totalQuote="999";
 assert.throws(()=>compareOrderLockSnapshots(init,badCustody,"place","bid","300"),/custody/i);
});

test("order header checks reject forged count, unexpected sequence and opposite side movement",()=>{
 const before=make(100,0,100,0),after=make(90,10,100,0);
 after.vaultBalances.base="100";after.custodyState.totalBase="100";
 after.marketState={askCount:1,bidCount:0,nextSequence:"2"};
 assert.equal(compareOrderLockSnapshots(before,after,"place","ask","10").marketHeaderTransitionsChecked,true);
 const badCount=structuredClone(after);badCount.marketState.askCount=0;
 assert.throws(()=>compareOrderLockSnapshots(before,badCount,"place","ask","10"),/count delta/i);
 const badSeq=structuredClone(after);badSeq.marketState.nextSequence="3";
 assert.throws(()=>compareOrderLockSnapshots(before,badSeq,"place","ask","10"),/sequence delta/i);
 const badOther=structuredClone(after);badOther.marketState.bidCount=1;
 assert.throws(()=>compareOrderLockSnapshots(before,badOther,"place","ask","10"),/opposite-side/i);
 const missing=structuredClone(after);delete missing.marketState;
 assert.throws(()=>compareOrderLockSnapshots(before,missing,"place","ask","10"),/Market header/i);
});

test("sequence expectations enforce Rust placement allocation and cancellation bounds",()=>{
 const before=make(100,0,0,0),after=make(80,20,0,0);
 after.vaultBalances.base="100";after.custodyState.totalBase="100";
 before.marketState={askCount:0,bidCount:0,nextSequence:"7"};
 after.marketState={askCount:1,bidCount:0,nextSequence:"8"};
 assert.equal(compareOrderLockSnapshots(before,after,"place","ask","20","7").sequenceExpectationChecked,true);
 assert.throws(()=>compareOrderLockSnapshots(before,after,"place","ask","20","8"),/pre-placement/i);
 const canceled=make(100,0,0,0);
 canceled.vaultBalances.base="100";canceled.custodyState.totalBase="100";
 canceled.marketState={askCount:0,bidCount:0,nextSequence:"8"};
 assert.equal(compareOrderLockSnapshots(after,canceled,"cancel","ask","20","7").sequenceExpectationChecked,true);
 assert.throws(()=>compareOrderLockSnapshots(after,canceled,"cancel","ask","20","8"),/precede/i);
 assert.throws(()=>compareOrderLockSnapshots(after,canceled,"cancel","ask","20","0"),/positive/i);
});
