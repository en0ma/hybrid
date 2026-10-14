import test from "node:test";
import assert from "node:assert/strict";
import {verifyFinalLifecycle} from "./final-lifecycle-audit.js";
const base={market:"m",maker:"owner",cluster:"surfpool",rpc:"http://127.0.0.1:8899",
 marketState:{askCount:0,bidCount:0,nextSequence:"1"},
 takerBalances:{base:"0",quote:"1000"},vaultBalances:{base:"0",quote:"0"},
 custodyState:{totalBase:"0",totalQuote:"0"},makerState:null,
 bookOrders:{ask:[],bid:[]}};
const deposit={...base,makerState:{freeBase:"1000",lockedBase:"0",freeQuote:"0",lockedQuote:"0"},
 custodyState:{totalBase:"1000",totalQuote:"0"},vaultBalances:{base:"1000",quote:"0"}};
const trade={...deposit,marketState:{askCount:1,bidCount:0,nextSequence:"2"},
 makerState:{freeBase:"0",lockedBase:"900",freeQuote:"100",lockedQuote:"0"},
 takerBalances:{base:"100",quote:"900"},vaultBalances:{base:"900",quote:"100"},
 custodyState:{totalBase:"900",totalQuote:"100"},
 bookOrders:{ask:[{sequence:"1"}],bid:[]}};
const final={...trade,marketState:{askCount:0,bidCount:0,nextSequence:"2"},
 makerState:{freeBase:"800",lockedBase:"0",freeQuote:"100",lockedQuote:"0"},
 vaultBalances:{base:"800",quote:"100"},custodyState:{totalBase:"800",totalQuote:"100"},
 bookOrders:{ask:[],bid:[]}};
const args={initial:base,deposit,trade,final,placeAmount:"1000",withdrawAmount:"100"};
test("end-to-end audit matches net maker collateral, custody and vaults",()=>{
 const r=verifyFinalLifecycle(args);
 assert.equal(r.verified,true);assert.equal(r.remainingFreeBaseAtoms,"800");
});
test("end-to-end audit rejects stranded locks, altered vaults and unexpected taker flows",()=>{
 assert.throws(()=>verifyFinalLifecycle({...args,final:{...final,makerState:{...final.makerState,lockedBase:"1"}}}),/locked/);
 assert.throws(()=>verifyFinalLifecycle({...args,final:{...final,vaultBalances:{base:"801",quote:"100"}}}),/vault/);
 assert.throws(()=>verifyFinalLifecycle({...args,final:{...final,takerBalances:{base:"101",quote:"900"}}}),/Non-trade/);
 assert.throws(()=>verifyFinalLifecycle({...args,final:{...final,bookOrders:{ask:[{}],bid:[]}}}),/book/);
});
