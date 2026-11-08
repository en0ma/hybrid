import test from "node:test";
import assert from "node:assert/strict";
import {decodeMakerBalance,loadMakerBalanceSnapshot} from "../src/maker-balance.mjs";

const market=Buffer.alloc(32,1), maker=Buffer.alloc(32,2);
const programId=Buffer.alloc(32,3), address=Buffer.alloc(32,4);
function fixture(){
  const b=Buffer.alloc(112);
  b.write("HYBBAL01",0,"ascii"); b[8]=1; b[9]=7;
  market.copy(b,16);maker.copy(b,48);
  b.writeBigUInt64LE(100n,80);
  b.writeBigUInt64LE(40n,88);
  b.writeBigUInt64LE(500n,96);
  b.writeBigUInt64LE(90n,104);
  return b;
}
test("decode maker funds with exact v1 offsets",()=>{
  assert.deepEqual(decodeMakerBalance(fixture(),{market,maker}),{
    bump:7,freeBase:100n,lockedBase:40n,freeQuote:500n,lockedQuote:90n
  });
});
test("reject invalid account encoding and mismatched identities",()=>{
  for(const bad of [
    Buffer.alloc(0),Buffer.alloc(111),Buffer.alloc(113),
    Buffer.from(fixture().map((x,i)=>i===0?0:x)),
    Buffer.from(fixture().map((x,i)=>i===0?(x|0x80):x)),
    Buffer.from(fixture().map((x,i)=>i===8?2:x)),
    Buffer.from(fixture().map((x,i)=>i===12?5:x)),
  ])assert.throws(()=>decodeMakerBalance(bad,{market,maker}));
  assert.throws(()=>decodeMakerBalance(fixture(),{market:Buffer.alloc(32,5),maker}),/mismatch/);
  assert.throws(()=>decodeMakerBalance(fixture(),{market,maker:Buffer.alloc(32,5)}),/mismatch/);
});
test("RPC reader requires owned account and sufficiently fresh context",async()=>{
  const connection={
    getAccountInfoAndContext:async()=>({
      context:{slot:250},value:{owner:programId,data:fixture()}
    })
  };
  assert.equal((await loadMakerBalanceSnapshot(connection,{
    programId,market,maker,makerBalance:address,minContextSlot:250
  })).freeQuote,500n);
  await assert.rejects(()=>loadMakerBalanceSnapshot(connection,{
    programId,market,maker,makerBalance:address,minContextSlot:251
  }),/stale/);
  const counterfeit={
    getAccountInfoAndContext:async()=>({
      context:{slot:250},value:{owner:Buffer.alloc(32,8),data:fixture()}
    })
  };
  await assert.rejects(()=>loadMakerBalanceSnapshot(counterfeit,{
    programId,market,maker,makerBalance:address
  }),/incorrectly owned/);
});
