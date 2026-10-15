import test from "node:test";
import assert from "node:assert/strict";
import {verifyAskTradeSettlement} from "./trade-settlement.js";
const pre={market:"m",maker:"owner",cluster:"surfpool",rpc:"http://127.0.0.1:8899",
 takerBalances:{base:"0",quote:"1000"},vaultBalances:{base:"100",quote:"0"},
 custodyState:{totalBase:"100",totalQuote:"0"},makerState:{lockedBase:"100",freeQuote:"0",freeBase:"0",lockedQuote:"0"},
 marketState:{nextSequence:"2",askCount:1}};
const post={...pre,takerBalances:{base:"20",quote:"980"},vaultBalances:{base:"80",quote:"20"},
 custodyState:{totalBase:"80",totalQuote:"20"},makerState:{lockedBase:"80",freeQuote:"20",freeBase:"0",lockedQuote:"0"}};
test("ask trade conserves taker/vault atoms and reconciles custody/maker",()=>{
 assert.equal(verifyAskTradeSettlement(pre,post).verified,true);
});
test("rejects forged vault, custody, maker and absent fills",()=>{
 assert.throws(()=>verifyAskTradeSettlement(pre,{...post,vaultBalances:{base:"81",quote:"20"}}),/vault/);
 assert.throws(()=>verifyAskTradeSettlement(pre,{...post,custodyState:{totalBase:"81",totalQuote:"20"}}),/custody/);
 assert.throws(()=>verifyAskTradeSettlement(pre,{...post,makerState:{...post.makerState,freeQuote:"19"}}),/maker/);
 assert.throws(()=>verifyAskTradeSettlement(pre,{...post,takerBalances:{base:"0",quote:"980"}}),/positive/);
});

test("partial filled ask ties on-chain book reduction to exact taker base tokens",()=>{
 const order={sequence:"1",owner:"owner",pageIndex:0,priceX64:"18446744073709551616",
  sqrtPriceX64:"18446744073709551616",baseQty:"100"};
 const a={...pre,bookOrders:{ask:[order],bid:[]}};
 const b={...post,bookOrders:{ask:[{...order,baseQty:"80"}],bid:[]}};
 assert.equal(verifyAskTradeSettlement(a,b).partialBookFillVerified,true);
 assert.throws(()=>verifyAskTradeSettlement(a,{...b,bookOrders:{ask:[{...order,baseQty:"81"}],bid:[]}}),/book quantity/);
 assert.throws(()=>verifyAskTradeSettlement(a,{...b,bookOrders:{ask:[{...order,owner:"attacker",baseQty:"80"}],bid:[]}}),/identity/);
 assert.throws(()=>verifyAskTradeSettlement(a,{...b,bookOrders:{ask:[],bid:[]}}),/partially fill/);
});
