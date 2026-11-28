// Actual SBF-backed SDK settlement on isolated loopback Surfpool.
import * as web3 from "@solana/web3.js";
import bs58 from "bs58";
import {writeFile,mkdir,readFile,unlink,stat} from "node:fs/promises";
import {execFile} from "node:child_process";
import {promisify} from "node:util";
import {fileURLToPath} from "node:url";
import {resolve,join} from "node:path";
import {clusterConfig,assertCluster} from "./cluster.js";
import {PROGRAM_ID,TOKEN_PROGRAM_ID} from "./abi.js";
import {marketBytes,detachedBookPage,classicMintBytes,classicTokenBytes,surfpoolAccountPayload} from "./provision-surfpool.js";
import {createSolanaSettlementAdapter} from "../../sdk/src/settlement-solana.mjs";
import {authenticateSettlementCollateral,loadSettlementEpochs,decodeSettlementCustody} from "../../sdk/src/settlement-rpc.mjs";
import {prepareSettlement} from "../../sdk/src/settlement-plan.mjs";
import {initPassivePool,openPassivePosition,closePassivePosition,transferMakerCollateral,swap} from "../../sdk/src/instructions.mjs";
import {canonicalSqrtPriceX64} from "../../sdk/src/quote-orders.mjs";
import {loadLiveMakerSnapshot} from "../../sdk/src/maker-live-snapshot.mjs";
// Maker mutations below deliberately traverse separate CLI processes.
import {buildSettlementClaim,recoverEpochProofs,decodeLazyReserve,reserveVaultFloor} from "../../sdk/src/settlement.mjs";
import {decodeMakerBalance} from "../../sdk/src/maker-balance.mjs";
import {simulateSettlement,journalSettlementClaim,loadSettlementLookupTable,recoverSettlementClaim} from "../../sdk/src/settlement-lifecycle.mjs";

