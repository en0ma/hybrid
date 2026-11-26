import test from "node:test";
import assert from "node:assert/strict";
import * as web3 from "@solana/web3.js";
import {createSolanaSettlementAdapter} from "../src/settlement-solana.mjs";
import {authenticateSettlementCollateral,loadSettlementEpochs} from "../src/settlement-rpc.mjs";
import {prepareSettlement} from "../src/settlement-plan.mjs";
import {recoverEpochProofs,buildSettlementClaim} from "../src/settlement.mjs";
import {hashPubkeyFrozenEpochV1,hashPubkeyMakerClaim} from "../src/frozen-pubkey-claim.mjs";
import {simulateSettlement,journalSettlementClaim,recoverSettlementClaim} from "../src/settlement-lifecycle.mjs";
import {mkdtemp,rm} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join} from "node:path";
const adapter=createSolanaSettlementAdapter(web3);
const key=()=>web3.Keypair.generate().publicKey;
const bytes=k=>Buffer.from(k.toBytes());
const hex=k=>bytes(k).toString("hex");
const Q=1n<<64n;
function fixture(){
  const programId=key(),market=key(),genesis=key(),ownerPage=key();
  const maker=web3.Keypair.generate(),taker=web3.Keypair.generate(),payer=web3.Keypair.generate();
  const baseMint=key(),quoteMint=key(),baseVault=key(),quoteVault=key(),takerBase=key(),takerQuote=key(),destination=key();
  const tokenProgram=new web3.PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
  const pda=(name,...seeds)=>adapter.findProgramAddressSync([Buffer.from(name),bytes(market),...seeds],programId);
  const [reserve,reserveBump]=pda("lazy-reserve"),[custody,custodyBump]=pda("custody"),
    [authority]=pda("vault-authority"),[page]=pda("ask-page",Buffer.alloc(4)),
    [balance,balanceBump]=pda("maker-balance",bytes(maker.publicKey)),
    [epoch,epochBump]=pda("frozen-epoch",Buffer.alloc(8));
  const accounts=new Map();
  const put=(k,data,owner=programId)=>accounts.set(k.toBase58(),{data,owner,executable:false});
  const h=Buffer.alloc(128);h.write("HYBRID01");h[8]=1;h.writeUInt16LE(1,10);h.writeUInt32LE(1,12);
  h.writeBigUInt64LE(2n,32);h.writeBigUInt64LE(1n,56);h.writeBigUInt64LE(1n,80);h.writeBigUInt64LE(1n,88);
  bytes(ownerPage).copy(h,96);put(market,h);
  const r=Buffer.alloc(144);r.write("HYBLAZY1");r[8]=1;r[9]=reserveBump;bytes(market).copy(r,16);bytes(genesis).copy(r,48);put(reserve,r);
  const book=Buffer.alloc(1552);book.writeUInt16LE(1);book.writeBigUInt64LE(1n,24);
  book.writeBigUInt64LE(1n,40);book.writeBigUInt64LE(100n,48);book.writeBigUInt64LE(1n,56);put(page,book);
  const owners=Buffer.alloc(1040);owners.writeUInt16LE(1);bytes(maker.publicKey).copy(owners,16);put(ownerPage,owners);
  const c=Buffer.alloc(192);c.write("HYBCUST1");c[8]=1;c[9]=custodyBump;c[10]=6;c[11]=6;
  [market,baseMint,quoteMint,baseVault,quoteVault].forEach((k,i)=>bytes(k).copy(c,16+32*i));
  c.writeBigUInt64LE(100n,176);put(custody,c);
  const b=Buffer.alloc(112);b.write("HYBBAL01");b[8]=1;b[9]=balanceBump;bytes(market).copy(b,16);
  bytes(maker.publicKey).copy(b,48);b.writeBigUInt64LE(100n,88);put(balance,b);
  for(const m of [baseMint,quoteMint]){const data=Buffer.alloc(82);data[44]=6;data[45]=1;put(m,data,tokenProgram);}
  function token(k,m,o,n){const data=Buffer.alloc(165);bytes(m).copy(data);bytes(o).copy(data,32);
    data.writeBigUInt64LE(n,64);data[108]=1;put(k,data,tokenProgram);return data;}
  const baseData=token(baseVault,baseMint,authority,100n);
  token(quoteVault,quoteMint,authority,0n);token(takerBase,baseMint,taker.publicKey,0n);
  token(takerQuote,quoteMint,taker.publicKey,200n);token(destination,quoteMint,maker.publicKey,0n);
  let slot=20;
  const connection={getMultipleAccountsInfoAndContext:async(keys)=>({context:{slot:slot++},
    value:keys.map(k=>accounts.get(k.toBase58())??null)})};
  const request={adapter,programId,market,side:"ask",headOwnerPage:ownerPage,genesisHex:hex(genesis),
    taker:taker.publicKey,takerBase,takerQuote};
  return {connection,request,programId,market,genesis,maker,payer,baseData,b,baseVault,quoteVault,baseMint,quoteMint,
    reserve,r,custody,authority,balance,epoch,epochBump,tokenProgram,destination,put};
}
test("canonical RPC collateral snapshot and quote/build parity; rejects vault and maker deficits",async()=>{
  const f=fixture(),s=await authenticateSettlementCollateral(f.connection,f.request);
  assert.equal(s.floor.base,100n);
  const prepared=prepareSettlement(f.programId,s,{payer:f.payer.publicKey,systemProgram:web3.SystemProgram.programId},
    {baseFill:100n,quoteLimit:101n});
  assert.equal(prepared.instruction.data[0],41);
  assert.equal(prepared.plan.quoteTotal,101n);
  f.baseData.writeBigUInt64LE(99n,64);
  await assert.rejects(authenticateSettlementCollateral(f.connection,f.request),/Vault deficit/);
  f.baseData.writeBigUInt64LE(100n,64);
  f.b.writeBigUInt64LE(99n,88);
  await assert.rejects(authenticateSettlementCollateral(f.connection,f.request),/Maker collateral deficit/);
});
test("durable claim journal, exact signed wire, unknown signature and finalized bitmap recovery",async()=>{
  const f=fixture();
  const makers=[{makerHex:hex(f.maker.publicKey),weight:"100",amounts:["100","0","0","100","0","1"]}];
  const totals=makers[0].amounts;
  const spec={marketHex:hex(f.market),programHex:hex(f.programId),genesisHex:hex(f.genesis),
    epochId:"0",tickSqrtPriceX64:String(Q),makers,totals};
  const commitment=hashPubkeyFrozenEpochV1(spec),root=hashPubkeyMakerClaim(commitment,{...makers[0],seat:0});
  const e=Buffer.alloc(912);e.write("HYBEPOCH");e[8]=1;e[9]=f.epochBump;e[10]=1;
  for(const [i,v] of [[16,spec.marketHex],[48,spec.marketHex],[80,spec.programHex],[112,spec.genesisHex],
    [168,commitment],[200,root],[284,hex(f.baseVault)],[316,hex(f.quoteVault)]])Buffer.from(v,"hex").copy(e,i);
  e.writeBigUInt64LE(1n,160);e.writeUInt32LE(1,232);
  totals.forEach((n,i)=>{e.writeBigUInt64LE(BigInt(n),236+8*i);e.writeBigUInt64LE(BigInt(n),352+8*i);});
  f.r.writeBigUInt64LE(1n,80);f.put(f.epoch,e);
  const [epoch]=(await loadSettlementEpochs(f.connection,{adapter,programId:f.programId,market:f.market,
    genesisHex:hex(f.genesis),epochIds:[0n]})).epochs;
  const [proof]=recoverEpochProofs(epoch,makers);
  const accounts={market:f.market,epoch:f.epoch,makerBalance:f.balance,maker:f.maker.publicKey,custody:f.custody,
    reserve:f.reserve,vault:f.quoteVault,destination:f.destination,mint:f.quoteMint,vaultAuthority:f.authority,
    tokenProgram:f.tokenProgram};
  const ix=buildSettlementClaim(f.programId,accounts,epoch,proof);
  const tx=adapter.compileUnsignedV0({payer:f.payer.publicKey,recentBlockhash:key().toBase58(),instruction:ix});
  tx.sign([f.payer,f.maker]);
  const alphabet="123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
  function base58(b){let n=BigInt("0x"+Buffer.from(b).toString("hex")),s="";
    while(n){s=alphabet[Number(n%58n)]+s;n/=58n;}for(const v of b){if(v)break;s="1"+s;}return s;}
  const signature=base58(tx.signatures[0]);
  const evidence=await simulateSettlement({simulateTransaction:async()=>({context:{slot:21},value:{err:null,unitsConsumed:50000}})},
    {adapter,transaction:tx,prepared:{mode:"unsigned-only",slot:20,instruction:ix},currentSlot:21});
  const dir=await mkdtemp(join(tmpdir(),"hybrid-recovery-"));
  try{
    const path=join(dir,"claim.json");
    await journalSettlementClaim(path,{adapter,programId:f.programId,accounts,epoch,proof,transaction:tx,
      simulationEvidence:evidence,signature,genesisHash:f.genesis.toBase58(),lastValidBlockHeight:30});
    const connection={...f.connection,getGenesisHash:async()=>f.genesis.toBase58(),getBlockHeight:async()=>31,
      getSignatureStatuses:async()=>({context:{slot:21},value:[null]})};
    const request={adapter,path,programId:f.programId,market:f.market};
    const pending=await recoverSettlementClaim(connection,request);
    assert.equal(pending.state,"unknown");assert.equal(pending.canResubmit,false);
    e.writeUInt32LE(1,348);e.fill(0,352,400);e[400]=1;
    const recovered=await recoverSettlementClaim(connection,request);
    assert.equal(recovered.state,"claimed");assert.equal(recovered.canResubmit,false);
  }finally{await rm(dir,{recursive:true,force:true});}
});
