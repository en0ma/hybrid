// Evidence verifier: checks real successful devnet transactions invoking Hybrid.
// Usage: node src/verify-receipts.js <place-signature> <trade-signature> <cancel-signature>
// Does not claim the three signatures are causally linked without market/state evidence.
import {Connection,PublicKey} from "@solana/web3.js";
import {PROGRAM_ID} from "./abi.js";
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
for(let i=0;i<sigs.length;i++){
  const tx=await connection.getParsedTransaction(sigs[i],{commitment:"confirmed",maxSupportedTransactionVersion:0});
  if(!tx||tx.meta?.err)throw Error("Transaction missing or failed at position "+(i+1));
  const instructions=tx.transaction.message.instructions;
  const hits=instructions.filter(ix=>ix.programId.equals(program));
  // ParsedTransaction contains partially decoded instructions with base58
  // program instruction data; verify opcode separately only after decoding.
  const {default:bs58}=await import("bs58");
  const opcodes=hits.filter(ix=>"data" in ix).map(ix=>bs58.decode(ix.data)[0]);
  if(!opcodes.some(op=>wanted[i].has(op)))throw Error("Expected Hybrid opcode absent at position "+(i+1));
  report.push({kind:["place","trade","cancel"][i],signature:sigs[i],slot:tx.slot,
    opcodes,feeLamports:tx.meta.fee,
    preTokenBalances:tx.meta.preTokenBalances||[],postTokenBalances:tx.meta.postTokenBalances||[]});
}
console.log(JSON.stringify({cluster:"devnet",program:program.toBase58(),
  warning:"Signatures prove execution only; verify same market and order ownership before milestone sign-off",report},null,2));