const {Connection,PublicKey,Keypair,SystemProgram,TransactionInstruction,ComputeBudgetProgram}=web3;
const adapter=createSolanaSettlementAdapter(web3),program=new PublicKey(PROGRAM_ID),token=new PublicKey(TOKEN_PROGRAM_ID);
const Q=1n<<64n,key=()=>Keypair.generate(),bytes=k=>Buffer.from(k.toBytes()),hex=k=>bytes(k).toString("hex");
const u32=n=>{const b=Buffer.alloc(4);b.writeUInt32LE(n);return b;};
const u64=n=>{const b=Buffer.alloc(8);b.writeBigUInt64LE(BigInt(n));return b;};
const u128=(b,i,n)=>{b.writeBigUInt64LE(n&(Q-1n),i);b.writeBigUInt64LE(n>>64n,i+8);};
const meta=(pubkey,isWritable=false,isSigner=false)=>({pubkey,isWritable,isSigner});
const fail=(condition,message)=>{if(!condition)throw Error(message);};
const json=value=>JSON.stringify(value,(_,v)=>typeof v==="bigint"?v.toString():v,2)+"\n";
async function seed(rpc,url,address,owner,data){
 const rent=await rpc.getMinimumBalanceForRentExemption(data.length);
 const response=await fetch(url,{method:"POST",headers:{"content-type":"application/json"},
  body:JSON.stringify({jsonrpc:"2.0",id:1,method:"surfnet_setAccount",params:[address.toBase58(),
    surfpoolAccountPayload({owner:owner.toBase58(),data},Math.max(rent,10000000))]})});
 const result=await response.json();fail(response.ok&&!result.error,"Fixture seed failed");
 const observed=await rpc.getAccountInfo(address,"confirmed");
 fail(observed?.owner.equals(owner)&&observed.data.equals(data),"Fixture seed readback mismatch");
}
async function signed(rpc,ix,payer,signers,report,lookupTables=[],prepared=null,onSimulated=async()=>{}){
 const {blockhash,lastValidBlockHeight}=await rpc.getLatestBlockhash("confirmed");
 const tx=adapter.compileUnsignedV0({payer:payer.publicKey,recentBlockhash:blockhash,instruction:ix,lookupTables,
  computeBudgetInstructions:[ComputeBudgetProgram.setComputeUnitLimit({units:400000})]});
 tx.sign([payer,...signers]);
 const slot=await rpc.getSlot("confirmed");
 const plan=prepared??{mode:"unsigned-only",slot,instruction:ix};
 const evidence=await simulateSettlement(rpc,{adapter,transaction:tx,prepared:plan,lookupTables,currentSlot:slot,maxLagSlots:32});
 const signature=bs58.encode(tx.signatures[0]);
 await onSimulated({transaction:tx,simulationEvidence:evidence,signature,lastValidBlockHeight});
 const entry={opcode:ix.data[0],signature,bytes:tx.serialize().length,units:evidence.unitsConsumed,status:"prepared"};
 report.receipts.push(entry);
 const submitted=await rpc.sendRawTransaction(tx.serialize(),{skipPreflight:false,maxRetries:0});
 fail(submitted===signature,"RPC signature mismatch");
 const confirmed=await rpc.confirmTransaction({signature,blockhash,lastValidBlockHeight},"confirmed");
 fail(!confirmed.value.err,"Transaction failed");
 let receipt;
 for(let i=0;i<20;i++){receipt=await rpc.getTransaction(signature,{commitment:"confirmed",maxSupportedTransactionVersion:0});
  if(receipt)break;await new Promise(r=>setTimeout(r,250));}
 fail(receipt?.meta?.err===null,"Missing successful independent receipt");
 fail(receipt.transaction.signatures[0]===signature,"Receipt signature mismatch");
 Object.assign(entry,{slot:receipt.slot,status:"confirmed",receiptUnits:receipt.meta.computeUnitsConsumed});
 return entry;
}
async function setup(rpc,url,side,multi,genesis,shared=false){
 const market=key(),payer=key(),taker=key(),head=key().publicKey,other=key().publicKey;
 const baseMint=key().publicKey,quoteMint=key().publicKey,baseVault=key().publicKey,quoteVault=key().publicKey;
 const takerBase=key().publicKey,takerQuote=key().publicKey;
 const count=multi?3:2,makers=Array.from({length:count},key),destinations=makers.map(()=>key().publicKey);
 const derive=(name,...seeds)=>PublicKey.findProgramAddressSync([Buffer.from(name),bytes(market.publicKey),...seeds],program);
 const [custody,bump]=derive("custody"),[reserve]=derive("lazy-reserve"),[authority]=derive("vault-authority");
 const rows=makers.map((maker,i)=>({maker,qty:i===1?200n:100n,price:i<2?Q:side==="ask"?4n*Q:Q/2n}));
 const bid=side==="bid",locked=rows.reduce((n,r)=>n+(bid?(r.qty*r.price+Q-1n)/Q:r.qty),0n);
 const h=marketBytes(bid?other:head,bid?head:other);
 h.writeUInt16LE(1,10);h.writeUInt32LE(count,bid?20:12);h.writeBigUInt64LE(BigInt(count+1),32);
 if(shared){h.writeUInt32LE(2,20);h.writeBigUInt64LE(5n,32);}
 await seed(rpc,url,market.publicKey,program,h);
 for(let i=0;i<(multi?2:1);i++){
  const pageRows=rows.slice(i?2:0,i?3:2),page=detachedBookPage(i),owners=detachedBookPage(i,true);
  for(const data of [page,owners]){data.writeUInt16LE(pageRows.length);if(multi){data.writeUInt32LE(i?0:0xffffffff,6);data.writeUInt32LE(i?0xffffffff:1,10);}}
  pageRows.forEach((r,j)=>{const o=16+j*48;u128(page,o,r.price);u128(page,o+16,canonicalSqrtPriceX64(r.price));
   page.writeBigUInt64LE(r.qty,o+32);page.writeBigUInt64LE(BigInt(i*2+j+1),o+40);
   bytes(r.maker.publicKey).copy(owners,16+32*j);});
  await seed(rpc,url,derive(side+"-page",u32(i))[0],program,page);
  await seed(rpc,url,i?derive(side+"-owner-page",u32(i))[0]:head,program,owners);
 }
 const c=Buffer.alloc(192);c.write("HYBCUST1");c[8]=1;c[9]=bump;c[10]=6;c[11]=6;
 [market.publicKey,baseMint,quoteMint,baseVault,quoteVault].forEach((k,i)=>bytes(k).copy(c,16+32*i));
 c.writeBigUInt64LE(bid?0n:locked,176);c.writeBigUInt64LE(shared?300n:bid?locked:0n,184);
 await seed(rpc,url,custody,program,c);
 for(const mint of [baseMint,quoteMint])await seed(rpc,url,mint,token,classicMintBytes(payer.publicKey));
 for(const [address,mint,owner,amount] of [[baseVault,baseMint,authority,bid?0n:locked],
  [quoteVault,quoteMint,authority,shared?300n:bid?locked:0n],[takerBase,baseMint,taker.publicKey,shared?1000n:bid?1000n:0n],
  [takerQuote,quoteMint,taker.publicKey,bid?0n:1000n]])await seed(rpc,url,address,token,classicTokenBytes(mint,owner,amount));
 const balances=[];
 const bidMakers=shared?Array.from({length:2},key):[],bidBalances=[],bidDestinations=[];
 if(shared){
  const page=detachedBookPage(0),owners=detachedBookPage(0,true);
  page.writeUInt16LE(2);owners.writeUInt16LE(2);
  for(const [i,maker] of bidMakers.entries()){
   const qty=i?200n:100n,o=16+i*48;
   u128(page,o,Q);u128(page,o+16,canonicalSqrtPriceX64(Q));page.writeBigUInt64LE(qty,o+32);page.writeBigUInt64LE(BigInt(i+3),o+40);
   bytes(maker.publicKey).copy(owners,16+32*i);
   const [balance,bump]=derive("maker-balance",bytes(maker.publicKey));bidBalances.push(balance);
   const b=Buffer.alloc(112);b.write("HYBBAL01");b[8]=1;b[9]=bump;bytes(market.publicKey).copy(b,16);bytes(maker.publicKey).copy(b,48);b.writeBigUInt64LE(qty,104);
   await seed(rpc,url,balance,program,b);const dest=key().publicKey;bidDestinations.push(dest);
   await seed(rpc,url,dest,token,classicTokenBytes(baseMint,maker.publicKey));
  }
  await seed(rpc,url,derive("bid-page",u32(0))[0],program,page);await seed(rpc,url,other,program,owners);
 }
 for(const [i,r] of rows.entries()){
  const [balance,bump]=derive("maker-balance",bytes(r.maker.publicKey));balances.push(balance);
  const b=Buffer.alloc(112);b.write("HYBBAL01");b[8]=1;b[9]=bump;
  bytes(market.publicKey).copy(b,16);bytes(r.maker.publicKey).copy(b,48);
  b.writeBigUInt64LE(bid?(r.qty*r.price+Q-1n)/Q:r.qty,bid?104:88);
  await seed(rpc,url,balance,program,b);
  await seed(rpc,url,destinations[i],token,classicTokenBytes(bid?baseMint:quoteMint,r.maker.publicKey));
 }
 for(const actor of [payer,taker,market,...makers,...bidMakers]){
  const sig=await rpc.requestAirdrop(actor.publicKey,2000000000);await rpc.confirmTransaction(sig,"confirmed");
 }
 return {market,payer,taker,head,baseMint,quoteMint,baseVault,quoteVault,takerBase,takerQuote,
  custody,reserve,authority,makers,balances,destinations,rows,genesis,side,multi,bidHead:other,bidMakers,bidBalances,bidDestinations};
}
async function runCase(rpc,url,side,multi,genesis,dir,report){
 const f=await setup(rpc,url,side,multi,genesis),bid=side==="bid";
 const result={side,multi,market:f.market.publicKey.toBase58(),receipts:[],epochs:[],complete:false};
 report.cases.push(result);
 const init={programId:program,keys:[meta(f.market.publicKey,false,true),meta(f.custody),meta(f.reserve,true),
  meta(f.payer.publicKey,true,true),meta(SystemProgram.programId)],data:Buffer.concat([Buffer.from([39]),bytes(genesis)])};
 await signed(rpc,init,f.payer,[f.market],result);
 const snapshot=await authenticateSettlementCollateral(rpc,{adapter,programId:program,market:f.market.publicKey,
  side,headOwnerPage:f.head,genesisHex:hex(genesis),taker:f.taker.publicKey,takerBase:f.takerBase,takerQuote:f.takerQuote});
 const prepared=prepareSettlement(program,snapshot,{payer:f.payer.publicKey,systemProgram:SystemProgram.programId},
  {baseFill:multi?350n:150n,quoteLimit:multi?(bid?325n:505n):(bid?150n:151n)});
 await writeFile(join(dir,side+"-"+multi+"-roster.json"),json(prepared.plan),{mode:0o600,flag:"wx"});
 let lookupTables=[];
 if(multi){
  const recentSlot=await rpc.getSlot("confirmed");
  const created=adapter.buildLookupCreate({authority:f.payer.publicKey,payer:f.payer.publicKey,recentSlot});
  async function lookupTransaction(ix){
   const {blockhash,lastValidBlockHeight}=await rpc.getLatestBlockhash("confirmed");
   const tx=new web3.Transaction({feePayer:f.payer.publicKey,recentBlockhash:blockhash}).add(ix);
   tx.sign(f.payer);
   const signature=await rpc.sendRawTransaction(tx.serialize(),{skipPreflight:false,maxRetries:0});
   const confirmation=await rpc.confirmTransaction({signature,blockhash,lastValidBlockHeight},"confirmed");
   fail(!confirmation.value.err,"ALT transaction failed");
   return signature;
  }
  const createSignature=await lookupTransaction(created.instruction);
  const addresses=prepared.instruction.keys.filter(k=>!k.isSigner).map(k=>k.pubkey);
  const extensionSignatures=[];
  for(let i=0;i<addresses.length;i+=20)extensionSignatures.push(await lookupTransaction(
    adapter.buildLookupExtend({address:created.address,authority:f.payer.publicKey,payer:f.payer.publicKey,addresses:addresses.slice(i,i+20)})));
  let table;
  for(let i=0;i<40;i++){
   try{table=await loadSettlementLookupTable(rpc,{adapter,address:created.address,authority:f.payer.publicKey,
    requiredAddresses:addresses,minContextSlot:snapshot.slot});break;}
   catch(e){if(i===39)throw e;await new Promise(r=>setTimeout(r,250));}
  }
  lookupTables=[table];
  result.lookup={address:created.address.toBase58(),createSignature,extensionSignatures,activationSlot:table.slot};
 }
 // Refresh discovery after lookup warmup; reject changed plans.
 const fresh=await authenticateSettlementCollateral(rpc,{adapter,programId:program,market:f.market.publicKey,
  side,headOwnerPage:f.head,genesisHex:hex(genesis),taker:f.taker.publicKey,takerBase:f.takerBase,takerQuote:f.takerQuote});
 const execution=prepareSettlement(program,fresh,{payer:f.payer.publicKey,systemProgram:SystemProgram.programId},
  {baseFill:prepared.plan.baseFill,quoteLimit:prepared.plan.quoteLimit});
 fail(json(execution.plan)===json(prepared.plan),"Plan changed after ALT setup");
 if(multi)lookupTables=[await loadSettlementLookupTable(rpc,{adapter,address:lookupTables[0].address,
  authority:f.payer.publicKey,requiredAddresses:lookupTables[0].requiredAddresses,minContextSlot:fresh.slot})];
 await signed(rpc,execution.instruction,f.payer,[f.taker],result,lookupTables,execution);
 const ids=prepared.plan.ticks.map((_,i)=>BigInt(i));
 const load=()=>loadSettlementEpochs(rpc,{adapter,programId:program,market:f.market.publicKey,
  genesisHex:hex(genesis),epochIds:ids,commitment:"confirmed"});
 const sealed=await load();
 for(const [i,epoch] of sealed.epochs.entries()){
  const tick=prepared.plan.ticks[i];
  fail(epoch.totals.every((v,j)=>v===tick.totals[j]),"SDK quote differs from sealed totals");
  const proofs=recoverEpochProofs(epoch,tick.makers);
  result.epochs.push({id:String(epoch.epochId),commitment:epoch.commitment,root:epoch.claimRoot,totals:epoch.totals});
  for(const proof of [...proofs].reverse()){
   const j=f.makers.findIndex(m=>hex(m.publicKey)===proof.makerHex);
   fail(j>=0,"Maker roster mismatch");
   const accounts={market:f.market.publicKey,epoch:epoch.address,makerBalance:f.balances[j],
    maker:f.makers[j].publicKey,custody:f.custody,reserve:f.reserve,vault:bid?f.baseVault:f.quoteVault,
    destination:f.destinations[j],mint:bid?f.baseMint:f.quoteMint,vaultAuthority:f.authority,tokenProgram:token};
   const ix=buildSettlementClaim(program,accounts,epoch,proof);
   const path=join(dir,side+"-"+multi+"-claim-"+epoch.epochId+"-"+proof.seat+".json");
   await signed(rpc,ix,f.payer,[f.makers[j]],result,[],null,async evidence=>{
    await journalSettlementClaim(path,{adapter,programId:program,accounts,epoch,proof,...evidence,
      genesisHash:genesis.toBase58()});
   });
   // Finalized receipt and bitmap must agree before recovery is considered complete.
   const sig=result.receipts.at(-1).signature;
   let recovery;
   for(let attempt=0;attempt<80;attempt++){
    const statuses=await rpc.getSignatureStatuses([sig],{searchTransactionHistory:true});
    if(statuses.value[0]?.confirmationStatus==="finalized"){
     try{
      recovery=await recoverSettlementClaim(rpc,{adapter,path,programId:program,market:f.market.publicKey});
      break;
     }catch(error){
      // Signature status context can lead the finalized account bank.
      // Retry only this transient RPC error; never lower the required slot.
      if(!/minimum context slot has not been reached/i.test(error.message))throw error;
     }
    }
    await new Promise(r=>setTimeout(r,500));
   }
   fail(recovery?.claimed&&recovery.status.state==="finalized-success"&&!recovery.canResubmit,"Claim recovery failed");
   result.receipts.at(-1).recovered=true;
   const freshEpoch=(await load()).epochs[i];
   let rejected=false;
   try{buildSettlementClaim(program,accounts,freshEpoch,proof);}catch{rejected=true;}
   fail(rejected,"SDK accepted claimed seat");
   // Force a fresh signed transaction rather than replaying a cached signature.
   const latest=await rpc.getLatestBlockhash("confirmed");
   const replay=adapter.compileUnsignedV0({payer:f.payer.publicKey,recentBlockhash:latest.blockhash,instruction:ix,
     computeBudgetInstructions:[ComputeBudgetProgram.setComputeUnitLimit({units:399999})]});
   replay.sign([f.payer,f.makers[j]]);
   const before=await rpc.getMultipleAccountsInfo([epoch.address,f.reserve,f.balances[j],accounts.vault,accounts.destination],"confirmed");
   const rejectedReplay=await rpc.simulateTransaction(replay,{sigVerify:true,replaceRecentBlockhash:false,commitment:"confirmed"});
   fail(rejectedReplay.value.err!==null,"On-chain replay simulation succeeded");
   const after=await rpc.getMultipleAccountsInfo([epoch.address,f.reserve,f.balances[j],accounts.vault,accounts.destination],"confirmed");
   fail(before.every((a,k)=>a.data.equals(after[k].data)),"Replay changed account state");
   result.receipts.at(-1).replayRejected=true;
  }
 }
 const final=await load();
 fail(final.epochs.every(e=>e.claimedCount===e.makerCount&&e.remaining.every(n=>n===0n)),"Incomplete epoch claims");
 const r=final.reserve;
 fail([r.makerBaseCredit,r.makerQuoteCredit,r.makerBaseDebit,r.makerQuoteDebit].every(n=>n===0n),"Pending liabilities remain");
 const [custody,base,quote,...makerInfos]=await rpc.getMultipleAccountsInfo([f.custody,f.baseVault,f.quoteVault,...f.balances],"confirmed");
 const c=decodeSettlementCustody(custody.data),floor=reserveVaultFloor(r,c.totalBase,c.totalQuote);
 fail(base.data.readBigUInt64LE(64)===floor.base&&quote.data.readBigUInt64LE(64)===floor.quote,"Vault reconciliation failed");
 const balances=makerInfos.map((a,i)=>decodeMakerBalance(a.data,{market:f.market.publicKey,maker:f.makers[i].publicKey}));
 fail(balances.reduce((n,b)=>n+b.lockedBase,0n)===c.totalBase&&balances.reduce((n,b)=>n+b.lockedQuote,0n)===c.totalQuote,"Maker lock reconciliation failed");
 Object.assign(result,{complete:true,finalReserve:r,finalCustody:c,finalFloor:floor,balances});
}

