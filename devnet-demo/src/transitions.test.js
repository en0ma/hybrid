import test from "node:test";
import assert from "node:assert/strict";
import {compareMakerSnapshots} from "./transitions.js";
const make=(freeBase,lockedBase,freeQuote,lockedQuote)=>({market:"m",maker:"u",makerState:{
 market:"m",owner:"u",freeBase:String(freeBase),lockedBase:String(lockedBase),
 freeQuote:String(freeQuote),lockedQuote:String(lockedQuote)}});
test("deposit and withdrawal only change corresponding free collateral",()=>{
 const a=make(0,0,0,0),b=make(100,0,0,0),c=make(100,0,5,0),d=make(100,0,2,0);
 assert.equal(compareMakerSnapshots(a,b,"deposit","base","100").checked,true);
 assert.equal(compareMakerSnapshots(b,c,"deposit","quote","5").delta.freeQuote,"5");
 assert.equal(compareMakerSnapshots(c,d,"withdraw","quote","3").delta.freeQuote,"-3");
 assert.throws(()=>compareMakerSnapshots(a,make(100,1,0,0),"deposit","base","100"));
 assert.throws(()=>compareMakerSnapshots(a,b,"withdraw","base","100"));
});
