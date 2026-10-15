// Funded six-stage Hybrid lifecycle against an already-provisioned local Surfpool market.
// Required: HYBRID_FIXTURE_CONFIG JSON, HYBRID_MAKER_KEYPAIR and HYBRID_TAKER_KEYPAIR file paths.
// The runner NEVER provisions or invents mints/market/vaults; failures remain explicit.
import {readFile,writeFile,mkdir} from "node:fs/promises";
import {resolve,dirname} from "node:path";
import {Connection,Keypair,PublicKey,SystemProgram,Transaction,TransactionInstruction} from "@solana/web3.js";
import {PROGRAM_ID,TOKEN_PROGRAM_ID,placeData,cancelData,tradeData,collateralData} from "./abi.js";
import {clusterConfig,assertCluster} from "./cluster.js";
import {decodeMarketHeader} from "./market.js";
import {decodeMakerBalance} from "./maker-balance.js";
import {decodeCustody} from "./custody.js";
import {decodeOwnedBookPage,combineOwnedBookPages} from "./book.js";
import {deriveMarketAccounts} from "./manifest.js";
import {verifyReceiptOrderLink} from "./receipt-policy.js";
import {compareMakerSnapshots,compareOrderLockSnapshots} from "./transitions.js";
import {verifyAskTradeSettlement} from "./trade-settlement.js";
import {verifyFinalLifecycle} from "./final-lifecycle-audit.js";
const program=new PublicKey(PROGRAM_ID),tokenProgram=new PublicKey(TOKEN_PROGRAM_ID);
const p=x=>new PublicKey(x);
const meta=(pubkey,isWritable=false,isSigner=false)=>({pubkey,isWritable,isSigner});
const instruction=(keys,data)=>new TransactionInstruction({programId:program,keys,data:Buffer.from(data)});
export function validateFixtureConfig(c){
 const required=["market","askOwner","bidOwner","baseVault","quoteVault","baseMint","quoteMint","makerBaseToken","makerQuoteToken","takerBaseToken","takerQuoteToken","priceX64","sqrtPriceX64","placeBaseAtoms","tradeQuoteAtoms","withdrawBaseAtoms"];
 for(const name of required)if(!c?.[name])throw Error("Missing fixture config: "+name);
 for(const name of required.filter(x=>!x.endsWith("Atoms")&&!x.endsWith("X64")))
  p(c[name]);
 for(const name of ["priceX64","sqrtPriceX64","placeBaseAtoms","tradeQuoteAtoms","withdrawBaseAtoms"])
  if(!/^[1-9][0-9]*$/.test(String(c[name])))throw Error("Invalid positive fixture amount: "+name);
 if(BigInt(c.withdrawBaseAtoms)>BigInt(c.placeBaseAtoms))
  throw Error("Withdrawal must not exceed initial maker deposit");
 return c;
}
async function loadSigner(file){
 const data=JSON.parse(await readFile(resolve(file),"utf8"));
 if(!Array.isArray(data)||data.length!==64||data.some(n=>!Number.isInteger(n)||n<0||n>255))
  throw Error("Signer file must contain a 64-byte Solana keypair array");
 return Keypair.fromSecretKey(Uint8Array.from(data));
}
export function lifecycleInstructions(c,maker,taker,sequence){
 const manifest=deriveMarketAccounts(c.market,maker.toBase58()),market=p(c.market);
 const book=[meta(p(manifest.askPage0),true),meta(p(c.askOwner),true),
             meta(p(manifest.askPage1),true),meta(p(manifest.askOwnerPage1),true)];
 const makerPda=p(manifest.makerBalance),custody=p(manifest.custody),authority=p(manifest.vaultAuthority);
 const init=instruction([meta(market),meta(custody),meta(makerPda,true),meta(maker,false,true),
   meta(maker,true,true),meta(SystemProgram.programId)],Uint8Array.of(12));
 const deposit=instruction([meta(market),meta(custody,true),meta(makerPda,true),meta(maker,false,true),
   meta(p(c.makerBaseToken),true),meta(p(c.baseVault),true),meta(p(c.baseMint)),meta(tokenProgram)],
   collateralData("deposit","base",c.placeBaseAtoms));
 const orderKeys=[meta(market,true),...book,meta(makerPda,true),meta(maker,false,true)];
 const place=instruction(orderKeys,placeData("ask",0,c.priceX64,c.sqrtPriceX64,c.placeBaseAtoms));
 const swap=instruction([meta(market,true),meta(custody,true),meta(taker,false,true),
   meta(p(c.takerQuoteToken),true),meta(p(c.takerBaseToken),true),
   meta(p(c.quoteVault),true),meta(p(c.baseVault),true),meta(authority),meta(tokenProgram),
   ...book,meta(makerPda,true)],tradeData("ask",false,2,c.tradeQuoteAtoms,1));
 const cancel=instruction(orderKeys,cancelData("ask",0,sequence));
 const withdraw=instruction([meta(market),meta(custody,true),meta(makerPda,true),meta(maker,false,true),
   meta(p(c.baseVault),true),meta(p(c.makerBaseToken),true),meta(p(c.baseMint)),
   meta(authority),meta(tokenProgram)],collateralData("withdraw","base",c.withdrawBaseAtoms));
 return {init,deposit,place,swap,cancel,withdraw};
}
async function snapshot(rpc,c,maker){
 const keys=deriveMarketAccounts(c.market,maker.toBase58());
 const addresses=[p(c.market),p(keys.custody),p(keys.makerBalance),p(c.baseVault),p(c.quoteVault),p(c.takerBaseToken),p(c.takerQuoteToken),
  p(keys.askPage0),p(c.askOwner),p(keys.askPage1),p(keys.askOwnerPage1),
  p(keys.bidPage0),p(c.bidOwner),p(keys.bidPage1),p(keys.bidOwnerPage1)];
 const batch=await rpc.getMultipleAccountsInfoAndContext(addresses,"confirmed");
 const [market,custody,balance,baseVault,quoteVault,takerBase,takerQuote,...books]=batch.value;
 if(!market||!custody||!baseVault||!quoteVault||!takerBase||!takerQuote||books.some(x=>!x))throw Error("Fixture account missing");
 const bookOrders={};
 for(const [side,offset] of [["ask",0],["bid",4]]){
  const pages=[0,1].map(index=>decodeOwnedBookPage(books[offset+index*2].data,
   books[offset+index*2+1].data,side,index));
  bookOrders[side]=combineOwnedBookPages(pages,side);
 }
 const header=decodeMarketHeader(market.data);
 if(bookOrders.ask.length!==header.askCount||bookOrders.bid.length!==header.bidCount)
  throw Error("Linked book order counts do not match market header");
 const vaultBalances={base:baseVault.data.readBigUInt64LE(64).toString(),
  quote:quoteVault.data.readBigUInt64LE(64).toString()};
 return {cluster:"surfpool",rpc:rpc.rpcEndpoint,slot:batch.context.slot,market:c.market,
  maker:maker.toBase58(),marketState:header,bookSlot:batch.context.slot,bookOrders,
  custodyState:decodeCustody(custody.data),
  makerState:balance?decodeMakerBalance(balance.data,c.market,maker.toBase58()):null,
  vaultBalances,takerBalances:{base:takerBase.data.readBigUInt64LE(64).toString(),
   quote:takerQuote.data.readBigUInt64LE(64).toString()}};
}
async function send(rpc,payer,signers,ix,onSubmitted=()=>{}){
 const {blockhash,lastValidBlockHeight}=await rpc.getLatestBlockhash("confirmed");
 const tx=new Transaction({feePayer:payer.publicKey,recentBlockhash:blockhash}).add(ix);
 tx.sign(...signers);
 const simulation=await rpc.simulateTransaction(tx);
 if(simulation.value.err)throw Error("Simulation failed: "+JSON.stringify({error:simulation.value.err,logs:simulation.value.logs}));
 const signature=await rpc.sendRawTransaction(tx.serialize(),{skipPreflight:false,maxRetries:0});
 await onSubmitted(signature);
 const confirmed=await rpc.confirmTransaction({signature,blockhash,lastValidBlockHeight},"confirmed");
 if(confirmed.value.err)throw Error("Confirmed transaction failed: "+signature+" "+JSON.stringify(confirmed.value.err));
 // Confirmation and transaction indexing need not become visible atomically.
 // Retry a null receipt briefly; a confirmed failure is never retried.
 let receipt=null;
 for(let attempt=0;attempt<10;attempt++){
  receipt=await rpc.getTransaction(signature,{commitment:"confirmed",maxSupportedTransactionVersion:0});
  if(receipt)break;
  await new Promise(r=>setTimeout(r,500));
 }
 if(!receipt||receipt.meta?.err)throw Error("Confirmed receipt missing/failed: "+signature);
 return {signature,slot:receipt.slot,feeLamports:receipt.meta.fee,simulationUnits:simulation.value.unitsConsumed};
}
export async function runFundedLifecycle({rpc,c,maker,taker,onJournal=async()=>{}}){
 validateFixtureConfig(c);
 if(maker.publicKey.equals(taker.publicKey))throw Error("Maker and taker must be distinct");
 const deployed=await rpc.getAccountInfo(program,"confirmed");
 if(!deployed?.executable)throw Error("Hybrid SBF program absent on Surfpool");
 const journal={cluster:"surfpool",market:c.market,maker:maker.publicKey.toBase58(),
  taker:taker.publicKey.toBase58(),startedAt:new Date().toISOString(),stages:[],completed:false};
 const persist=async()=>onJournal(journal);
 const record=async(stage,sender,ix,signers)=>{
  const before=await snapshot(rpc,c,maker.publicKey);
  const entry={stage,status:"simulating",before};
  journal.stages.push(entry);
  await persist();
  const receipt=await send(rpc,sender,signers,ix,async signature=>{
   entry.signature=signature;
   entry.status="submitted";
   await persist();
  });
  Object.assign(entry,receipt,{status:"confirmed"});
  await persist();
  const after=await snapshot(rpc,c,maker.publicKey);
  entry.after=after;
  entry.status="snapshotted";
  await persist();
  return {before,after};
 };
 try{
  const initial=await snapshot(rpc,c,maker.publicKey);
  if(initial.makerState)throw Error("Fixture requires uninitialized maker PDA");
  const sequence=initial.marketState.nextSequence;
  const ix=lifecycleInstructions(c,maker.publicKey,taker.publicKey,sequence);
  await record("init",maker,ix.init,[maker]);
  const deposit=await record("deposit",maker,ix.deposit,[maker]);
  compareMakerSnapshots(deposit.before,deposit.after,"deposit","base",c.placeBaseAtoms);
  const placement=await record("place",maker,ix.place,[maker]);
  compareOrderLockSnapshots(placement.before,placement.after,"place","ask",c.placeBaseAtoms,sequence,0);
  const trade=await record("trade",taker,ix.swap,[taker]);
  journal.tradeSettlement=verifyAskTradeSettlement(trade.before,trade.after);
  const cancel=await record("cancel",maker,ix.cancel,[maker]);
  compareOrderLockSnapshots(cancel.before,cancel.after,"cancel","ask",undefined,sequence,0);
  // Withdrawal is only valid if the remaining unlocked free base is sufficient.
  const withdrawal=await record("withdraw",maker,ix.withdraw,[maker]);
  compareMakerSnapshots(withdrawal.before,withdrawal.after,"withdraw","base",c.withdrawBaseAtoms);
  verifyReceiptOrderLink(ix.place.data,ix.cancel.data,sequence);
  journal.expectedSequence=String(sequence);
  journal.finalAudit=verifyFinalLifecycle({initial,deposit:deposit.after,
   trade:trade.after,final:withdrawal.after,
   placeAmount:c.placeBaseAtoms,withdrawAmount:c.withdrawBaseAtoms});
  journal.completed=true;
  await persist();
 }catch(error){
  journal.error=error.message;
  await persist();
  throw Object.assign(Error(error.message),{journal});
 }finally{journal.finishedAt=new Date().toISOString();await persist();}
 return journal;
}
async function main(){
 const c=validateFixtureConfig(JSON.parse(await readFile(resolve(process.env.HYBRID_FIXTURE_CONFIG),"utf8")));
 const maker=await loadSigner(process.env.HYBRID_MAKER_KEYPAIR);
 const taker=await loadSigner(process.env.HYBRID_TAKER_KEYPAIR);
 const cfg=clusterConfig("surfpool",process.env.SOLANA_RPC_URL||"http://127.0.0.1:8899");
 const rpc=new Connection(cfg.url,"confirmed");
 await assertCluster(rpc,"surfpool");
 const destination=resolve(process.env.HYBRID_EVIDENCE_FILE||"./surfpool-lifecycle-evidence.json");
 await mkdir(dirname(destination),{recursive:true});
 const onJournal=async journal=>writeFile(destination,JSON.stringify(journal,null,2)+"\n",{mode:0o600});
 let report;
 try{report=await runFundedLifecycle({rpc,c,maker,taker,onJournal});}
 catch(e){report=e.journal||{completed:false,error:e.message};process.exitCode=1;}
 await onJournal(report);
 console.log(JSON.stringify({completed:report.completed,stages:report.stages?.map(x=>({stage:x.stage,signature:x.signature,slot:x.slot})),error:report.error,evidence:destination},null,2));
}
if(process.argv[1]&&resolve(process.argv[1])===new URL(import.meta.url).pathname){
 main().catch(e=>{console.error(e.message);process.exitCode=1;});
}