async function runShared(rpc,url,genesis,dir,report){
 const f=await setup(rpc,url,"ask",false,genesis,true);
 const result={market:f.market.publicKey.toBase58(),receipts:[],epochs:[],complete:false};
 report.shared=result;
 const init={programId:program,keys:[meta(f.market.publicKey,false,true),meta(f.custody),meta(f.reserve,true),
  meta(f.payer.publicKey,true,true),meta(SystemProgram.programId)],data:Buffer.concat([Buffer.from([39]),bytes(genesis)])};
 await signed(rpc,init,f.payer,[f.market],result);
 const market=f.market.publicKey,owner=f.taker.publicKey;
 const [pool]=PublicKey.findProgramAddressSync([Buffer.from("passive-pool"),bytes(market)],program);
 const nonce=77n;
 const [position]=PublicKey.findProgramAddressSync([Buffer.from("passive-position"),bytes(market),bytes(owner),u64(nonce)],program);
 const aggregateGuard={pool,reserve:f.reserve,baseVault:f.baseVault,quoteVault:f.quoteVault};
 const guarded={market,marketFlags:3,aggregateGuard,custody:f.custody,pool,position,owner,
  baseVault:f.baseVault,quoteVault:f.quoteVault,baseMint:f.baseMint,quoteMint:f.quoteMint,
  baseSource:f.takerBase,quoteSource:f.takerQuote,baseDestination:f.takerBase,quoteDestination:f.takerQuote,
  vaultAuthority:f.authority,tokenProgram:token,systemProgram:SystemProgram.programId};
 await signed(rpc,initPassivePool(program,{market,custody:f.custody,pool,
  payer:f.payer.publicKey,systemProgram:SystemProgram.programId}),f.payer,[],result);
 await signed(rpc,openPassivePosition(program,guarded,{nonce,lowerSqrtPriceX64:Q/2n,
  upperSqrtPriceX64:Q*2n,liquidity:100n,baseDeposit:100n,quoteDeposit:100n}),f.payer,[f.taker],result);
 const lpBefore=await rpc.getAccountInfo(position,"confirmed");
 fail(lpBefore?.data.length===176,"LP position layout mismatch");
 // Exercise the maker collateral descriptors on the now-guarded market.
 const maker=f.makers[0],source=key().publicKey;
 await seed(rpc,url,source,token,classicTokenBytes(f.baseMint,maker.publicKey,25n));
 const collateral={market,marketFlags:3,aggregateGuard,custody:f.custody,makerBalance:f.balances[0],
  maker:maker.publicKey,source,destination:source,vault:f.baseVault,mint:f.baseMint,
  vaultAuthority:f.authority,tokenProgram:token};
 for(const direction of ["deposit","withdraw"])
  await signed(rpc,transferMakerCollateral(program,collateral,{direction,asset:"base",amount:25n}),
   f.payer,[maker],result);
 fail((await rpc.getAccountInfo(source,"confirmed")).data.readBigUInt64LE(64)===25n,
  "Guarded maker collateral round-trip failed");
 const plans=[];
 for(const [i,side] of ["ask","bid","ask"].entries()){
  const snapshot=await authenticateSettlementCollateral(rpc,{adapter,programId:program,market:f.market.publicKey,
   side,headOwnerPage:side==="ask"?f.head:f.bidHead,genesisHex:hex(genesis),taker:f.taker.publicKey,takerBase:f.takerBase,takerQuote:f.takerQuote});
  fail(snapshot.reserveState.nextEpochId===BigInt(i)&&snapshot.header.aggregateGuardEnabled&&
   snapshot.aggregateGuard.pool.equals(pool),"Unexpected shared epoch or missing aggregate guard");
  const prepared=prepareSettlement(program,snapshot,{payer:f.payer.publicKey,systemProgram:SystemProgram.programId},
   {baseFill:150n,quoteLimit:side==="ask"?151n:150n});
  await writeFile(join(dir,"shared-"+i+"-roster.json"),json(prepared.plan),{mode:0o600,flag:"wx"});
  plans.push(prepared.plan);
  await signed(rpc,prepared.instruction,f.payer,[f.taker],result,[],prepared);
 }
 const load=()=>loadSettlementEpochs(rpc,{adapter,programId:program,market:f.market.publicKey,genesisHex:hex(genesis),
  epochIds:[0n,1n,2n],commitment:"confirmed"});
 const sealed=await load();
 fail(sealed.reserve.nextEpochId===3n&&sealed.epochs.every(e=>e.claimedCount===0),"Expected three simultaneously unclaimed epochs");
 for(const [i,e] of sealed.epochs.entries()){
  fail(e.totals.every((n,j)=>n===plans[i].ticks[0].totals[j]),"Shared epoch quote mismatch");
  result.epochs.push({id:String(e.epochId),side:e.side,root:e.claimRoot,totals:e.totals});
 }
 // Deliberately claim later ASK first, then BID, then oldest ASK.
 for(const i of [2,1,0]){
  const epoch=sealed.epochs[i],bid=epoch.side==="bid",makers=bid?f.bidMakers:f.makers,
   balances=bid?f.bidBalances:f.balances,destinations=bid?f.bidDestinations:f.destinations;
  for(const proof of recoverEpochProofs(epoch,plans[i].ticks[0].makers).reverse()){
   const j=makers.findIndex(m=>hex(m.publicKey)===proof.makerHex);
   fail(j>=0,"Shared maker missing");
   const accounts={market:f.market.publicKey,epoch:epoch.address,makerBalance:balances[j],maker:makers[j].publicKey,
    custody:f.custody,reserve:f.reserve,vault:bid?f.baseVault:f.quoteVault,destination:destinations[j],
    mint:bid?f.baseMint:f.quoteMint,vaultAuthority:f.authority,tokenProgram:token,
    marketFlags:3,aggregateGuard};
   await signed(rpc,buildSettlementClaim(program,accounts,epoch,proof),f.payer,[makers[j]],result);
  }
 }
 const final=await load(),r=final.reserve;
 fail(final.epochs.every(e=>e.claimedCount===e.makerCount&&e.remaining.every(n=>n===0n)),"Shared claims incomplete");
 fail([r.makerBaseCredit,r.makerQuoteCredit,r.makerBaseDebit,r.makerQuoteDebit].every(n=>n===0n),"Shared liabilities remain");
 // Redeem in two guarded, revision-bound burns after all active epochs settle.
 await signed(rpc,closePassivePosition(program,guarded,{nonce,expectedRevision:1n,burnLiquidity:40n}),
  f.payer,[f.taker],result);
 await signed(rpc,closePassivePosition(program,guarded,{nonce,expectedRevision:2n,burnLiquidity:60n}),
  f.payer,[f.taker],result);
 const poolAfter=await rpc.getAccountInfo(pool,"confirmed");
 fail(poolAfter?.data.subarray(48,96).every(x=>x===0),"LP inventory not fully redeemed");
 const [c,b,q]=await rpc.getMultipleAccountsInfo([f.custody,f.baseVault,f.quoteVault],"confirmed");
 const custody=decodeSettlementCustody(c.data),floor=reserveVaultFloor(r,custody.totalBase,custody.totalQuote);
 fail(custody.totalBase===0n&&custody.totalQuote===150n&&r.baseFees===1n&&r.quoteFees===2n&&
  b.data.readBigUInt64LE(64)===floor.base&&q.data.readBigUInt64LE(64)===floor.quote,"Shared final reconciliation failed");
 Object.assign(result,{complete:true,aggregateGuardVerified:true,finalReserve:r,finalFloor:floor});
}

