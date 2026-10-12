import {
  Connection, PublicKey, Transaction, TransactionInstruction,
} from "@solana/web3.js";
import {Buffer} from "buffer";
import {PROGRAM_ID, TOKEN_PROGRAM_ID, placeData, cancelData, tradeData} from "./abi.js";
import "./style.css";

const $ = (id) => document.getElementById(id);
const program = new PublicKey(PROGRAM_ID);
const tokenProgram = new PublicKey(TOKEN_PROGRAM_ID);
let wallet;
let connection;
const read = (id) => $(id).value.trim();
const pk = (id) => new PublicKey(read(id));
const status = (s) => $("status").textContent = s;
const show = (value) => {
  $("output").textContent = typeof value === "string" ? value : JSON.stringify(value, null, 2);
};
const meta = (pubkey,isWritable=false,isSigner=false) => ({pubkey,isWritable,isSigner});
const pda = (seeds) => PublicKey.findProgramAddressSync(seeds,program)[0];
const b = (s) => new TextEncoder().encode(s);
const indexed = (side,index) => {
  const m = pk("market");
  const ix = new Uint8Array([index,0,0,0]);
  return {
    page:pda([b(side === "ask" ? "ask-page" : "bid-page"),m.toBytes(),ix]),
    owner:index===0 ? pk(side === "ask" ? "askOwner" : "bidOwner") :
      pda([b(side === "ask" ? "ask-owner-page" : "bid-owner-page"),m.toBytes(),ix]),
  };
};
const makerBalance = (owner) => pda([b("maker-balance"),pk("market").toBytes(),owner.toBytes()]);
const custody = () => pda([b("custody"),pk("market").toBytes()]);
const authority = () => pda([b("vault-authority"),pk("market").toBytes()]);
function signer() {
  if (!wallet?.publicKey) throw Error("Connect an injected Solana wallet first");
  return wallet.publicKey;
}
function rpc() {
  const url=read("rpc");
  const parsed=new URL(url);
  if (parsed.protocol!=="https:" && !(parsed.hostname==="127.0.0.1" && parsed.protocol==="http:")) throw Error("Use HTTPS RPC or localhost");
  return new Connection(url,"confirmed");
}
async function confirmDevnet() {
  connection=rpc();
  const genesis=await connection.getGenesisHash();
  // Solana devnet genesis hash, not a mainnet-beta or testnet cluster.
  if(genesis!=="EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG") throw Error("RPC is not the expected Solana devnet cluster");
}
function unique(keys) {
  const set=new Set(keys.map((key)=>key.toBase58()));
  if(set.size!==keys.length) throw Error("Aliased or duplicate account in transaction");
}
function orderInstruction(opcode) {
  const side=read("makerSide"),idx=Number(read("pageIndex"));
  const a=indexed(side,0),z=indexed(side,1);
  const owner=signer();
  const keys=[
    meta(pk("market"),true),meta(a.page,true),meta(a.owner,true),
    meta(z.page,true),meta(z.owner,true),meta(makerBalance(owner),true),
    meta(owner,false,true)
  ];
  unique(keys.map(x=>x.pubkey));
  const data=opcode===30 ?
    placeData(side,idx,read("price"),read("sqrtPrice"),read("quantity")):
    cancelData(side,idx,read("sequence"));
  return new TransactionInstruction({programId:program,keys,data:Buffer.from(data)});
}
function swapInstruction() {
  const side=read("tradeSide"),count=Number(read("pageCount")),owner=signer();
  const buy=side==="ask";
  const inToken=pk(buy?"quoteToken":"baseToken"),outToken=pk(buy?"baseToken":"quoteToken");
  const inVault=pk(buy?"quoteVault":"baseVault"),outVault=pk(buy?"baseVault":"quoteVault");
  const keys=[
    meta(pk("market"),true),meta(custody(),true),meta(owner,false,true),
    meta(inToken,true),meta(outToken,true),meta(inVault,true),meta(outVault,true),
    meta(authority()),meta(tokenProgram)
  ];
  for(let index=0;index<count;index++){
    const p=indexed(side,index);
    keys.push(meta(p.page,true),meta(p.owner,true));
  }
  const makers=read("makers").split(",").map(s=>s.trim()).filter(Boolean).map(s=>new PublicKey(s));
  if(makers.length===0||makers.length>8) throw Error("Provide 1–8 touched maker owners");
  for(const maker of makers)keys.push(meta(makerBalance(maker),true));
  unique(keys.map(k=>k.pubkey));
  const data=tradeData(side,read("tradeMode")==="out",count,read("tradeAmount"),read("tradeLimit"));
  return new TransactionInstruction({programId:program,keys,data:Buffer.from(data)});
}
async function snapshot() {
  await confirmDevnet();
  const market=pk("market");
  const pubkeys=[
    market,custody(),indexed("ask",0).page,indexed("ask",0).owner,
    indexed("bid",0).page,indexed("bid",0).owner,
    pk("baseVault"),pk("quoteVault"),pk("baseToken"),pk("quoteToken")
  ];
  const result=await connection.getMultipleAccountsInfoAndContext(pubkeys,"confirmed");
  const balances={};
  for(const name of ["baseToken","quoteToken","baseVault","quoteVault"]){
    try{balances[name]=(await connection.getTokenAccountBalance(pk(name),"confirmed")).value.amount;}
    catch(e){balances[name]="unavailable: "+e.message;}
  }
  show({
    cluster:"devnet",snapshotSlot:result.context.slot,
    accounts:pubkeys.map((key,i)=>({key:key.toBase58(),exists:!!result.value[i],
      owner:result.value[i]?.owner.toBase58()??null,
      bytes:result.value[i]?.data.length??0})),
    balances,
    note:"Book entries are protocol binary; do not infer orders from balances alone."
  });
}
async function send(ix){
  await confirmDevnet();
  const owner=signer();
  const accountKeys=ix.keys.map(x=>x.pubkey);
  const result=await connection.getMultipleAccountsInfoAndContext(accountKeys,"confirmed");
  if(result.value.some((v,i)=>v===null&&!accountKeys[i].equals(owner)&&!accountKeys[i].equals(tokenProgram)&&!accountKeys[i].equals(authority())))
    throw Error("Missing required initialized account. Inspect addresses before retrying.");
  const before={};
  for(const id of ["baseToken","quoteToken"]){
    before[id]=(await connection.getTokenAccountBalance(pk(id),"confirmed")).value.amount;
  }
  const {blockhash,lastValidBlockHeight}=await connection.getLatestBlockhash("confirmed");
  const tx=new Transaction({feePayer:owner,recentBlockhash:blockhash}).add(ix);
  const simulation=await connection.simulateTransaction(tx,{sigVerify:false,commitment:"confirmed"});
  if(simulation.value.err){
    show({simulationError:simulation.value.err,logs:simulation.value.logs});
    throw Error("Simulation rejected; no transaction submitted");
  }
  status("Simulation passed. Awaiting wallet approval");
  const signed=await wallet.signTransaction(tx);
  // Never retry an unconfirmed transaction with a new quote or blockhash.
  const signature=await connection.sendRawTransaction(signed.serialize(),{skipPreflight:false,maxRetries:0});
  status("Submitted "+signature+" — confirming");
  const confirmation=await connection.confirmTransaction({signature,blockhash,lastValidBlockHeight},"confirmed");
  if(confirmation.value.err)throw Error("Confirmed transaction failed: "+JSON.stringify(confirmation.value.err));
  const after={};
  for(const id of ["baseToken","quoteToken"]){
    after[id]=(await connection.getTokenAccountBalance(pk(id),"confirmed")).value.amount;
  }
  show({signature,explorer:"https://explorer.solana.com/tx/"+signature+"?cluster=devnet",
    before,after,simulationUnits:simulation.value.unitsConsumed,
    warning:"Confirm on an explorer and reload a fresh book snapshot before next action"});
  status("Confirmed "+signature);
}
async function action(fn){
  try{status("Validating and simulating");await fn();}
  catch(error){status("Stopped: "+error.message);show({error:error.message});}
}
$("connect").onclick=()=>action(async()=>{
  const provider=window.solana;
  if(!provider?.isPhantom && !provider?.signTransaction)throw Error("No compatible injected Solana wallet");
  wallet=provider;
  await wallet.connect();
  signer();await confirmDevnet();
  $("wallet").textContent=wallet.publicKey.toBase58();
  status("Connected to devnet");
});
$("refresh").onclick=()=>action(snapshot);
$("place").onclick=()=>action(()=>send(orderInstruction(30)));
$("cancel").onclick=()=>action(()=>send(orderInstruction(31)));
$("trade").onclick=()=>action(()=>send(swapInstruction()));
