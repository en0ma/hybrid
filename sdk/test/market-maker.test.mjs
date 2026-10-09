import test from "node:test";
import assert from "node:assert/strict";
import { planAuthenticatedMakerUpdate } from "../src/market-maker.mjs";
const Q=1n<<64n;
const key=(v)=>({toBytes:()=>Buffer.alloc(32,v),toString:()=>String(v),
  equals(o){return o?.toString()===String(v)}});
function put128(b,i,v){b.writeBigUInt64LE(v&((1n<<64n)-1n),i);b.writeBigUInt64LE(v>>64n,i+8)}
function fixture({secondSlot=100}={}){
 const programId=key(1),market=key(2),maker=key(7);
 const askPage=key(3),bidPage=key(4),askOwnerPage=key(5),bidOwnerPage=key(6);
 const makerBalance=key(8);
 const make=(side)=>{
  const header=Buffer.alloc(128);header.write("HYBRID01",0);header[8]=1;
  header.writeUInt16LE(1,10);header.writeUInt32LE(1,12);header.writeUInt32LE(1,20);
  header.writeBigUInt64LE(3n,32);put128(header,48,Q);put128(header,64,10n);
  header.writeBigUInt64LE(1n,80);header.writeBigUInt64LE(1n,88);
  header.fill(5,96,128);header.fill(6,24,32);header.fill(6,40,48);
  const orders=Buffer.alloc(1552);orders.writeUInt16LE(1,0);
  put128(orders,16,Q);put128(orders,32,Q);orders.writeBigUInt64LE(10n,48);
  orders.writeBigUInt64LE(side==="ask"?1n:2n,56);
  const owners=Buffer.alloc(1040);owners.writeUInt16LE(1,0);owners.fill(side==="ask"?7:12,16,48);
  return [header,orders,owners].map(data=>({owner:programId,data}));
 };
 let calls=0;
 const connection={async getMultipleAccountsInfoAndContext(){
  const side=calls++===0?"ask":"bid";
  return {context:{slot:side==="ask"?100:secondSlot},value:make(side)};
 }};
 return {connection,opts:{programId,market,askPage,askOwnerPage,bidPage,bidOwnerPage,
   maker,makerBalance,minContextSlot:90}};
}
test("only maker-owned orders can be cancelled or replaced",async()=>{
 const {connection,opts}=fixture();
 const result=await planAuthenticatedMakerUpdate(connection,{
  ...opts,desired:[{side:"ask",priceX64:Q,sqrtPriceX64:Q,baseQty:10n}],
 });
 assert.equal(result.slot,100);
 assert.equal(result.existingCount,1);
 assert.equal(result.plan.unchanged,1);
 assert.equal(result.descriptors.length,0);
});
test("maker replacement builds correct signed order instructions",async()=>{
 const {connection,opts}=fixture();
 const result=await planAuthenticatedMakerUpdate(connection,{
  ...opts,desired:[{side:"ask",priceX64:Q,sqrtPriceX64:Q,baseQty:20n}],
 });
 assert.deepEqual(result.descriptors.map(x=>x.data[0]),[7,6]);
 assert.equal(result.descriptors[0].keys[3].pubkey,opts.makerBalance);
 assert.equal(result.descriptors[0].keys[4].isSigner,true);
});
test("a mixed RPC slot fails closed",async()=>{
 const {connection,opts}=fixture({secondSlot:101});
 await assert.rejects(planAuthenticatedMakerUpdate(connection,{...opts,desired:[]}),/Mixed RPC slots/);
});

test("maker bid removal includes writable balance and final maker signer",async()=>{
 const {connection,opts}=fixture();
 // Maker 7 is not the resting bid owner, so a new desired bid must be placed.
 const result=await planAuthenticatedMakerUpdate(connection,{
  ...opts,desired:[
    {side:"ask",priceX64:Q,sqrtPriceX64:Q,baseQty:10n},
    {side:"bid",priceX64:Q,sqrtPriceX64:Q,baseQty:4n},
  ],
 });
 assert.deepEqual(result.descriptors.map(ix=>ix.data[0]),[8]);
 assert.equal(result.descriptors[0].keys[3].pubkey,opts.makerBalance);
 assert.equal(result.descriptors[0].keys[3].isWritable,true);
 assert.equal(result.descriptors[0].keys[4].pubkey,opts.maker);
 assert.equal(result.descriptors[0].keys[4].isSigner,true);
});