// M83: isolated fixture, actual SDK messages and SBF receipts. Each phase
// is reloaded from the durable session; no manual account mutation after
// provisioning and funding. External fills occur only between retired intents.
async function runMakerSession(rpc,url,genesis,dir,report,incident=false){
 const prefix=incident?"m83-incident":"m83-maker";
 const f=await setup(rpc,url,"ask",false,genesis,true);
 const maker=f.makers[0],market=f.market.publicKey;
 const result={market:market.toBase58(),maker:maker.publicKey.toBase58(),
  receipts:[],mutations:[],complete:false};
 report[incident?"makerIncident":"makerSession"]=result;
 const snapshotOptions={adapter,programId:program,market,maker:maker.publicKey,
  askHeadOwner:f.head,bidHeadOwner:f.bidHead,expectedGenesisHash:genesis.toBase58()};
 const observe=(commitment="confirmed")=>loadLiveMakerSnapshot(rpc,{...snapshotOptions,
  commitment,maxLagSlots:32});
 for(const [asset,amount,mint,vault] of [["base",100n,f.baseMint,f.baseVault],
  ["quote",200n,f.quoteMint,f.quoteVault]]){
  const source=key().publicKey;
  await seed(rpc,url,source,token,classicTokenBytes(mint,maker.publicKey,amount));
  await signed(rpc,transferMakerCollateral(program,{market,custody:f.custody,
   makerBalance:f.balances[0],maker:maker.publicKey,source,vault,mint,tokenProgram:token},
   {direction:"deposit",asset,amount}),f.payer,[maker],result);
 }
 const initial=await observe();
 fail(initial.inventory.freeBase===100n&&initial.inventory.lockedBase===100n&&
  initial.inventory.freeQuote===200n&&initial.inventory.lockedQuote===0n,
  "M83 funding mismatch");
 const sessionPath=join(dir,prefix+".session"),policy={maxLagSlots:32,maxOrders:8,maxChanges:8,
  maxBaseExposure:"500",maxQuoteNotional:"500",maxLossQuote:"0",baselineEquityQuote:"400",
  maxOrdersPerWindow:8,maxTransactionsPerWindow:8,maxRetries:0};
 const configPath=join(dir,prefix+"-cli-config.json");
 const config={rpcUrl:url,expectedGenesisHash:genesis.toBase58(),market:market.toBase58(),
  maker:maker.publicKey.toBase58(),askHeadOwner:f.head.toBase58(),bidHeadOwner:f.bidHead.toBase58(),
  sessionPath,policy,orders:[],isolatedTestWalletAcknowledged:true,acknowledgeEmptySession:true};
 const child=promisify(execFile),script=fileURLToPath(new URL("./maker-session-cli.mjs",import.meta.url));
 const env={...process.env,HYBRID_SANDBOX_EXECUTION:"1",HYBRID_MAKER_KILL_SWITCH:"off"};
 const disabled={...env,HYBRID_SANDBOX_EXECUTION:"0",HYBRID_MAKER_KILL_SWITCH:"on"};
 await writeFile(configPath,json(config),{mode:0o600,flag:"wx"});
 async function command(name,extra=[],execution=true,expectedError=null){
  // Never forward child stdout/stderr on error: reserve output contains wire.
  let out;
  try{out=await child(process.execPath,[script,name,configPath,...extra],{
   env:execution?env:disabled,timeout:30000,maxBuffer:1024*1024});}
  catch(e){
   if(expectedError&&e.code===1&&expectedError.test(String(e.stderr)))return {rejected:true};
   throw Error("M83 CLI "+name+" failed (exit "+String(e.code??"unknown")+")");
  }
  fail(!expectedError,"M83 CLI "+name+" unexpectedly succeeded");
  return JSON.parse(out.stdout);
 }
 async function setOrders(orders){
  await writeFile(configPath,json({...config,orders}),{mode:0o600});
 }
 await command("sandbox-init");
 const ask=(qty,price="1")=>({side:"ask",priceAtoms:price,sizeAtoms:String(qty)});
 const bid=(qty,price="0.5")=>({side:"bid",priceAtoms:price,sizeAtoms:String(qty)});
 async function waitFinalized(signature){
  for(let i=0;i<120;i++){
   const statuses=await rpc.getSignatureStatuses([signature],{searchTransactionHistory:true});
   const status=statuses.value[0];
   if(status?.confirmationStatus==="finalized"){
    fail(status.err===null,"M83 finalized transaction failed");
    const receipt=await rpc.getTransaction(signature,{commitment:"finalized",maxSupportedTransactionVersion:0});
    if(receipt&&await rpc.getSlot("finalized")>=receipt.slot)return receipt;
   }
   await new Promise(r=>setTimeout(r,500));
  }
  throw Error("M83 finalized receipt unavailable");
 }
 async function partialFill(){
  const pre=await observe();
  const [page]=PublicKey.findProgramAddressSync([Buffer.from("ask-page"),bytes(market),u32(0)],program);
  const fill=await signed(rpc,swap(program,{market,custody:f.custody,page,ownerPage:f.head,
   taker:f.taker.publicKey,takerBase:f.takerBase,takerQuote:f.takerQuote,baseVault:f.baseVault,
   quoteVault:f.quoteVault,vaultAuthority:f.authority,tokenProgram:token,makerBalances:[f.balances[0]]},
   {side:"buy",mode:"exact-out",amount:25n,limit:25n}),f.payer,[f.taker],result);
  const receipt=await waitFinalized(fill.signature);
  const post=await observe();
  fail(post.ownedOrders.some(o=>o.side==="ask"&&o.sequence===1n&&o.baseQty===75n)&&
   post.inventory.lockedBase===pre.inventory.lockedBase-25n&&
   post.inventory.freeQuote===pre.inventory.freeQuote+25n&&
   post.custodyState.totalBase===pre.custodyState.totalBase-25n&&
   post.custodyState.totalQuote===pre.custodyState.totalQuote+25n&&
   post.vaultAmounts.base===pre.vaultAmounts.base-25n&&
   post.vaultAmounts.quote===pre.vaultAmounts.quote+25n,"M83 partial fill reconciliation mismatch");
  result.partialFill={signature:fill.signature,slot:receipt.slot,baseAtoms:"25",quoteAtoms:"25",verified:true};
 }
 async function mutate(orders,phase){
  await setOrders(orders);
  const reserved=await command("sandbox-reserve");
  fail(reserved.phase==="reserved"&&reserved.action,"M83 reservation missing");
  const tx=web3.VersionedTransaction.deserialize(Buffer.from(reserved.unsignedWireBase64,"base64"));
  tx.sign([maker]);
  const wirePath=join(dir,prefix+"-private-wire-"+result.mutations.length+".base64");
  await writeFile(wirePath,Buffer.from(tx.serialize()).toString("base64"),{mode:0o600,flag:"wx"});
  const prepared=await command("sandbox-prepare",[wirePath]);
  const inspected=await command("inspect",[],false);
  fail(inspected.phase==="prepared"&&inspected.signature===prepared.signature&&
   inspected.canSubmit===false&&!Object.hasOwn(inspected,"intent"),"M83 restart preparation mismatch");
  const beforeStatus=await command("status",[],false);
  fail(beforeStatus.state==="unknown"&&beforeStatus.phase==="prepared"&&
   beforeStatus.signature===prepared.signature&&!beforeStatus.canSubmit&&!beforeStatus.canRetry&&
   !beforeStatus.canRelease,"M83 disabled read-only status mismatch");
  const sent=await command("sandbox-submit",[wirePath]);
  fail(sent.state==="submitted"||sent.state==="unknown","M83 unexpected send result");
  const receipt=await waitFinalized(prepared.signature);
  const status=await command("status",[],false);
  fail(status.state==="finalized-success"&&status.slot===receipt.slot&&status.signature===prepared.signature&&
   !status.canSubmit&&!status.canRetry&&!status.canRelease,"M83 restarted finalized status mismatch");
  const final=await command("sandbox-finalize");
  fail(final.state==="finalized-success"&&final.finalizedSlot===receipt.slot,"M83 finality mismatch");
  if(incident){
   // A genuine finalized fill after the original maker receipt but before
   // retirement invalidates the strict expected post-state.
   await partialFill();
   fail(result.partialFill.slot>=receipt.slot,"M83 incident fill precedes mutation");
   const audit=await command("audit",[],false);
   fail(audit.matches===false&&audit.signature===prepared.signature&&
    ["inventory.lockedBase","inventory.freeQuote","ownedOrders","custody","vaults"]
      .every(k=>audit.differences.includes(k))&&
    !audit.canSubmit&&!audit.canRetry&&!audit.canRelease,"M83 incident audit mismatch");
   const rejected=await command("sandbox-reconcile",[],true,/Unexplained post-finality maker, custody or vault state/);
   fail(rejected.rejected,"M83 incident reconciliation unexpectedly released");
   const saved=await command("inspect",[],false);
   fail(saved.phase==="finalized"&&saved.signature===prepared.signature&&
    saved.transactionsReserved===1,"M83 incident journal not retained");
   const after=await command("audit",[],false);
   fail(!after.matches&&after.signature===prepared.signature,"M83 locked incident audit unavailable");
   const blocked=await command("sandbox-reserve",[],true,/EEXIST.*maker|EEXIST.*incident/);
   fail(blocked.rejected,"M83 incident permitted a new reservation");
   fail((await stat(sessionPath+".lock")).isFile()&&(await stat(sessionPath+".transaction")).isFile(),
    "M83 incident lock or journal missing");
   try{await stat(sessionPath+".settled."+prepared.signature);throw Error("M83 unexpected archive");}
   catch(e){if(e.code!=="ENOENT")throw e;}
   await unlink(wirePath);
   Object.assign(result,{complete:true,signature:prepared.signature,finalizedSlot:receipt.slot,
    audit:after,reconcileRejected:true,newReservationBlocked:true,retainedPhase:saved.phase,
    retainedCounters:saved.transactionsReserved});
   return;
  }
  const reconciled=await command("sandbox-reconcile");
  const saved=await command("inspect",[],false);
  await unlink(wirePath);
  fail(reconciled.state==="reconciled"&&saved.phase==="idle"&&
   saved.ordersReserved===result.mutations.length+1,"M83 retirement/counter mismatch");
  const archive=JSON.parse(await readFile(sessionPath+".settled."+prepared.signature,"utf8"));
  fail(archive.state.session.intent.phase===phase&&archive.state.journal.signature===prepared.signature,
   "M83 archived phase mismatch");
  result.mutations.push({phase,signature:prepared.signature,slot:receipt.slot,
   units:receipt.meta.computeUnitsConsumed,action:reserved.action,post:archive.state.audit,
   restartVerified:true,independentCliProcesses:true,disabledStatusVerified:true,reconciled:true});
 }
 await mutate([ask(100),bid(20)],"place");
 if(incident)return;
 await partialFill();
 await mutate([ask(90,"1.25"),bid(20)],"cancel");
 await mutate([ask(90,"1.25"),bid(20)],"place");
 await mutate([],"cancel");
 await mutate([],"cancel");
 const end=await observe();
 fail(end.ownedOrders.length===0&&end.inventory.lockedBase===0n&&end.inventory.lockedQuote===0n&&
  end.inventory.freeBase===175n&&end.inventory.freeQuote===225n&&
  end.vaultAmounts.base===end.floor.base&&end.vaultAmounts.quote===end.floor.quote,
  "M83 final inventory/vault mismatch");
 await setOrders([]);
 const noOp=await command("sandbox-reserve");
 fail(noOp.phase==="noop","M83 empty desired plan not idempotent");
 const saved=await command("inspect",[],false);
 fail(saved.phase==="idle"&&saved.transactionsReserved===5,"M83 final durable session mismatch");
 Object.assign(result,{complete:true,finalInventory:end.inventory,finalFloor:end.floor,
  durableCounters:{orders:saved.ordersReserved,transactions:saved.transactionsReserved},
  idempotentNoop:true});
}

