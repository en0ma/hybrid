// Actual SBF-backed SDK settlement on isolated loopback Surfpool.
import * as web3 from "@solana/web3.js";
import bs58 from "bs58";
import {writeFile,mkdir} from "node:fs/promises";
import {resolve,join} from "node:path";
import {clusterConfig,assertCluster} from "./cluster.js";
import {PROGRAM_ID,TOKEN_PROGRAM_ID} from "./abi.js";
import {marketBytes,detachedBookPage,classicMintBytes,classicTokenBytes,surfpoolAccountPayload} from "./provision-surfpool.js";
import {createSolanaSettlementAdapter} from "../../sdk/src/settlement-solana.mjs";
import {authenticateSettlementCollateral,loadSettlementEpochs,decodeSettlementCustody} from "../../sdk/src/settlement-rpc.mjs";
import {prepareSettlement} from "../../sdk/src/settlement-plan.mjs";
import {canonicalSqrtPriceX64} from "../../sdk/src/quote-orders.mjs";
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
async function setup(rpc,url,side,multi,genesis){
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
 c.writeBigUInt64LE(bid?0n:locked,176);c.writeBigUInt64LE(bid?locked:0n,184);
 await seed(rpc,url,custody,program,c);
 for(const mint of [baseMint,quoteMint])await seed(rpc,url,mint,token,classicMintBytes(payer.publicKey));
 for(const [address,mint,owner,amount] of [[baseVault,baseMint,authority,bid?0n:locked],
  [quoteVault,quoteMint,authority,bid?locked:0n],[takerBase,baseMint,taker.publicKey,bid?1000n:0n],
  [takerQuote,quoteMint,taker.publicKey,bid?0n:1000n]])await seed(rpc,url,address,token,classicTokenBytes(mint,owner,amount));
 const balances=[];
 for(const [i,r] of rows.entries()){
  const [balance,bump]=derive("maker-balance",bytes(r.maker.publicKey));balances.push(balance);
  const b=Buffer.alloc(112);b.write("HYBBAL01");b[8]=1;b[9]=bump;
  bytes(market.publicKey).copy(b,16);bytes(r.maker.publicKey).copy(b,48);
  b.writeBigUInt64LE(bid?(r.qty*r.price+Q-1n)/Q:r.qty,bid?104:88);
  await seed(rpc,url,balance,program,b);
  await seed(rpc,url,destinations[i],token,classicTokenBytes(bid?baseMint:quoteMint,r.maker.publicKey));
 }
 for(const actor of [payer,taker,market,...makers]){
  const sig=await rpc.requestAirdrop(actor.publicKey,2000000000);await rpc.confirmTransaction(sig,"confirmed");
 }
 return {market,payer,taker,head,baseMint,quoteMint,baseVault,quoteVault,takerBase,takerQuote,
  custody,reserve,authority,makers,balances,destinations,rows,genesis,side,multi};
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
  report.complete=true;
 }catch(error){report.error=error.message;throw error;}
 finally{await writeFile(join(dir,"sdk-settlement-evidence.json"),json(report),{mode:0o600});}
}
main().catch(e=>{console.error(e.message);process.exitCode=1;});
