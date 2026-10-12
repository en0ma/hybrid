// Evidence verifier: checks real successful devnet transactions invoking Hybrid.
// Usage: node src/verify-receipts.js <place-signature> <trade-signature> <cancel-signature>
// Does not claim the three signatures are causally linked without market/state evidence.
import {Connection,PublicKey} from "@solana/web3.js";
import {PROGRAM_ID} from "./abi.js";
import {verifiesTradeMovement,tokenBalanceDeltas} from "./evidence.js";
const sigs=process.argv.slice(2);
if(sigs.length!==3||sigs.some(s=>!/^[1-9A-HJ-NP-Za-km-z]{80,90}$/.test(s))){
  console.error("Usage: node src/verify-receipts.js <place-signature> <trade-signature> <cancel-signature>");
  process.exit(2);
}
const rpc=process.env.SOLANA_DEVNET_RPC||"https://api.devnet.solana.com";
const connection=new Connection(rpc,"confirmed");
const expected="EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG";
if(await connection.getGenesisHash()!==expected) throw Error("Not a devnet RPC");
const program=new PublicKey(PROGRAM_ID);
const wanted=[new Set([30]),new Set([23,24,25,26]),new Set([31])];
const report=[];
if(new Set(sigs).size!==sigs.length)throw Error("Signatures must be three distinct transactions");
let commonMarket=null;
let previousSlot=0;
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
  if(!market)throw Error("Hybrid market account is missing");
  if(commonMarket===null)commonMarket=market;
  if(market!==commonMarket)throw Error("Place, trade and cancel target different markets");
  if(tx.slot<previousSlot)throw Error("Receipt slots are not chronological");
  previousSlot=tx.slot;
  const opcodes=matching.map(ix=>bs58.decode(ix.data)[0]);
  if(i===1 && !verifiesTradeMovement(tx.meta.preTokenBalances,tx.meta.postTokenBalances)){
    throw Error("Trade receipt has no two-sided SPL balance movement");
  }
  const tokenDeltas=tokenBalanceDeltas(tx.meta.preTokenBalances,tx.meta.postTokenBalances)
    .map(x=>({accountIndex:x.accountIndex,deltaAtoms:x.delta.toString()}));
  report.push({tokenDeltas,kind:["place","trade","cancel"][i],signature:sigs[i],slot:tx.slot,
    opcodes,feeLamports:tx.meta.fee,
    preTokenBalances:tx.meta.preTokenBalances||[],postTokenBalances:tx.meta.postTokenBalances||[]});
}
console.log(JSON.stringify({cluster:"devnet",program:program.toBase58(),
  market:commonMarket,
  warning:"These receipts share a market, but do not prove same maker/order, exact fill or inventory. Verify state transitions.",report},null,2));
