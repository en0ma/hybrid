// Evidence verifier: checks real successful devnet transactions invoking Hybrid.
// Usage: node src/verify-receipts.js <place-signature> <trade-signature> <cancel-signature>
// Does not claim the three signatures are causally linked without market/state evidence.
import {Connection,PublicKey} from "@solana/web3.js";
import {PROGRAM_ID} from "./abi.js";
import {verifiesTradeMovement,tokenBalanceDeltas} from "./evidence.js";
const sigs=process.argv.slice(2);
const full=sigs.length===6;
const steps=full?["initialize maker","deposit","place","trade","cancel","withdraw"]:["place","trade","cancel"];
const wanted=full?[new Set([12]),new Set([13]),new Set([30]),new Set([23,24,25,26]),new Set([31]),new Set([14])]:[new Set([30]),new Set([23,24,25,26]),new Set([31])];
if(![3,6].includes(sigs.length)||sigs.some(s=>!/^[1-9A-HJ-NP-Za-km-z]{80,90}$/.test(s))){
  console.error("Usage: node src/verify-receipts.js <place> <trade> <cancel> OR <init-maker> <deposit> <place> <trade> <cancel> <withdraw>");
  process.exit(2);
}
const rpc=process.env.SOLANA_DEVNET_RPC||"https://api.devnet.solana.com";
const connection=new Connection(rpc,"confirmed");
const expected="EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG";
if(await connection.getGenesisHash()!==expected) throw Error("Not a devnet RPC");
const program=new PublicKey(PROGRAM_ID);
const report=[];
if(new Set(sigs).size!==sigs.length)throw Error("Each stage needs a distinct transaction signature");
let commonMarket=null;
let previousSlot=0;
let placingMaker=null;
for(let i=0;i<sigs.length;i++){
  const tx=await connection.getParsedTransaction(sigs[i],{commitment:"confirmed",maxSupportedTransactionVersion:0});
  if(!tx||tx.meta?.err)throw Error("Transaction missing or failed at position "+(i+1));
  const instructions=tx.transaction.message.instructions;
  const hits=instructions.filter(ix=>ix.programId.equals(program));
  // ParsedTransaction contains partially decoded instructions with base58
  // program instruction data; verify opcode separately only after decoding.
  const {default:bs58}=await import("bs58");
  const matching=hits.filter(ix=>"data" in ix && wanted[i].has(bs58.decode(ix.data)[0]));
  if(matching.length!==1)throw Error("Expected exactly one Hybrid instruction at position "+(i+1));
  const market=matching[0].accounts[0]?.toBase58();
  const signedKeys=new Set(tx.transaction.message.accountKeys.filter(x=>x.signer).map(x=>x.pubkey.toBase58()));
  if(!market)throw Error("Hybrid market account is missing");
  if(commonMarket===null)commonMarket=market;
  if(market!==commonMarket)throw Error("Place, trade and cancel target different markets");
  if(tx.slot<previousSlot)throw Error("Receipt slots are not chronological");
  previousSlot=tx.slot;
  const opcodes=matching.map(ix=>bs58.decode(ix.data)[0]);
  const opcode=opcodes[0];
  if([12,13,14,30,31].includes(opcode)){
    const ix=matching[0];
    const makerIndex=opcode===30||opcode===31?6:3;
    const accountLength={12:6,13:8,14:9,30:7,31:7}[opcode];
    if(ix.accounts.length!==accountLength)throw Error("Unexpected lifecycle instruction account layout");
    const maker=ix.accounts[makerIndex].toBase58();
    if(!signedKeys.has(maker))throw Error("Maker is not a transaction signer");
    if(placingMaker===null)placingMaker=maker;
    if(maker!==placingMaker)throw Error("Lifecycle maker differs from initial maker");
  }
  if([23,24,25,26].includes(opcode) && !signedKeys.has(matching[0].accounts[2].toBase58())){
    throw Error("Taker is not a transaction signer");
  }
  if([23,24,25,26].includes(opcode) && placingMaker){
    // Linked swaps list page+sidecar pairs followed by touched maker-balance PDAs.
    const ix=matching[0];
    const decoded=bs58.decode(ix.data);
    const pageCount=decoded[1];
    if(!pageCount||pageCount>8||ix.accounts.length<9+2*pageCount+1){
      throw Error("Malformed linked swap account layout");
    }
    const expectedBalance=PublicKey.findProgramAddressSync([
      Buffer.from("maker-balance"),
      new PublicKey(commonMarket).toBuffer(),
      new PublicKey(placingMaker).toBuffer()
    ],program)[0].toBase58();
    const touched=ix.accounts.slice(9+2*pageCount).map(x=>x.toBase58());
    if(!touched.includes(expectedBalance)) {
      throw Error("Trade did not include placing maker collateral PDA");
    }
  }
  if([23,24,25,26].includes(opcode) && !verifiesTradeMovement(tx.meta.preTokenBalances,tx.meta.postTokenBalances)){
    throw Error("Trade receipt has no two-sided SPL balance movement");
  }
  const tokenDeltas=tokenBalanceDeltas(tx.meta.preTokenBalances,tx.meta.postTokenBalances)
    .map(x=>({accountIndex:x.accountIndex,deltaAtoms:x.delta.toString()}));
  report.push({tokenDeltas,kind:steps[i],signature:sigs[i],slot:tx.slot,
    opcodes,feeLamports:tx.meta.fee,
    preTokenBalances:tx.meta.preTokenBalances||[],postTokenBalances:tx.meta.postTokenBalances||[]});
}
console.log(JSON.stringify({cluster:"devnet",program:program.toBase58(),
  market:commonMarket,placingMaker,
  warning:"Receipts share a market and authenticated maker; they do not prove the same order sequence, correct fills, market funding or vault solvency. Inspect decoded state.",report},null,2));
