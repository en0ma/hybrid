// Local-only Surfpool fixture provisioner. Requires an already deployed Hybrid SBF program.
// Program-owned market/pages and SPL layouts are seeded via Surfpool cheatcodes;
// Hybrid custody and linked page initialization execute signed on-chain instructions.
import {readFile,writeFile,mkdir} from "node:fs/promises";
import {resolve,join} from "node:path";
import {Connection,Keypair,PublicKey,SystemProgram,Transaction,TransactionInstruction} from "@solana/web3.js";
import {PROGRAM_ID,TOKEN_PROGRAM_ID} from "./abi.js";
import {clusterConfig,assertCluster} from "./cluster.js";
import {deriveMarketAccounts} from "./manifest.js";
import {decodeMarketHeader} from "./market.js";
import {decodeCustody} from "./custody.js";
const PROGRAM=new PublicKey(PROGRAM_ID),TOKEN=new PublicKey(TOKEN_PROGRAM_ID),Q64=1n<<64n;
const key=()=>Keypair.generate(),pub=k=>k.publicKey.toBase58();
const writeU64=(b,o,n)=>b.writeBigUInt64LE(BigInt(n),o);
const writeU128=(b,o,n)=>{const v=BigInt(n);writeU64(b,o,v&((1n<<64n)-1n));writeU64(b,o+8,v>>64n);};
const meta=(p,w=false,s=false)=>({pubkey:p,isWritable:w,isSigner:s});
export function marketBytes(askOwner,bidOwner){
 const data=Buffer.alloc(128);
 data.write("HYBRID01",0,"ascii");data[8]=1;data[9]=1;
 writeU64(data,32,1);writeU128(data,48,Q64);
 writeU64(data,80,1);writeU64(data,88,1);
 new PublicKey(askOwner).toBuffer().copy(data,96);
 const bid=new PublicKey(bidOwner).toBuffer();
 bid.copy(data,24,0,8);bid.copy(data,40,8,16);
 return data;
}
export function detachedBookPage(index,ownerSidecar=false){
 if(![0,1].includes(index))throw Error("Expected book page 0 or 1");
 const data=Buffer.alloc(ownerSidecar?1040:1552);
 data.writeUInt32LE(index,2);
 data.writeUInt32LE(0xffffffff,6);
 data.writeUInt32LE(0xffffffff,10);
 return data;
}
export function classicMintBytes(mintAuthority,decimals=6){
 const data=Buffer.alloc(82);
 data.writeUInt32LE(1,0);
 new PublicKey(mintAuthority).toBuffer().copy(data,4);
 writeU64(data,36,1000000n);data[44]=decimals;data[45]=1;
 return data;
}
export function classicTokenBytes(mint,owner,amount=0n){
 const data=Buffer.alloc(165);
 new PublicKey(mint).toBuffer().copy(data,0);
 new PublicKey(owner).toBuffer().copy(data,32);
 writeU64(data,64,amount);data[108]=1;
 return data;
}
export function provisioningPlan(keys){
 const m=deriveMarketAccounts(pub(keys.market),pub(keys.maker));
 const accounts=[
  {name:"market",address:pub(keys.market),owner:PROGRAM_ID,data:marketBytes(pub(keys.askOwner),pub(keys.bidOwner))},
  {name:"askPage0",address:m.askPage0,owner:PROGRAM_ID,data:detachedBookPage(0)},
  {name:"askOwner0",address:pub(keys.askOwner),owner:PROGRAM_ID,data:detachedBookPage(0,true)},
  {name:"askPage1",address:m.askPage1,owner:PROGRAM_ID,data:detachedBookPage(1)},
  {name:"askOwner1",address:m.askOwnerPage1,owner:PROGRAM_ID,data:detachedBookPage(1,true)},
  {name:"bidPage0",address:m.bidPage0,owner:PROGRAM_ID,data:detachedBookPage(0)},
  {name:"bidOwner0",address:pub(keys.bidOwner),owner:PROGRAM_ID,data:detachedBookPage(0,true)},
  {name:"bidPage1",address:m.bidPage1,owner:PROGRAM_ID,data:detachedBookPage(1)},
  {name:"bidOwner1",address:m.bidOwnerPage1,owner:PROGRAM_ID,data:detachedBookPage(1,true)},
  {name:"baseMint",address:pub(keys.baseMint),owner:TOKEN_PROGRAM_ID,data:classicMintBytes(pub(keys.maker))},
  {name:"quoteMint",address:pub(keys.quoteMint),owner:TOKEN_PROGRAM_ID,data:classicMintBytes(pub(keys.maker))},
  {name:"baseVault",address:pub(keys.baseVault),owner:TOKEN_PROGRAM_ID,data:classicTokenBytes(pub(keys.baseMint),m.vaultAuthority)},
  {name:"quoteVault",address:pub(keys.quoteVault),owner:TOKEN_PROGRAM_ID,data:classicTokenBytes(pub(keys.quoteMint),m.vaultAuthority)},
  {name:"makerBaseToken",address:pub(keys.makerBaseToken),owner:TOKEN_PROGRAM_ID,data:classicTokenBytes(pub(keys.baseMint),pub(keys.maker),1000000n)},
  {name:"makerQuoteToken",address:pub(keys.makerQuoteToken),owner:TOKEN_PROGRAM_ID,data:classicTokenBytes(pub(keys.quoteMint),pub(keys.maker))},
  {name:"takerBaseToken",address:pub(keys.takerBaseToken),owner:TOKEN_PROGRAM_ID,data:classicTokenBytes(pub(keys.baseMint),pub(keys.taker))},
  {name:"takerQuoteToken",address:pub(keys.takerQuoteToken),owner:TOKEN_PROGRAM_ID,data:classicTokenBytes(pub(keys.quoteMint),pub(keys.taker),1000000n)}
 ];
 if(new Set(accounts.map(a=>a.address)).size!==accounts.length)throw Error("Fixture address alias");
 return {accounts,manifest:m};
}
function config(keys){
 return Object.fromEntries(["market","askOwner","bidOwner","baseVault","quoteVault","baseMint","quoteMint",
  "makerBaseToken","makerQuoteToken","takerBaseToken","takerQuoteToken"].map(k=>[k,pub(keys[k])]));
}
async function cheatcode(url,method,params){
 const response=await fetch(url,{method:"POST",headers:{"content-type":"application/json"},
  body:JSON.stringify({jsonrpc:"2.0",id:1,method,params})});
 if(!response.ok)throw Error(method+" HTTP "+response.status);
 const data=await response.json();
 if(data.error)throw Error(method+" "+JSON.stringify(data.error));
 return data.result;
}
async function send(rpc,signers,ix){
 const {blockhash,lastValidBlockHeight}=await rpc.getLatestBlockhash("confirmed");
 const tx=new Transaction({feePayer:signers[0].publicKey,recentBlockhash:blockhash}).add(ix);
 tx.sign(...signers);
 const sim=await rpc.simulateTransaction(tx);
 if(sim.value.err)throw Error("Hybrid init simulation failed: "+JSON.stringify({error:sim.value.err,logs:sim.value.logs}));
 const signature=await rpc.sendRawTransaction(tx.serialize(),{maxRetries:0,skipPreflight:false});
 const confirm=await rpc.confirmTransaction({signature,blockhash,lastValidBlockHeight},"confirmed");
 if(confirm.value.err)throw Error("Hybrid init transaction failed: "+signature);
 return signature;
}
export async function provisionSurfpool({rpc,keys,rpcUrl}){
 const deployed=await rpc.getAccountInfo(PROGRAM,"confirmed");
 if(!deployed?.executable)throw Error("Deploy fixed-ID Hybrid SBF program before provisioning");
 const plan=provisioningPlan(keys);
 // Never replace pre-existing accounts: protect earlier runs and unrelated markets.
 const existing=await rpc.getMultipleAccountsInfo(plan.accounts.map(a=>new PublicKey(a.address)),"confirmed");
 if(existing.some(Boolean))throw Error("Fixture accounts already exist; use a fresh Surfpool instance/keys");
 if(!(await rpc.getAccountInfo(TOKEN,"confirmed"))?.executable)
  throw Error("Classic SPL Token program missing from Surfpool; token settlement cannot execute");
 for(const a of plan.accounts){
  await cheatcode(rpcUrl,"surfnet_setAccount",[a.address,{
   data:"0x"+a.data.toString("hex"),executable:false,lamports:10000000,
   owner:a.owner,rentEpoch:0
  }]);
 }
 // Read-back validates the RPC actually persisted the bytes and ownership.
 const observed=await rpc.getMultipleAccountsInfo(plan.accounts.map(a=>new PublicKey(a.address)),"confirmed");
 for(let i=0;i<plan.accounts.length;i++){
  const a=plan.accounts[i],actual=observed[i];
  if(!actual||!actual.owner.equals(new PublicKey(a.owner))||!actual.data.equals(a.data))
   throw Error("Surfpool seeded account read-back mismatch: "+a.name);
 }
 for(const actor of [keys.market,keys.maker,keys.taker]){
  const sig=await rpc.requestAirdrop(actor.publicKey,5000000000);
  await rpc.confirmTransaction(sig,"confirmed");
 }
 const m=plan.manifest;
 const ix=new TransactionInstruction({programId:PROGRAM,
  keys:[meta(keys.market.publicKey,true,true),meta(new PublicKey(m.custody),true),
   meta(keys.maker.publicKey,true,true),meta(keys.baseMint.publicKey),meta(keys.quoteMint.publicKey),
   meta(keys.baseVault.publicKey),meta(keys.quoteVault.publicKey),meta(SystemProgram.programId),meta(TOKEN)],
  data:Buffer.from([11,6,6])});
 const receipts={custodyInit:await send(rpc,[keys.maker,keys.market],ix)};
 for(const side of ["ask","bid"]){
  const book=side==="ask"?
    [m.askPage0,pub(keys.askOwner),m.askPage1,m.askOwnerPage1]:
    [m.bidPage0,pub(keys.bidOwner),m.bidPage1,m.bidOwnerPage1];
  const link=new TransactionInstruction({programId:PROGRAM,
   keys:[meta(keys.market.publicKey,true,true),...book.map(x=>meta(new PublicKey(x),true))],
   data:Buffer.from([29,side==="ask"?0:1])});
  receipts[side+"Link"]=await send(rpc,[keys.maker,keys.market],link);
 }
 const state=await rpc.getAccountInfo(keys.market.publicKey,"confirmed");
 if(!decodeMarketHeader(state.data).collateralizedActive)throw Error("Custody activation not recorded");
 const custody=await rpc.getAccountInfo(new PublicKey(m.custody),"confirmed");
 if(decodeCustody(custody.data).market!==pub(keys.market))throw Error("Custody market mismatch");
 return {config:{...config(keys),priceX64:Q64.toString(),sqrtPriceX64:Q64.toString(),
   placeBaseAtoms:"1000",tradeQuoteAtoms:"100",withdrawBaseAtoms:"100"},
   receipts,addresses:plan.accounts.map(a=>({name:a.name,address:a.address}))};
}
async function main(){
 const cfg=clusterConfig("surfpool",process.env.SOLANA_RPC_URL||"http://127.0.0.1:8899");
 const rpc=new Connection(cfg.url,"confirmed");await assertCluster(rpc,"surfpool");
 const dir=resolve(process.env.HYBRID_FIXTURE_DIR||"./.surfpool-fixture");
 await mkdir(dir,{recursive:true,mode:0o700});
 const names=["market","maker","taker","askOwner","bidOwner","baseMint","quoteMint",
   "baseVault","quoteVault","makerBaseToken","makerQuoteToken","takerBaseToken","takerQuoteToken"];
 const keys={};for(const n of names){const f=join(dir,n+".json");
  try{const secret=JSON.parse(await readFile(f,"utf8"));keys[n]=Keypair.fromSecretKey(Uint8Array.from(secret));}
  catch(e){if(e.code!=="ENOENT")throw e;keys[n]=key();await writeFile(f,JSON.stringify([...keys[n].secretKey]),{mode:0o600,flag:"wx"});}
 }
 const result=await provisionSurfpool({rpc,keys,rpcUrl:cfg.url});
 await writeFile(join(dir,"fixture.json"),JSON.stringify(result.config,null,2)+"\n",{mode:0o600,flag:"wx"});
 await writeFile(join(dir,"provision-evidence.json"),JSON.stringify(result,null,2)+"\n",{mode:0o600,flag:"wx"});
 console.log(JSON.stringify({fixture:join(dir,"fixture.json"),maker:join(dir,"maker.json"),
  taker:join(dir,"taker.json"),receipts:result.receipts},null,2));
}
if(process.argv[1]&&resolve(process.argv[1])===new URL(import.meta.url).pathname)
 main().catch(e=>{console.error(e.message);process.exitCode=1;});
