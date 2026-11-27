import {planLiveMakerQuotes} from "../src/maker-live-plan.mjs";
import test from "node:test";
import assert from "node:assert/strict";
import * as web3 from "@solana/web3.js";
import {PublicKey,Keypair,VersionedTransaction} from "@solana/web3.js";
import {mkdtemp,rm,writeFile,stat} from "node:fs/promises";
import {tmpdir} from "node:os";
import {join} from "node:path";
import {createSolanaSettlementAdapter} from "../src/settlement-solana.mjs";
import {initializeLiveMakerSession,inspectLiveMakerSession,reserveLiveMakerIntent,
  prepareLiveMakerIntent,observeLiveMakerIntent,submitSandboxMakerIntent,
  finalizeSandboxMakerIntent} from "../src/maker-live-session.mjs";
import {loadLiveMakerSnapshot} from "../src/maker-live-snapshot.mjs";
const Q=1n<<64n;
const key=n=>new PublicKey(Buffer.alloc(32,n));
const [programId,market,askHeadOwner,bidHeadOwner]=[1,2,4,5].map(key);
const makerSigner=Keypair.fromSeed(Buffer.alloc(32,3));
const maker=makerSigner.publicKey;
const expectedGenesisHash="4".repeat(44);
const adapter=createSolanaSettlementAdapter(web3);
const TOKEN=new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
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
  const [custody,custodyBump]=derive(Buffer.from("custody"),market.toBuffer());
  const [authority]=derive(Buffer.from("vault-authority"),market.toBuffer());
  const [pool,poolBump]=derive(Buffer.from("passive-pool"),market.toBuffer());
  const [reserve,reserveBump]=derive(Buffer.from("lazy-reserve"),market.toBuffer());
  const [baseVault,quoteVault,baseMint,quoteMint]=[11,12,13,14].map(key);
  const custodyData=Buffer.alloc(192);custodyData.write("HYBCUST1");custodyData[8]=1;
  custodyData[9]=custodyBump;custodyData[10]=6;custodyData[11]=6;
  [market,baseMint,quoteMint,baseVault,quoteVault].forEach((k,i)=>k.toBuffer().copy(custodyData,16+32*i));
  custodyData.writeBigUInt64LE(120n,176);custodyData.writeBigUInt64LE(110n,184);set(custody,custodyData);
  const token=(k,mint,amount)=>{
    const b=Buffer.alloc(165);mint.toBuffer().copy(b,0);authority.toBuffer().copy(b,32);
    b.writeBigUInt64LE(amount,64);b[108]=1;accounts.set(k.toBase58(),{owner:TOKEN,data:b});
    return b;
  };
  const baseToken=token(baseVault,baseMint,150n),quoteToken=token(quoteVault,quoteMint,150n);
  for(const k of [baseMint,quoteMint]){
    const b=Buffer.alloc(82);b[44]=6;b[45]=1;accounts.set(k.toBase58(),{owner:TOKEN,data:b});
  }
  const addLedgers=()=>{
    header.writeUInt16LE(3,10);
    const p=Buffer.alloc(112);p.write("HYBPOOL1");p[8]=1;p[9]=poolBump;
    market.toBuffer().copy(p,16);p.writeBigUInt64LE(10n,48);p.writeBigUInt64LE(12n,56);
    p.writeBigUInt64LE(2n,80);p.writeBigUInt64LE(3n,88);set(pool,p);
    const r=Buffer.alloc(144);r.write("HYBLAZY1");r[8]=1;r[9]=reserveBump;
    market.toBuffer().copy(r,16);new PublicKey(expectedGenesisHash).toBuffer().copy(r,48);
    r.writeBigUInt64LE(2n,88);r.writeBigUInt64LE(3n,96);
    r.writeBigUInt64LE(1n,120);r.writeBigUInt64LE(2n,128);set(reserve,r);
    return {p,r};
  };
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
      reads++;
      assert.equal(keys.length,reads===1?1:41);
      assert.deepEqual(opts,{commitment:"confirmed",minContextSlot:reads===1?100:101});
      return {context:{slot:101},value:keys.map(k=>accounts.get(k.toBase58())??null)};
    },
  };
  const args={adapter,programId,market,maker,askHeadOwner,bidHeadOwner,expectedGenesisHash,minContextSlot:100};
  return {connection,args,pairs,accounts,inventory,header,custodyData,baseToken,quoteToken,
    baseVault,quoteVault,baseMint,quoteMint,addLedgers,reads:()=>reads};
}
test("one coherent batch observes complete linked depth and maker inventory without submission",async()=>{
  const f=fixture();const s=await loadLiveMakerSnapshot(f.connection,f.args);
  assert.equal(f.reads(),2);assert.equal(s.slot,101);assert.equal(s.canSubmit,false);
  assert.deepEqual(s.floor,{base:120n,quote:110n});
  assert.deepEqual(s.vaultAmounts,{base:150n,quote:150n});
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

test("guarded snapshot includes frozen reserve and passive principal plus fees",async()=>{
  const f=fixture();f.addLedgers();
  const s=await loadLiveMakerSnapshot(f.connection,f.args);
  assert.deepEqual(s.floor,{base:135n,quote:130n});
  assert.equal(s.marketFlags,3);assert.ok(s.aggregateGuard);assert.equal(s.canSubmit,false);
});
test("reject vault deficits, missing pool, wrong reserve domain, frozen vault and wrong mint",async()=>{
  const a=fixture();a.addLedgers();a.baseToken.writeBigUInt64LE(134n,64);
  await assert.rejects(loadLiveMakerSnapshot(a.connection,a.args),/vault deficit/);
  const b=fixture();b.header.writeUInt16LE(3,10);
  await assert.rejects(loadLiveMakerSnapshot(b.connection,b.args),/missing passive pool/);
  const c=fixture();c.addLedgers().r[48]^=1;
  await assert.rejects(loadLiveMakerSnapshot(c.connection,c.args),/reserve domain mismatch/);
  const d=fixture();d.baseToken[108]=2;
  await assert.rejects(loadLiveMakerSnapshot(d.connection,d.args),/frozen SPL vault/);
  const e=fixture();e.accounts.get(e.baseMint.toBase58()).data[44]=7;
  await assert.rejects(loadLiveMakerSnapshot(e.connection,e.args),/Invalid SPL mint/);
});
test("reject discovery identity changes and inventory in excess of custody",async()=>{
  const a=fixture();const fetch=a.connection.getMultipleAccountsInfoAndContext;
  a.connection.getMultipleAccountsInfoAndContext=async(keys,opts)=>{
    const result=await fetch(keys,opts);
    if(a.reads()===1){
      result.value[0]={...result.value[0],data:Buffer.from(result.value[0].data)};
      a.custodyData[112]^=1;
    }
    return result;
  };
  await assert.rejects(loadLiveMakerSnapshot(a.connection,a.args),/Custody identity changed/);
  const b=fixture();b.inventory.writeBigUInt64LE(101n,80);
  await assert.rejects(loadLiveMakerSnapshot(b.connection,b.args),/exceeds custody/);
});

test("lazy debits explain nominal inventory before claim but never authorize spending",async()=>{
  const f=fixture();const {r}=f.addLedgers();
  f.custodyData.writeBigUInt64LE(110n,176);
  r.writeBigUInt64LE(10n,104);
  const s=await loadLiveMakerSnapshot(f.connection,f.args);
  assert.equal(s.nominalBound.base,120n);
  assert.equal(s.inventory.freeBase+s.inventory.lockedBase,120n);
  assert.equal(s.pendingLazySettlement,true);
  assert.equal(s.canSubmit,false);
  // One atom more than authenticated pending debits remains invalid.
  const g=fixture();g.addLedgers().r.writeBigUInt64LE(9n,104);
  g.custodyData.writeBigUInt64LE(110n,176);
  await assert.rejects(loadLiveMakerSnapshot(g.connection,g.args),/exceeds custody/);
});

const policy={enabled:true,killSwitch:false,pendingMutation:false,maxLagSlots:4,maxOrders:8,maxChanges:8,
  maxBaseExposure:"200",maxQuoteNotional:"100",maxLossQuote:"0",baselineEquityQuote:"230",
  maxOrdersPerWindow:8,maxTransactionsPerWindow:8,maxRetries:2,ordersInWindow:0,transactionsInWindow:0,retries:0};
const targets=[{side:"ask",priceAtoms:"1",sizeAtoms:"10"},{side:"ask",priceAtoms:"1",sizeAtoms:"10"},
  {side:"bid",priceAtoms:"1",sizeAtoms:"10"}];
test("advisory exact targets match existing quotes without reusing locked funds",async()=>{
  const f=fixture();
  const p=await planLiveMakerQuotes(f.connection,{snapshotOptions:f.args,orders:targets,policy});
  assert.equal(p.phase,"noop");assert.equal(p.canSubmit,false);assert.equal(p.markedEquityQuote,230n);
});
test("cancel only before any replacement and reject unsupported linked page changes",async()=>{
  const f=fixture();
  const p=await planLiveMakerQuotes(f.connection,{snapshotOptions:f.args,orders:targets.slice(0,2),policy});
  assert.equal(p.phase,"cancel");assert.equal(p.actions.length,1);assert.equal(p.requiresFreshSnapshot,true);
  const g=fixture();
  await assert.rejects(planLiveMakerQuotes(g.connection,{snapshotOptions:g.args,orders:[],policy}),/multi-page cancellation/);
});
test("advisory risk fails closed on loss, exposure, rate and unresolved claims",async()=>{
  for(const [patch,pattern] of [[{baselineEquityQuote:"231"},/loss limit/],
    [{maxBaseExposure:"129"},/Base exposure/],[{maxQuoteNotional:"29"},/notional/],
    [{ordersInWindow:8},/rate limit/]]){
    const f=fixture();
    await assert.rejects(planLiveMakerQuotes(f.connection,{snapshotOptions:f.args,
      orders:targets.slice(0,2),policy:{...policy,...patch}}),pattern);
  }
  const f=fixture();f.addLedgers();
  await assert.rejects(planLiveMakerQuotes(f.connection,{snapshotOptions:f.args,orders:targets,policy}),/Pending lazy claims/);
});

const control={enabled:true,killSwitch:false};
async function sessionFixture(callback){
  const root=await mkdtemp(join(tmpdir(),"hybrid-live-session-"));
  try{
    const f=fixture(),sessionPath=join(root,"maker.session");
    f.connection.getLatestBlockhashAndContext=async opts=>{
      assert.deepEqual(opts,{commitment:"confirmed",minContextSlot:102});
      return {context:{slot:102},value:{blockhash:key(99).toBase58(),lastValidBlockHeight:500}};
    };
    f.connection.rpcEndpoint="http://127.0.0.1:8899";
    f.connection.getBlockHeight=async()=>499;
    f.connection.simulateTransaction=async(tx,opts)=>{
      assert.equal(opts.sigVerify,true);assert.equal(opts.replaceRecentBlockhash,false);
      assert.ok(tx.signatures[0].some(Boolean));return {context:{slot:102},value:{err:null,unitsConsumed:12345}};
    };
    f.connection.getSignatureStatuses=async()=>({context:{slot:103},value:[null]});
    await initializeLiveMakerSession({sessionPath,snapshotOptions:f.args,policy,acknowledgeEmptySession:true});
    const reserve=()=>reserveLiveMakerIntent(f.connection,{sessionPath,snapshotOptions:f.args,
      orders:targets.slice(0,2),control});
    await callback({f,sessionPath,reserve});
  }finally{await rm(root,{recursive:true,force:true});}
}
test("durable single-intent reservation, exact signed preparation and unknown recovery",async()=>{
  await sessionFixture(async({f,sessionPath,reserve})=>{
    const r=await reserve();
    assert.equal(r.phase,"reserved");assert.equal(r.canSubmit,false);
    const saved=await inspectLiveMakerSession(sessionPath);
    assert.equal(saved.ordersReserved,1);assert.equal(saved.transactionsReserved,1);
    assert.equal(saved.intent.phase,"cancel");assert.equal(saved.intent.action.sequence,"3");
    assert.equal((await stat(sessionPath)).mode&0o777,0o600);
    const tx=VersionedTransaction.deserialize(Buffer.from(r.unsignedWireBase64,"base64"));
    tx.sign([makerSigner]);
    const prepared=await prepareLiveMakerIntent(f.connection,{sessionPath,snapshotOptions:f.args,
      transaction:tx,control});
    assert.equal(prepared.phase,"prepared");assert.equal(prepared.canSubmit,false);
    assert.equal((await inspectLiveMakerSession(sessionPath)).phase,"prepared");
    const status=await observeLiveMakerIntent(f.connection,{sessionPath,snapshotOptions:f.args});
    assert.equal(status.state,"unknown");assert.equal(status.canRetry,false);
    await assert.rejects(reserve(),/Pending maker intent/);
  });
});
test("reserved intent cannot be replaced by a different signed message",async()=>{
  await sessionFixture(async({f,sessionPath,reserve})=>{
    const r=await reserve();
    const tx=VersionedTransaction.deserialize(Buffer.from(r.unsignedWireBase64,"base64"));
    tx.message.recentBlockhash=key(98).toBase58();tx.sign([makerSigner]);
    await assert.rejects(prepareLiveMakerIntent(f.connection,{sessionPath,snapshotOptions:f.args,
      transaction:tx,control}),/differs from durable reservation/);
    assert.equal((await inspectLiveMakerSession(sessionPath)).phase,"reserved");
    assert.ok((await stat(sessionPath+".lock")).size>0);
  });
});
test("kill switch and interrupted durable state block reservation",async()=>{
  await sessionFixture(async({f,sessionPath,reserve})=>{
    await assert.rejects(reserveLiveMakerIntent(f.connection,{sessionPath,snapshotOptions:f.args,
      orders:targets.slice(0,2),control:{enabled:true,killSwitch:true}}),/disabled/);
    await writeFile(sessionPath+".pending","torn update");
    await assert.rejects(reserve(),/Interrupted session/);
  });
});

const sandboxControl={...control,sandboxSubmit:true};
async function preparedFixture(callback){
  await sessionFixture(async({f,sessionPath,reserve})=>{
    const r=await reserve();
    const transaction=VersionedTransaction.deserialize(Buffer.from(r.unsignedWireBase64,"base64"));
    transaction.sign([makerSigner]);
    const prepared=await prepareLiveMakerIntent(f.connection,{sessionPath,snapshotOptions:f.args,transaction,control});
    // Fresh batch at the same simulated slot; the prior fixture deliberately
    // asserted its initial two discovery/read contexts.
    const later=fixture();
    let reads=0;
    f.connection.getMultipleAccountsInfoAndContext=async(keys,opts)=>{
      reads++;
      assert.equal(keys.length,reads===1?1:41);
      assert.equal(opts.minContextSlot,102);
      return {context:{slot:102},value:keys.map(k=>later.accounts.get(k.toBase58())??null)};
    };
    const opts={sessionPath,snapshotOptions:f.args,transaction,control:sandboxControl};
    await callback({f,sessionPath,transaction,prepared,opts});
  });
}
test("sandbox submission persists intent before send, matches receipt and blocks further work",async()=>{
  await preparedFixture(async({f,sessionPath,transaction,prepared,opts})=>{
    let sends=0;
    f.connection.sendRawTransaction=async(bytes,options)=>{
      sends++;assert.deepEqual(bytes,Buffer.from(transaction.serialize()));
      assert.equal(options.maxRetries,0);assert.equal(options.skipPreflight,false);
      assert.equal((await inspectLiveMakerSession(sessionPath)).phase,"submitted");
      return prepared.signature;
    };
    const result=await submitSandboxMakerIntent(f.connection,opts);
    assert.equal(result.state,"submitted");assert.equal(sends,1);
    f.connection.getSignatureStatuses=async()=>({context:{slot:104},
      value:[{slot:103,confirmationStatus:"finalized",err:null}]});
    f.connection.getTransaction=async()=>({version:0,slot:103,meta:{err:null},
      transaction:{signatures:[prepared.signature],message:transaction.message}});
    const done=await finalizeSandboxMakerIntent(f.connection,opts);
    assert.equal(done.state,"finalized-success");assert.equal(done.canRelease,false);
    assert.equal((await inspectLiveMakerSession(sessionPath)).phase,"finalized");
    assert.equal(sends,1);
    await assert.rejects(submitSandboxMakerIntent(f.connection,opts),/duplicate submission/);
  });
});
test("ambiguous send is never retried, and null history cannot release",async()=>{
  await preparedFixture(async({f,sessionPath,opts})=>{
    let sends=0;
    f.connection.sendRawTransaction=async()=>{sends++;throw new Error("RPC disconnected");};
    assert.equal((await submitSandboxMakerIntent(f.connection,opts)).state,"unknown");
    assert.equal((await inspectLiveMakerSession(sessionPath)).phase,"submitted");
    const status=await finalizeSandboxMakerIntent(f.connection,opts);
    assert.equal(status.state,"unknown");assert.equal(status.canRelease,false);
    await assert.rejects(submitSandboxMakerIntent(f.connection,opts),/duplicate submission/);
    assert.equal(sends,1);
  });
});
test("sandbox refuses public RPC and requires explicit submit switch",async()=>{
  await preparedFixture(async({f,opts})=>{
    f.connection.rpcEndpoint="https://api.devnet.solana.com";
    await assert.rejects(submitSandboxMakerIntent(f.connection,opts),/Loopback RPC/);
    f.connection.rpcEndpoint="http://127.0.0.1:8899";
    await assert.rejects(submitSandboxMakerIntent(f.connection,{...opts,control}),/sandbox submission opt-in/);
  });
});
test("finalized status without matching full receipt never retires journal",async()=>{
  await preparedFixture(async({f,sessionPath,prepared,opts})=>{
    f.connection.sendRawTransaction=async()=>prepared.signature;
    await submitSandboxMakerIntent(f.connection,opts);
    f.connection.getSignatureStatuses=async()=>({context:{slot:104},
      value:[{slot:103,confirmationStatus:"finalized",err:null}]});
    f.connection.getTransaction=async()=>null;
    await assert.rejects(finalizeSandboxMakerIntent(f.connection,opts),/receipt does not match/);
    assert.equal((await inspectLiveMakerSession(sessionPath)).phase,"submitted");
  });
});
