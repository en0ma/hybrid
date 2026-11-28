// Local-only paired full-fill SBF comparison. No public endpoint or wallet keys.
import * as web3 from "@solana/web3.js";
import bs58 from "bs58";
import {readFile,mkdir,writeFile,rename} from "node:fs/promises";
import {resolve,join} from "node:path";
import {createHash} from "node:crypto";
import {execFileSync} from "node:child_process";
import {marketBytes,detachedBookPage,classicMintBytes,classicTokenBytes,surfpoolAccountPayload} from "./provision-surfpool.js";
import {PROGRAM_ID,TOKEN_PROGRAM_ID} from "./abi.js";
import {programChunks} from "./deploy-surfpool.js";
import {createSolanaSettlementAdapter} from "../../sdk/src/settlement-solana.mjs";
import {initMakerBalance,transferMakerCollateral,placeOrder} from "../../sdk/src/instructions.mjs";
import {authenticateSettlementCollateral,loadSettlementEpochs,decodeSettlementCustody} from "../../sdk/src/settlement-rpc.mjs";
import {prepareSettlement} from "../../sdk/src/settlement-plan.mjs";
import {buildSettlementClaim,recoverEpochProofs,reserveVaultFloor} from "../../sdk/src/settlement.mjs";
import {loadLiveMakerSnapshot} from "../../sdk/src/maker-live-snapshot.mjs";
import {auditExecutedReceipt,summarizeReceipts,validateFullFillPair,waitForFinalized,MANIFEST_PIN} from "./m84-metrics.mjs";
const {PublicKey,Keypair,Connection,SystemProgram,ComputeBudgetProgram,TransactionMessage,VersionedTransaction}=web3;
const H=new PublicKey(PROGRAM_ID),M=new PublicKey("MNFSTqtC93rEfYHB6hF82sKdZpUDFWkViLByLd1k1Ms");
const T=new PublicKey(TOKEN_PROGRAM_ID),T22=new PublicKey("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
const SYS=SystemProgram.programId,Q=1n<<64n,ORDER=100n,FUND=1000000000n,NIL=0xffffffff;
const key=()=>Keypair.generate(),bytes=k=>k.toBuffer(),hex=k=>bytes(k).toString("hex"),pk=k=>k.toBase58();
const meta=(pubkey,isWritable=false,isSigner=false)=>({pubkey,isWritable,isSigner});
const u32=n=>{const b=Buffer.alloc(4);b.writeUInt32LE(n);return b;};
const u64=n=>{const b=Buffer.alloc(8);b.writeBigUInt64LE(n);return b;};
const hash=b=>createHash("sha256").update(b).digest("hex");
const fail=(ok,msg)=>{if(!ok)throw Error(msg);};
const json=x=>JSON.stringify(x,(_,v)=>typeof v==="bigint"?String(v):v,2)+"\n";
 const ix=(programId,keys,data)=>({programId,keys,data});
const adapter=createSolanaSettlementAdapter(web3);
let rpc,url,genesis,checkpoint=async()=>{};
async function cheat(method,params){
 const response=await fetch(url,{method:"POST",signal:AbortSignal.timeout(10000),redirect:"error",headers:{"content-type":"application/json"},
  body:JSON.stringify({jsonrpc:"2.0",id:1,method,params})});
 const value=await response.json();fail(response.ok&&!value.error,"Local fixture "+method+" failed");
 return value.result;
}
async function seed(address,owner,data){
 fail(!(await rpc.getAccountInfo(address,"confirmed")),"Fixture address already exists");
 const rent=await rpc.getMinimumBalanceForRentExemption(data.length);
 await cheat("surfnet_setAccount",[pk(address),surfpoolAccountPayload({owner:pk(owner),data},rent)]);
 const a=await rpc.getAccountInfo(address,"confirmed");
 fail(a?.owner.equals(owner)&&a.data.equals(data)&&a.lamports===rent,"Seed readback mismatch");
}
async function deploy(program,path){
 fail(!(await rpc.getAccountInfo(program,"confirmed"))?.executable,"Expected fresh local deployment");
 const elf=await readFile(path);
 for(const chunk of programChunks(elf))await cheat("surfnet_writeProgram",[pk(program),chunk.data,chunk.offset]);
 fail((await rpc.getAccountInfo(program,"confirmed"))?.executable,"Deployment not executable");
 return {programId:pk(program),elfSha256:hash(elf),elfBytes:elf.length};
}
async function send(result,stage,market,program,instructions,payer,others=[]){
 const {blockhash}=await rpc.getLatestBlockhash("confirmed");
 const message=new TransactionMessage({payerKey:payer.publicKey,recentBlockhash:blockhash,
  instructions:[ComputeBudgetProgram.setComputeUnitLimit({units:400000}),...instructions]}).compileToV0Message();
 const tx=new VersionedTransaction(message);tx.sign([payer,...others]);
 const wire=Buffer.from(tx.serialize()),signature=bs58.encode(tx.signatures[0]);
 result.pendingAttempt={stage,signature,slot:await rpc.getSlot("confirmed"),status:"prepared"};
 await checkpoint();
 const sim=await rpc.simulateTransaction(tx,{sigVerify:true,replaceRecentBlockhash:false,commitment:"confirmed"});
 fail(sim.value.err===null,"Simulation failed "+stage+" "+JSON.stringify(sim.value.err));
 result.pendingAttempt.status="submission-attempted";
 await checkpoint();
 fail(await rpc.sendRawTransaction(wire,{maxRetries:0,skipPreflight:false})===signature,"Send signature mismatch");
 const receipt=await waitForFinalized(rpc,signature);
 const proof=auditExecutedReceipt(receipt,{signature,wire,programId:program,market,bs58});
 result.receipts.push({stage,...proof,simulationComputeUnits:sim.value.unitsConsumed});
 delete result.pendingAttempt;
 await checkpoint();
 console.log(JSON.stringify({stage,program:pk(program),market:pk(market),signature,slot:proof.slot,computeUnits:proof.computeUnits}));
 return proof;
}
async function externalAccounts(f){
 const accounts=[];
 for(const owner of [...f.makers,f.taker]){
  const base=key().publicKey,quote=key().publicKey;
  await seed(base,T,classicTokenBytes(f.baseMint,owner.publicKey,FUND));
  await seed(quote,T,classicTokenBytes(f.quoteMint,owner.publicKey,FUND));
  accounts.push({owner,base,quote});
 }
 return accounts;
}
async function amount(address,mint,owner,commitment="finalized"){
 const a=await rpc.getAccountInfo(address,commitment);
 fail(a?.owner.equals(T)&&a.data.length===165&&a.data.subarray(0,32).equals(bytes(mint))&&
  a.data.subarray(32,64).equals(bytes(owner))&&a.data[108]===1,"SPL account identity mismatch");
 return a.data.readBigUInt64LE(64);
}
async function walletState(accounts,f){
 return Promise.all(accounts.map(async a=>({base:await amount(a.base,f.baseMint,a.owner.publicKey),
  quote:await amount(a.quote,f.quoteMint,a.owner.publicKey)})));
}
async function footprint(keys){
 fail(new Set(keys.map(pk)).size===keys.length,"Aliased footprint");
 const r=await rpc.getMultipleAccountsInfoAndContext(keys,{commitment:"finalized"});
 fail(r.value.every(Boolean),"Missing allocated account");
 return {slot:r.context.slot,bytes:r.value.reduce((n,a)=>n+a.data.length,0),
  lamports:r.value.reduce((n,a)=>n+a.lamports,0),accounts:keys.map((k,i)=>({address:pk(k),owner:pk(r.value[i].owner),
   bytes:r.value[i].data.length,lamports:r.value[i].lamports,dataSha256:hash(r.value[i].data)}))};
}
async function verifyPayout(result,f,accounts,vaults,fee){
 const fill=ORDER*BigInt(f.makers.length),state=await walletState(accounts,f);
 const makers=state.slice(0,-1),taker=state.at(-1);
 fail(makers.every(a=>a.base===FUND-ORDER&&a.quote===FUND+ORDER)&&
  taker.base===FUND+fill&&taker.quote===FUND-fill-fee&&
  vaults.base===0n&&vaults.quote===fee,"Final external SPL conservation mismatch");
 const n=BigInt(accounts.length),sum=(field)=>state.reduce((a,x)=>a+x[field],0n);
 fail(sum("base")+vaults.base===n*FUND&&sum("quote")+vaults.quote===n*FUND,"Mint atom conservation mismatch");
 Object.assign(result,{verified:true,grossBaseAtoms:String(fill),grossQuoteAtoms:String(fill),
  payouts:makers.map(a=>String(a.quote-FUND)),protocolFeeQuoteAtoms:String(fee),
  inventoryUtilization:{numerator:String(fill),denominator:String(BigInt(f.makers.length)*FUND)},
  finalWallets:state,finalVaults:vaults,metrics:summarizeReceipts(result.receipts)});
}
async function hybrid(f,accounts,result){
 Object.assign(result,{programId:pk(H),receipts:[],verified:false,initialization:"seeded empty market/book/custody; signed balance/reserve initialization"});
 const market=key(),askOwner=key().publicKey,bidOwner=key().publicKey;
 const derive=(name,...rest)=>PublicKey.findProgramAddressSync([Buffer.from(name),bytes(market.publicKey),...rest],H);
 const [custody,bump]=derive("custody"),[reserve]=derive("lazy-reserve"),[authority]=derive("vault-authority");
 const [askPage]=derive("ask-page",u32(0)),[bidPage]=derive("bid-page",u32(0));
 const baseVault=key().publicKey,quoteVault=key().publicKey;
 const header=marketBytes(askOwner,bidOwner);header.writeUInt16LE(1,10);
 await seed(market.publicKey,H,header);
 for(const [address,sidecar] of [[askPage,false],[bidPage,false],[askOwner,true],[bidOwner,true]])
  await seed(address,H,detachedBookPage(0,sidecar));
 const c=Buffer.alloc(192);c.write("HYBCUST1");c[8]=1;c[9]=bump;c[10]=6;c[11]=6;
 [market.publicKey,f.baseMint,f.quoteMint,baseVault,quoteVault].forEach((k,i)=>bytes(k).copy(c,16+i*32));
 await seed(custody,H,c);
 await seed(baseVault,T,classicTokenBytes(f.baseMint,authority));
 await seed(quoteVault,T,classicTokenBytes(f.quoteMint,authority));
 const mk=market.publicKey,payer=f.payer;
 await send(result,"initialize",mk,H,[ix(H,[meta(mk,false,true),meta(custody),meta(reserve,true),meta(payer.publicKey,true,true),meta(SYS)],
  Buffer.concat([Buffer.from([39]),bytes(genesis)]))],payer,[market]);
 const balances=[];
 for(const [i,maker] of f.makers.entries()){
  const [makerBalance]=derive("maker-balance",bytes(maker.publicKey));balances.push(makerBalance);
  await send(result,"initialize",mk,H,[initMakerBalance(H,{market:mk,custody,makerBalance,maker:maker.publicKey,
   payer:maker.publicKey,systemProgram:SYS})],maker);
  await send(result,"deposit",mk,H,[transferMakerCollateral(H,{market:mk,custody,makerBalance,maker:maker.publicKey,
   source:accounts[i].base,vault:baseVault,mint:f.baseMint,tokenProgram:T},{direction:"deposit",asset:"base",amount:ORDER})],maker);
  await send(result,"place",mk,H,[placeOrder(H,{market:mk,page:askPage,ownerPage:askOwner,makerBalance,maker:maker.publicKey},
   {side:"ask",baseQty:ORDER,priceX64:Q,sqrtPriceX64:Q})],maker);
 }
 const taker=accounts.at(-1);
 const snap=()=>authenticateSettlementCollateral(rpc,{adapter,programId:H,market:mk,side:"ask",
  headOwnerPage:askOwner,genesisHex:hex(genesis),taker:f.taker.publicKey,takerBase:taker.base,takerQuote:taker.quote});
 // Preserve the SDK's slot policy. Capture actual RPC slot pairs on failure,
 // without replacing or inventing a commitment slot.
 async function makerSnapshot(maker,commitment,minContextSlot){
  const diagnostics={commitment,minContextSlot,batches:[]};
  const traced={
   getGenesisHash:()=>rpc.getGenesisHash(),
   async getMultipleAccountsInfoAndContext(keys,opts){
    const response=await rpc.getMultipleAccountsInfoAndContext(keys,opts);
    diagnostics.batches.push({requestedMinSlot:opts.minContextSlot,slot:response.context.slot,accounts:keys.length});
    return response;
   },
   async getSlot(level){const slot=await rpc.getSlot(level);diagnostics.observedSlot=slot;return slot;}
  };
  try{
   return await loadLiveMakerSnapshot(traced,{adapter,programId:H,market:mk,maker:maker.publicKey,
    askHeadOwner:askOwner,bidHeadOwner:bidOwner,expectedGenesisHash:pk(genesis),commitment,minContextSlot,maxLagSlots:32});
  }catch(error){
   result.snapshotDiagnostics=diagnostics;
   throw Error("Hybrid "+commitment+" maker snapshot: "+error.message);
  }
 }
 const before=await snap(),fill=ORDER*BigInt(f.makers.length),fee=fill/100n;
 fail(before.orders.length===f.makers.length&&before.orders.every((o,i)=>o.baseQty===ORDER&&o.priceX64===Q&&
  Buffer.from(o.owner).equals(bytes(f.makers[i].publicKey)))&&before.custodyState.totalBase===fill&&
  before.custodyState.totalQuote===0n,"Hybrid pre-trade depth/custody mismatch");
 for(const [i,maker] of f.makers.entries()){
  const state=await makerSnapshot(maker,"confirmed",result.receipts.at(-1).slot);
  fail(state.inventory.lockedBase===ORDER&&state.inventory.freeBase===0n&&state.ownedOrders.length===1,
   "Hybrid maker inventory mismatch");
 }
 result.preTradeFootprint=await footprint([mk,askPage,askOwner,bidPage,bidOwner,custody,reserve,baseVault,quoteVault,...balances]);
 const fresh=await snap(),prepared=prepareSettlement(H,fresh,{payer:f.taker.publicKey,systemProgram:SYS},
  {baseFill:fill,quoteLimit:fill+fee});
 await send(result,"execute",mk,H,[prepared.instruction],f.taker);
 const load=()=>loadSettlementEpochs(rpc,{adapter,programId:H,market:mk,genesisHex:hex(genesis),epochIds:[0n],commitment:"finalized"});
 const sealed=await load(),epoch=sealed.epochs[0];
 fail(epoch.totals.every((v,i)=>v===prepared.plan.ticks[0].totals[i]),"Hybrid sealed totals mismatch");
 result.sealedEpoch={root:epoch.claimRoot,totals:epoch.totals,claimedCount:epoch.claimedCount};
 for(const proof of recoverEpochProofs(epoch,prepared.plan.ticks[0].makers)){
  const i=f.makers.findIndex(m=>hex(m.publicKey)===proof.makerHex);fail(i>=0,"Unknown maker");
  const claim=buildSettlementClaim(H,{market:mk,epoch:epoch.address,makerBalance:balances[i],maker:f.makers[i].publicKey,
   custody,reserve,vault:quoteVault,destination:accounts[i].quote,mint:f.quoteMint,vaultAuthority:authority,tokenProgram:T},epoch,proof);
  await send(result,"payout",mk,H,[claim],f.makers[i]);
 }
 const final=await load(),r=final.reserve;
 fail(final.epochs[0].claimedCount===f.makers.length&&final.epochs[0].remaining.every(v=>v===0n)&&
  [r.makerBaseDebit,r.makerQuoteDebit,r.makerBaseCredit,r.makerQuoteCredit,r.baseFees].every(v=>v===0n)&&r.quoteFees===fee,
  "Hybrid unclaimed liabilities");
 const cFinal=decodeSettlementCustody((await rpc.getAccountInfo(custody,"finalized")).data);
 const floor=reserveVaultFloor(r,cFinal.totalBase,cFinal.totalQuote);
 fail(cFinal.totalBase===0n&&cFinal.totalQuote===0n&&floor.base===0n&&floor.quote===fee,"Hybrid final custody mismatch");
 for(const maker of f.makers){
  const state=await makerSnapshot(maker,"finalized",result.receipts.at(-1).slot);
  fail(state.ownedOrders.length===0&&["freeBase","lockedBase","freeQuote","lockedQuote"].every(k=>state.inventory[k]===0n),
   "Hybrid residual orders or collateral");
 }
 result.footprint=await footprint([mk,askPage,askOwner,bidPage,bidOwner,custody,reserve,epoch.address,baseVault,quoteVault,...balances]);
 await verifyPayout(result,f,accounts,{base:await amount(baseVault,f.baseMint,authority),quote:await amount(quoteVault,f.quoteMint,authority)},fee);
 return result;
}
// Narrow exact decoder for the pinned Manifest v3.1.0 market, classic SPL
// local Limit orders only. Every reachable node validated for bounds/parent/alias.
function manifestBook(data,f,baseVault,quoteVault){
 fail(data.length>=256&&data.readBigUInt64LE(0)===4859840929024028656n&&data[8]===0&&
  data[9]===6&&data[10]===6&&[f.baseMint,f.quoteMint,baseVault,quoteVault].every((k,i)=>data.subarray(16+32*i,48+32*i).equals(bytes(k)))&&
  data.readUInt32LE(152)===data.length-256,"Manifest market layout/domain mismatch");
 const used=new Set(),dynamic=data.subarray(256);
 function nodes(root){
  const stack=[[root,NIL]],out=[];
  while(stack.length){const [offset,parent]=stack.pop();if(offset===NIL)continue;
   fail(offset%80===0&&offset+80<=dynamic.length&&!used.has(offset),"Manifest tree offset/alias mismatch");
   used.add(offset);fail(dynamic.readUInt32LE(offset+8)===parent,"Manifest tree parent mismatch");
   out.push({offset,body:dynamic.subarray(offset+16,offset+80)});
   stack.push([dynamic.readUInt32LE(offset),offset],[dynamic.readUInt32LE(offset+4),offset]);
  }return out;
 }
 const seats=new Map(nodes(data.readUInt32LE(172)).map(({offset,body})=>[offset,{owner:new PublicKey(body.subarray(0,32)),
  base:body.readBigUInt64LE(32),quote:body.readBigUInt64LE(40)}]));
 const bids=nodes(data.readUInt32LE(156)),asks=nodes(data.readUInt32LE(164)).map(({body})=>{
  const seat=seats.get(body.readUInt32LE(32));fail(seat&&body[40]===0&&body[41]===0&&body.readUInt32LE(36)===0,"Unsupported Manifest resting order");
  return {owner:seat.owner,base:body.readBigUInt64LE(16),price:body.readBigUInt64LE(0)+(body.readBigUInt64LE(8)<<64n)};
 });
 fail(bids.length===0,"Unexpected Manifest bids");
 return {seats:[...seats.values()],asks};
}
async function manifest(f,accounts,result){
 Object.assign(result,{programId:pk(M),receipts:[],verified:false,initialization:"signed core market and seat initialization"});
 const market=key(),mk=market.publicKey;
 const vault=m=>PublicKey.findProgramAddressSync([Buffer.from("vault"),bytes(mk),bytes(m)],M)[0];
 const baseVault=vault(f.baseMint),quoteVault=vault(f.quoteMint);
 const rent=await rpc.getMinimumBalanceForRentExemption(256);
 await send(result,"initialize",mk,M,[SystemProgram.createAccount({fromPubkey:f.payer.publicKey,newAccountPubkey:mk,
  space:256,lamports:rent,programId:M}),ix(M,[meta(f.payer.publicKey,true,true),meta(mk,true),meta(SYS),
  meta(f.baseMint),meta(f.quoteMint),meta(baseVault,true),meta(quoteVault,true),meta(T),meta(T22)],Buffer.from([0]))],f.payer,[market]);
 const balances=(op,maker,tokenAccount,vault,mint)=>ix(M,[meta(maker.publicKey,true,true),meta(mk,true),
  meta(tokenAccount,true),meta(vault,true),meta(T),meta(mint)],Buffer.concat([Buffer.from([op]),u64(ORDER),Buffer.from([0])]));
 for(const [i,maker] of f.makers.entries()){
  const prefix=[meta(maker.publicKey,true,true),meta(mk,true),meta(SYS)];
  await send(result,"initialize",mk,M,[ix(M,prefix,Buffer.from([1]))],maker);
  await send(result,"deposit",mk,M,[balances(2,maker,accounts[i].base,baseVault,f.baseMint)],maker);
  const data=Buffer.concat([Buffer.from([6,0]),u32(0),u32(1),u64(ORDER),u32(1),Buffer.from([0,0]),u32(0),Buffer.from([0])]);
  await send(result,"place",mk,M,[ix(M,prefix,data)],maker);
 }
 const readBook=async()=>{const a=await rpc.getAccountInfo(mk,"finalized");fail(a?.owner.equals(M),"Manifest market owner mismatch");
  return manifestBook(a.data,f,baseVault,quoteVault);};
 const book=await readBook(),fill=ORDER*BigInt(f.makers.length);
 fail(book.asks.length===f.makers.length&&book.seats.length===f.makers.length&&
  f.makers.every(m=>book.asks.filter(o=>o.owner.equals(m.publicKey)&&o.base===ORDER&&o.price===10n**18n).length===1)&&
  book.seats.every(s=>s.base===0n&&s.quote===0n)&&await amount(baseVault,f.baseMint,baseVault)===fill&&
  await amount(quoteVault,f.quoteMint,quoteVault)===0n,"Manifest pre-trade collateral mismatch");
 result.preTradeFootprint=await footprint([mk,baseVault,quoteVault]);
 const taker=accounts.at(-1);
 await send(result,"execute",mk,M,[ix(M,[meta(f.taker.publicKey,true,true),meta(mk,true),meta(SYS),
  meta(taker.base,true),meta(taker.quote,true),meta(baseVault,true),meta(quoteVault,true),meta(T)],
  Buffer.concat([Buffer.from([4]),u64(fill),u64(fill),Buffer.from([0,0])]))],f.taker);
 const post=await readBook();
 fail(post.asks.length===0&&f.makers.every(m=>post.seats.filter(s=>s.owner.equals(m.publicKey)&&s.base===0n&&s.quote===ORDER).length===1),
  "Manifest executed fill/seat mismatch");
 for(const [i,maker] of f.makers.entries())
  await send(result,"payout",mk,M,[balances(3,maker,accounts[i].quote,quoteVault,f.quoteMint)],maker);
 const final=await readBook();fail(final.asks.length===0&&final.seats.every(s=>s.base===0n&&s.quote===0n),"Manifest residual liability");
 result.footprint=await footprint([mk,baseVault,quoteVault]);
 await verifyPayout(result,f,accounts,{base:await amount(baseVault,f.baseMint,baseVault),quote:await amount(quoteVault,f.quoteMint,quoteVault)},0n);
 return result;
}
async function main(){
 const endpoint=new URL(process.env.SOLANA_RPC_URL||"http://127.0.0.1:8899");
 fail(endpoint.protocol==="http:"&&endpoint.hostname==="127.0.0.1"&&endpoint.port&&endpoint.pathname==="/"&&
  !endpoint.username&&!endpoint.password&&!endpoint.search&&!endpoint.hash&&process.env.M84_OFFLINE_FIXTURE==="1",
  "Explicit isolated offline Surfpool required");
 url=endpoint.href;
 // web3.js must never wait indefinitely for HTTP or retry HTTP 429.
 const boundedFetch=(input,init)=>fetch(input,{...init,signal:AbortSignal.timeout(10000),redirect:"error"});
 rpc=new Connection(url,{commitment:"confirmed",disableRetryOnRateLimit:true,fetch:boundedFetch});
 genesis=new PublicKey(await rpc.getGenesisHash());
 const rev=path=>execFileSync("git",["-C",path,"rev-parse","HEAD"],{encoding:"utf8"}).trim();
 const hybridCommit=rev(".."),manifestCommit=rev(process.env.MANIFEST_REPO);
 fail(hybridCommit===process.env.HYBRID_BENCHMARK_COMMIT&&manifestCommit===MANIFEST_PIN,"Unpinned checkout");
 const dir=resolve(process.env.M84_EVIDENCE_DIR||".m84-evidence");await mkdir(dir,{recursive:true,mode:0o700});
 const selected=process.env.M84_CASE_MAKERS===undefined?[1,8]:[Number(process.env.M84_CASE_MAKERS)];
 fail(selected.every(m=>[1,8].includes(m)),"Unknown full-fill case selection");
 const report={schemaVersion:1,kind:"preliminary-paired-full-fill",network:"offline-surfpool",selectedMakers:selected,
  hybridCommit,manifestCommit,genesis:pk(genesis),toolchain:{node:process.version,
   solana:execFileSync("solana",["--version"],{encoding:"utf8"}).trim(),
   rust:execFileSync("rustc",["--version"],{encoding:"utf8"}).trim(),
   surfpool:execFileSync("surfpool",["--version"],{encoding:"utf8"}).trim(),
   surfpoolBinarySha256:process.env.M84_SURFPOOL_SHA256||null,
   runner:process.env.GITHUB_RUN_ID||"local",job:process.env.GITHUB_JOB||"local",
   caseMakers:selected.join(","),arch:process.arch},cases:[],complete:false,
  unmeasured:["two-market-inventory","partial-fill-cancel-replace-retry","adverse-price-collateral-stress"]};
 const evidencePath=join(dir,"m84-empirical.json");
 checkpoint=async()=>{
  const next=evidencePath+".next";
  await writeFile(next,json(report),{mode:0o600});
  await rename(next,evidencePath);
 };
 await checkpoint();
 try{
  report.hybridBuild=await deploy(H,process.env.HYBRID_SBF_PATH);
  report.manifestBuild=await deploy(M,process.env.MANIFEST_SBF_PATH);
  await checkpoint();
  for(const makers of selected){
   const f={makers:Array.from({length:makers},key),taker:key(),payer:key(),baseMint:key().publicKey,quoteMint:key().publicKey};
   // Distinct newly generated actor accounts: issue funding once per actor,
   // then wait for all finalized balances concurrently. No trade is parallelized.
   const funding=await Promise.all([f.payer,f.taker,...f.makers].map(async actor=>{
    const signature=await rpc.requestAirdrop(actor.publicKey,2000000000);
    return {actor,signature};
   }));
   await Promise.all(funding.map(async({actor,signature})=>{
    await waitForFinalized(rpc,signature,{receiptRequired:false});
    fail(await rpc.getBalance(actor.publicKey,"finalized")>=2000000000,"Airdrop funding unavailable in finalized bank");
   }));
   const supply=2n*BigInt(makers+1)*FUND;
   for(const mint of [f.baseMint,f.quoteMint]){
    const data=classicMintBytes(f.payer.publicKey);data.writeBigUInt64LE(supply,36);await seed(mint,T,data);
   }
   const h=await externalAccounts(f),m=await externalAccounts(f);
   const row={name:makers===1?"one-maker-one-level-full-fill":"eight-maker-fanout-full-fill",makers,
    baseMint:pk(f.baseMint),quoteMint:pk(f.quoteMint),makerOwners:f.makers.map(x=>pk(x.publicKey)),
    taker:pk(f.taker.publicKey),orderBaseAtoms:String(ORDER),initialMakerBaseAtoms:String(FUND),initialMakerQuoteAtoms:String(FUND),
    initialTakerBaseAtoms:String(FUND),initialTakerQuoteAtoms:String(FUND),baseDecimals:6,quoteDecimals:6};
   report.cases.push(row);
   row.hybrid={};await hybrid(f,h,row.hybrid);
   row.manifest={};await manifest(f,m,row.manifest);
   validateFullFillPair(row);
   await checkpoint();
  }
  report.complete=true;
 }catch(error){report.error=error.message;throw error;}
 finally{await checkpoint();}
}
main().catch(e=>{console.error(e.message);process.exitCode=1;});
