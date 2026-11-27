import test from "node:test";
import assert from "node:assert/strict";
import {PublicKey} from "@solana/web3.js";
import {loadLiveMakerSnapshot} from "../src/maker-live-snapshot.mjs";
const Q=1n<<64n;
const key=n=>new PublicKey(Buffer.alloc(32,n));
const [programId,market,maker,askHeadOwner,bidHeadOwner]=[1,2,3,4,5].map(key);
const expectedGenesisHash="4".repeat(44);
const adapter=PublicKey;
const derive=(...seeds)=>PublicKey.findProgramAddressSync(seeds,programId);
const put128=(b,i,v)=>{b.writeBigUInt64LE(v&(Q-1n),i);b.writeBigUInt64LE(v>>64n,i+8);};
function fixture(){
  const accounts=new Map();
  const set=(k,data)=>accounts.set(k.toBase58(),{owner:programId,executable:false,data});
  const header=Buffer.alloc(128);header.write("HYBRID01");header[8]=1;
  header.writeUInt16LE(1,10);header.writeUInt32LE(2,12);header.writeUInt32LE(1,20);
  header.writeBigUInt64LE(4n,32);put128(header,48,Q);put128(header,64,1n);
  header.writeBigUInt64LE(1n,80);header.writeBigUInt64LE(1n,88);
  askHeadOwner.toBuffer().copy(header,96);
  bidHeadOwner.toBuffer().copy(header,24,0,8);bidHeadOwner.toBuffer().copy(header,40,8,16);
  set(market,header);
  const [balance,bump]=derive(Buffer.from("maker-balance"),market.toBuffer(),maker.toBuffer());
  const inventory=Buffer.alloc(112);inventory.write("HYBBAL01");inventory[8]=1;inventory[9]=bump;
  market.toBuffer().copy(inventory,16);maker.toBuffer().copy(inventory,48);
  inventory.writeBigUInt64LE(100n,80);inventory.writeBigUInt64LE(20n,88);
  inventory.writeBigUInt64LE(100n,96);inventory.writeBigUInt64LE(10n,104);set(balance,inventory);
  const pairs={ask:[],bid:[]};
  for(const side of ["ask","bid"]){
    const count=side==="ask"?2:1;
    for(let index=0;index<count;index++){
      const n=Buffer.alloc(4);n.writeUInt32LE(index);
      const [page]=derive(Buffer.from(side+"-page"),market.toBuffer(),n);
      const ownerPage=index===0?(side==="ask"?askHeadOwner:bidHeadOwner):
        derive(Buffer.from(side+"-owner-page"),market.toBuffer(),n)[0];
      const p=Buffer.alloc(1552),o=Buffer.alloc(1040);
      for(const b of [p,o]){
        b.writeUInt16LE(1);b.writeUInt32LE(index,2);
        b.writeUInt32LE(index?index-1:0xffffffff,6);
        b.writeUInt32LE(index+1<count?index+1:0xffffffff,10);
      }
      put128(p,16,Q);put128(p,32,Q);p.writeBigUInt64LE(10n,48);
      p.writeBigUInt64LE(BigInt(side==="ask"?index+1:3),56);
      maker.toBuffer().copy(o,16);set(page,p);set(ownerPage,o);
      pairs[side].push({page,ownerPage,p,o});
    }
  }
  let reads=0;
  const connection={
    async getGenesisHash(){return expectedGenesisHash;},
    async getSlot(){return 102;},
    async getMultipleAccountsInfoAndContext(keys,opts){
      reads++;assert.equal(keys.length,34);assert.deepEqual(opts,{commitment:"confirmed",minContextSlot:100});
      return {context:{slot:101},value:keys.map(k=>accounts.get(k.toBase58())??null)};
    },
  };
  const args={adapter,programId,market,maker,askHeadOwner,bidHeadOwner,expectedGenesisHash,minContextSlot:100};
  return {connection,args,pairs,accounts,inventory,header,reads:()=>reads};
}
test("one coherent batch observes complete linked depth and maker inventory without submission",async()=>{
  const f=fixture();const s=await loadLiveMakerSnapshot(f.connection,f.args);
  assert.equal(f.reads(),1);assert.equal(s.slot,101);assert.equal(s.canSubmit,false);
  assert.equal(s.asks.length,2);assert.equal(s.bids.length,1);assert.equal(s.ownedOrders.length,3);
  assert.deepEqual(s.requiredLocked,{base:20n,quote:10n});assert.equal(s.inventory.freeBase,100n);
});
test("reject stale or foreign context and mismatched head binding",async()=>{
  const f=fixture();f.connection.getSlot=async()=>110;
  await assert.rejects(loadLiveMakerSnapshot(f.connection,f.args),/stale/);
  const g=fixture();g.connection.getGenesisHash=async()=>"5".repeat(44);
  await assert.rejects(loadLiveMakerSnapshot(g.connection,g.args),/genesis mismatch/);
  const h=fixture();h.args.bidHeadOwner=key(6);
  await assert.rejects(loadLiveMakerSnapshot(h.connection,h.args),/binding mismatch/);
});
test("reject missing live suffix, foreign owner, duplicate global sequence and undercollateralized maker",async()=>{
  const a=fixture();a.accounts.delete(a.pairs.ask[1].page.toBase58());
  await assert.rejects(loadLiveMakerSnapshot(a.connection,a.args),/foreign live account/);
  const b=fixture();b.accounts.get(b.pairs.bid[0].ownerPage.toBase58()).owner=key(9);
  await assert.rejects(loadLiveMakerSnapshot(b.connection,b.args),/foreign live account/);
  const c=fixture();c.pairs.bid[0].p.writeBigUInt64LE(1n,56);
  await assert.rejects(loadLiveMakerSnapshot(c.connection,c.args),/duplicate global/);
  const d=fixture();d.inventory.writeBigUInt64LE(19n,88);
  await assert.rejects(loadLiveMakerSnapshot(d.connection,d.args),/collateral deficit/);
});
