import test from "node:test";
import assert from "node:assert/strict";
import {decimalFromQ64, verifiedMidPrice, checkSlotFreshness, observeLiveQuotes} from "./live-observer.mjs";
const Q64=1n<<64n;
const book=(side, price, slot=100)=>({side,slot,orders:[{priceX64:price}]});

test("Q64 prices produce deterministic decimal atoms without floats",()=>{
  assert.equal(decimalFromQ64(Q64),"1");
  assert.equal(decimalFromQ64(Q64/2n),"0.5");
  assert.equal(decimalFromQ64(Q64*2n + Q64/4n),"2.25");
  assert.throws(()=>decimalFromQ64(0n),/Invalid/);
});
test("mid price requires same-slot, ordered, two-sided market",()=>{
  const ask=book("ask",2n*Q64);
  const bid=book("bid",Q64);
  assert.equal(verifiedMidPrice(ask,bid),"1.5");
  assert.throws(()=>verifiedMidPrice(ask,{...bid,slot:101}),/slots/);
  assert.throws(()=>verifiedMidPrice(ask,book("bid",2n*Q64)),/Crossed/);
  assert.throws(()=>verifiedMidPrice(ask,book("bid",3n*Q64)),/Crossed/);
  assert.throws(()=>verifiedMidPrice(ask,{...bid,orders:[]}),/Both/);
  assert.throws(()=>verifiedMidPrice({...ask,side:"bid"},bid),/Mixed/);
});
test("slot skew guard rejects stale, future and malformed observations",()=>{
  checkSlotFreshness(100,110,10);
  assert.throws(()=>checkSlotFreshness(100,111,10),/Stale/);
  assert.throws(()=>checkSlotFreshness(101,100,10),/Stale/);
  assert.throws(()=>checkSlotFreshness(0,100,10),/Stale/);
  assert.throws(()=>checkSlotFreshness(100,100,-1),/Stale/);
});

test("live observer fails closed on missing or unauthenticated RPC accounts",async()=>{
  let sends=0;
  const connection={
    getMultipleAccountsInfoAndContext:async()=>({
      context:{slot:100}, value:[null,null,null]
    }),
    getSlot:async()=>100,
    sendTransaction:async()=>{sends++;throw Error("must not submit");}
  };
  await assert.rejects(
    ()=>observeLiveQuotes(connection,{
      programId:"program",market:"market",
      askPage:"ask",askOwnerPage:"askOwner",
      bidPage:"bid",bidOwnerPage:"bidOwner",
      strategy:{
        spreadBps:50,sizeAtoms:"1",maxBaseAtoms:"1",maxQuoteAtoms:"1"
      }
    }),
    /Missing account/
  );
  assert.equal(sends,0);
});

function put128(b,i,v){b.writeBigUInt64LE(v&((1n<<64n)-1n),i);b.writeBigUInt64LE(v>>64n,i+8);}
const key=v=>({toBytes:()=>Buffer.alloc(32,v),equals(o){return Buffer.from(o?.toBytes?.()??[]).equals(this.toBytes());}});
function sqrt(n) {
  let x=n,y=(x+1n)/2n;
  while(y<x){x=y;y=(x+n/x)/2n;}
  return x;
}
function rpcFixture({inventorySlot=100,freeBase=2n,freeQuote=3n}={}) {
  const program=key(1),market=key(3),askPage=key(4),bidPage=key(5);
  const askOwnerPage=key(9),bidOwnerPage=key(9),maker=key(7),makerBalance=key(8);
  const m=Buffer.alloc(128);
  m.write("HYBRID01",0,"ascii");m[8]=1;m.writeUInt16LE(1,10);
  m.writeUInt32LE(1,12);m.writeUInt32LE(1,20);
  m.fill(9,96,128);m.fill(9,24,32);m.fill(9,40,48);
  m.writeBigUInt64LE(5n,32);put128(m,48,Q64);put128(m,64,1n);
  m.writeBigUInt64LE(1n,80);m.writeBigUInt64LE(1n,88);
  function page(price,root){
    const p=Buffer.alloc(1552);p.writeUInt16LE(1,0);
    put128(p,16,price);put128(p,32,root);
    p.writeBigUInt64LE(10n,48);p.writeBigUInt64LE(1n,56);return p;
  }
  const owner=Buffer.alloc(1040);owner.writeUInt16LE(1,0);owner.fill(7,16,48);
  const balance=Buffer.alloc(112);balance.write("HYBBAL01",0,"ascii");balance[8]=1;
  Buffer.alloc(32,3).copy(balance,16);Buffer.alloc(32,7).copy(balance,48);
  balance.writeBigUInt64LE(freeBase,80);balance.writeBigUInt64LE(freeQuote,96);
  let count=0,submissions=0;
  const connection={
    async getMultipleAccountsInfoAndContext(){
      const ask=count++===0;
      const p=ask?page(2n*Q64,sqrt(2n*Q64*Q64)):page(Q64,Q64);
      return {context:{slot:100},value:[m,p,owner].map(data=>({owner:program,data}))};
    },
    async getAccountInfoAndContext(){
      return {context:{slot:inventorySlot},value:{owner:program,data:balance}};
    },
    async getSlot(){return 100;},
    async sendTransaction(){submissions++;throw Error("Unexpected submission");}
  };
  return {connection,program,market,askPage,bidPage,askOwnerPage,bidOwnerPage,maker,makerBalance,
    submissions:()=>submissions};
}
function config(f) {
  return {
    programId:f.program,market:f.market,askPage:f.askPage,bidPage:f.bidPage,
    askOwnerPage:f.askOwnerPage,bidOwnerPage:f.bidOwnerPage,
    maker:f.maker,makerBalance:f.makerBalance,
    strategy:{spreadBps:50,levels:1,sizeAtoms:"2",maxBaseAtoms:"100",maxQuoteAtoms:"100"},
  };
}
test("authenticated same-slot maker inventory clamps live quote plan",async()=>{
  const f=rpcFixture();
  const output=await observeLiveQuotes(f.connection,config(f));
  assert.equal(output.mode,"observation-only");
  assert.equal(output.midPriceAtoms,"1.5");
  assert.equal(output.availableBaseAtoms,"2");
  assert.equal(output.availableQuoteAtoms,"3");
  assert.equal(output.plan.orders.length,2);
  assert.equal(output.plan.reservedBaseAtoms,"2");
  assert.equal(output.plan.reservedQuoteAtoms,"3");
  assert.equal(output.transactionsSubmitted,0);
  assert.equal(f.submissions(),0);
});
test("maker collateral cannot be reused across slots or insufficient funds",async()=>{
  const skewed=rpcFixture({inventorySlot:101});
  await assert.rejects(
    observeLiveQuotes(skewed.connection,config(skewed)),/Mixed RPC market and maker collateral slots/
  );
  assert.equal(skewed.submissions(),0);
  const empty=rpcFixture({freeBase:1n,freeQuote:1n});
  const output=await observeLiveQuotes(empty.connection,config(empty));
  assert.equal(output.plan.orders.length,0);
  assert.equal(empty.submissions(),0);
});