async function main(){
 const url=clusterConfig("surfpool",process.env.SOLANA_RPC_URL||"http://127.0.0.1:8899").url;
 const rpc=new Connection(url,"confirmed");await assertCluster(rpc,"surfpool");
 fail((await rpc.getAccountInfo(program))?.executable,"Deploy SBF first");
 // The reserve domain is an operator assertion. Use a stable local test
 // domain, not a claim of public cluster identity.
 const genesis=new PublicKey(await rpc.getGenesisHash());
 const dir=resolve(process.env.HYBRID_SDK_EVIDENCE_DIR||"./.surfpool-sdk");
 await mkdir(dir,{recursive:true,mode:0o700});
 const report={network:"surfpool",program:PROGRAM_ID,fixtureSeeded:true,cases:[],complete:false};
 try{for(const side of ["ask","bid"])for(const multi of [false,true])await runCase(rpc,url,side,multi,genesis,dir,report);
  await runShared(rpc,url,genesis,dir,report);
  await runMakerSession(rpc,url,genesis,dir,report);
  await runMakerSession(rpc,url,genesis,dir,report,true);
  report.complete=true;
 }catch(error){report.error=error.message;throw error;}
 finally{await writeFile(join(dir,"sdk-settlement-evidence.json"),json(report),{mode:0o600});}
}
main().catch(e=>{console.error(e.message);process.exitCode=1;});
