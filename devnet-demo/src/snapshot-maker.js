// Reproducible non-signing Surfpool snapshot for before/after lifecycle comparison.
import {Connection,PublicKey} from "@solana/web3.js";
import {PROGRAM_ID} from "./abi.js";
import {clusterConfig,assertCluster} from "./cluster.js";
import {decodeMarketHeader} from "./market.js";
import {decodeCustody} from "./custody.js";
import {decodeMakerBalance} from "./maker-balance.js";
import {decodeOwnedBookPage,combineOwnedBookPages} from "./book.js";
const args=process.argv.slice(2);
if(args.length!==2){console.error("Usage: npm run snapshot:maker -- <MARKET> <MAKER>");process.exit(2);}
const mode=process.env.HYBRID_CLUSTER||"surfpool";
const cfg=clusterConfig(mode,process.env.SOLANA_RPC_URL||(mode==="surfpool"?
 "http://127.0.0.1:8899":"https://api.devnet.solana.com"));
const rpc=new Connection(cfg.url,"confirmed");
await assertCluster(rpc,mode);
const [market,maker]=args.map(x=>new PublicKey(x));
const program=new PublicKey(PROGRAM_ID);
const derive=(seed)=>PublicKey.findProgramAddressSync(
 [new TextEncoder().encode(seed),market.toBytes()],program)[0];
const custody=derive("custody");
const balance=PublicKey.findProgramAddressSync(
 [new TextEncoder().encode("maker-balance"),market.toBytes(),maker.toBytes()],program)[0];
const custodyInfo=await rpc.getAccountInfo(custody,"confirmed");
if(!custodyInfo||!custodyInfo.owner.equals(program))throw Error("Custody absent / wrong owner");
const initialCustody=decodeCustody(custodyInfo.data);
const vaultKeys=[new PublicKey(initialCustody.baseVault),new PublicKey(initialCustody.quoteVault)];
// Discover custody vault addresses, then fetch every relevant account together.
// The initial custody read is only for discovery; validate it against the final batch.
const preliminaryMarket=await rpc.getAccountInfo(market,"confirmed");
if(!preliminaryMarket||!preliminaryMarket.owner.equals(program))throw Error("Market missing");
const marketStatePre=decodeMarketHeader(preliminaryMarket.data);
// Capture both sides' order pages and their owner sidecars in one RPC response.
const bookKeys=[];
for(const side of ["ask","bid"]){
 const owner0=side==="ask"?new PublicKey(marketStatePre.askOwner):null;
 // Bid page-0 commitment stores only an owner-address prefix; caller supplies
 // its full public key through HYBRID_BID_OWNER0 for an authenticated lookup.
 const bidOwner=process.env.HYBRID_BID_OWNER0?new PublicKey(process.env.HYBRID_BID_OWNER0):null;
 const headOwner=side==="ask"?owner0:bidOwner;
 if(!headOwner){
  if(side==="bid")continue;
  throw Error("Missing ask head owner");
 }
 if(side==="bid"&&!marketStatePre.bidOwnerTag.every((v,i)=>v===headOwner.toBytes()[i]))
  throw Error("Bid owner sidecar commitment mismatch");
 for(let index=0;index<2;index++){
  const indexBytes=Buffer.alloc(4);indexBytes.writeUInt32LE(index);
  const page=PublicKey.findProgramAddressSync(
   [new TextEncoder().encode(side+"-page"),market.toBytes(),indexBytes],program)[0];
  const owner=index===0?headOwner:PublicKey.findProgramAddressSync(
   [new TextEncoder().encode(side+"-owner-page"),market.toBytes(),indexBytes],program)[0];
  bookKeys.push({side,index,page,owner});
 }
}
const bookAccounts=bookKeys.flatMap(x=>[x.page,x.owner]);
const result=await rpc.getMultipleAccountsInfoAndContext([market,custody,balance,...vaultKeys,...bookAccounts],"confirmed");
const [m,c,b,baseVault,quoteVault]=result.value;
if(!m||!m.owner.equals(program)||!c||!c.owner.equals(program))
 throw Error("Market or custody absent / wrong program owner");
const marketState=decodeMarketHeader(m.data);
const custodyState=decodeCustody(c.data);
if(custodyState.market!==market.toBase58()||
 custodyState.baseVault!==initialCustody.baseVault||
 custodyState.quoteVault!==initialCustody.quoteVault)
 throw Error("Custody changed during snapshot discovery");
if(marketState.askOwner!==marketStatePre.askOwner||
 !marketState.bidOwnerTag.every((x,i)=>x===marketStatePre.bidOwnerTag[i]))
 throw Error("Market owner commitments changed during snapshot discovery");
if(b&&!b.owner.equals(program))throw Error("Maker balance has wrong program owner");
const makerState=b?decodeMakerBalance(b.data,market.toBase58(),maker.toBase58()):null;
const tokenProgram=new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const vaultAmounts=[baseVault,quoteVault].map((a,i)=>{
 if(!a||!a.owner.equals(tokenProgram)||a.data.length!==165||a.data[108]!==1)
  throw Error("Vault missing, frozen, or wrong SPL layout");
 const mint=new PublicKey(a.data.subarray(0,32)).toBase58();
 const authority=new PublicKey(a.data.subarray(32,64)).toBase58();
 const expectedAuthority=derive("vault-authority").toBase58();
 if(mint!==(i===0?custodyState.baseMint:custodyState.quoteMint)||authority!==expectedAuthority)
  throw Error("Vault mint or authority mismatch");
 return a.data.readBigUInt64LE(64).toString();
});
const bookResponse={value:result.value.slice(5),context:result.context};
const ownedOrders={};
for(const side of ["ask","bid"]){
 const ids=bookKeys.filter(x=>x.side===side);
 if(ids.length!==2)continue;
 const decoded=ids.map(x=>{
  const i=bookKeys.indexOf(x)*2;
  const page=bookResponse.value[i],owners=bookResponse.value[i+1];
  if(!page||!owners||!page.owner.equals(program)||!owners.owner.equals(program))
   throw Error("Book page/sidecar absent or wrong owner");
  return decodeOwnedBookPage(page.data,owners.data,side,x.index);
 });
 ownedOrders[side]=combineOwnedBookPages(decoded,side);
 const expected=side==="ask"?marketState.askCount:marketState.bidCount;
 if(ownedOrders[side].length!==expected)throw Error("Decoded book count differs from market header");
}
const report={cluster:mode,rpc:cfg.url,slot:result.context.slot,market:market.toBase58(),
 maker:maker.toBase58(),custody:custody.toBase58(),makerPda:balance.toBase58(),
 marketState:{...marketState,bidOwnerTag:[...marketState.bidOwnerTag]},
 custodyState,makerState,vaultBalances:{base:vaultAmounts[0],quote:vaultAmounts[1]},
 bookOrders:ownedOrders,bookSlot:bookResponse.context.slot,
 warning:"Maker, custody, vault and linked book accounts were fetched in one RPC batch at the reported slot; this does not establish a signed transaction or market-wide solvency."};
console.log(JSON.stringify(report,null,2));
