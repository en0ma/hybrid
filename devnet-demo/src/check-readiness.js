// Read-only devnet readiness check. No wallet or private key required.
import {Connection,PublicKey} from "@solana/web3.js";
import {PROGRAM_ID,TOKEN_PROGRAM_ID} from "./abi.js";
import {decodeCustody,verifyCustodyLinks} from "./custody.js";

const names=["market","askOwner","bidOwner","baseVault","quoteVault"];
const args=process.argv.slice(2);
if(args.length!==names.length){
  console.error("Usage: node src/check-readiness.js <market> <ask-owner-sidecar> <bid-owner-sidecar> <base-vault> <quote-vault>");
  process.exit(2);
}
const conn=new Connection(process.env.SOLANA_DEVNET_RPC||"https://api.devnet.solana.com","confirmed");
const genesis=await conn.getGenesisHash();
if(genesis!=="EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG")throw Error("RPC is not devnet");
const program=new PublicKey(PROGRAM_ID);
const token=new PublicKey(TOKEN_PROGRAM_ID);
const keys=Object.fromEntries(names.map((name,i)=>[name,new PublicKey(args[i])]));
const seeds=(name,index)=>[new TextEncoder().encode(name),keys.market.toBytes(),new Uint8Array([index,0,0,0])];
const derive=(array)=>PublicKey.findProgramAddressSync(array,program)[0];
const derived={
  custody:derive([new TextEncoder().encode("custody"),keys.market.toBytes()]),
  askPage0:derive(seeds("ask-page",0)),bidPage0:derive(seeds("bid-page",0)),
  askPage1:derive(seeds("ask-page",1)),bidPage1:derive(seeds("bid-page",1)),
  askOwner1:derive(seeds("ask-owner-page",1)),bidOwner1:derive(seeds("bid-owner-page",1))
};
const checks={...keys,...derived,program};
const ids=Object.entries(checks);
const result=await conn.getMultipleAccountsInfoAndContext(ids.map(([,key])=>key),"confirmed");
const rows=ids.map(([name,key],i)=>{
  const a=result.value[i],expectedOwner=["baseVault","quoteVault"].includes(name)?token:program;
  return {name,key:key.toBase58(),exists:!!a,owner:a?.owner.toBase58()??null,
    bytes:a?.data.length??0,executable:a?.executable??false,
    valid:!!a&&(name==="program"?a.executable:a.owner.equals(expectedOwner))};
});
let custodyState=null;
const custodyIndex=ids.findIndex(([name])=>name==="custody");
try {
  const raw=result.value[custodyIndex]?.data;
  if(!raw)throw Error("Custody account missing");
  custodyState=decodeCustody(raw);
  verifyCustodyLinks(custodyState,keys.market.toBase58(),keys.baseVault.toBase58(),keys.quoteVault.toBase58());
} catch(error) {
  rows[custodyIndex].valid=false;
  rows[custodyIndex].error=error.message;
}
// SPL Token account layout: mint [0..32], authority [32..64], amount [64..72].
const authority=derive([new TextEncoder().encode("vault-authority"),keys.market.toBytes()]).toBase58();
for(const [name,expectedMint] of [["baseVault",custodyState?.baseMint],["quoteVault",custodyState?.quoteMint]]){
  const i=ids.findIndex(([n])=>n===name);
  const account=result.value[i];
  if(!account||!expectedMint)continue;
  try {
    if(account.data.length!==165)throw Error("Vault is not a classic SPL Token account");
    const mint=new PublicKey(account.data.subarray(0,32)).toBase58();
    const owner=new PublicKey(account.data.subarray(32,64)).toBase58();
    if(mint!==expectedMint||owner!==authority)throw Error("Vault mint or PDA authority mismatch");
    rows[i].mint=mint;
    rows[i].tokenAuthority=owner;
    const amount=account.data.readBigUInt64LE(64);
    rows[i].amountAtoms=amount.toString();
    const recorded=BigInt(name==="baseVault"?custodyState.totalBase:custodyState.totalQuote);
    if(amount<recorded)throw Error("Vault balance below custody recorded collateral total");
  }catch(error){rows[i].valid=false;rows[i].error=error.message;}
}
const bad=rows.filter(r=>!r.valid);
console.log(JSON.stringify({network:"devnet",slot:result.context.slot,rows,custodyState,ready:bad.length===0,
  warning:"Custody and SPL vault links are checked, but this does not prove book integrity, maker balances, or sufficient vault reserves."},null,2));
if(bad.length)process.exitCode=1;
