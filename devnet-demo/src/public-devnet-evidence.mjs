#!/usr/bin/env node
// Read-only verification of a publicly collected, operator-supplied devnet evidence bundle.
// Does not provision wallets, accept secrets, or claim RPC confirmation independently.
// Solana identifiers are decoded and checked for their canonical byte lengths.
import { readFileSync } from "node:fs";
import bs58 from "bs58";

function validBase58Bytes(value, expectedLength) {
  if (typeof value !== "string" || !value.length) return false;
  try { return bs58.decode(value).length === expectedLength; } catch { return false; }
}
const file=process.argv[2];
if(!file){console.error("Usage: node src/public-devnet-evidence.mjs <evidence.json>");process.exit(2);}
let e;try{e=JSON.parse(readFileSync(file,"utf8"));}catch(error){console.error(error.message);process.exit(2);}
const errors=[];
const required=["initialize","deposit","place","trade","cancel","withdraw"];
if(e.network!=="devnet")errors.push("network must be devnet");
if(typeof e.rpc!=="string"||!e.rpc.startsWith("https://"))errors.push("expected HTTPS devnet RPC URL");
if(!validBase58Bytes(e.programId,32))errors.push("programId must be a 32-byte base58 Solana public key");
if(!validBase58Bytes(e.market,32))errors.push("market must be a 32-byte base58 Solana public key");
if(!validBase58Bytes(e.maker,32)||!validBase58Bytes(e.taker,32)||e.maker===e.taker)errors.push("distinct, valid 32-byte base58 maker and taker addresses required");
const stages=Array.isArray(e.stages)?e.stages:[];
if(stages.length!==6||stages.some((x,i)=>x?.name!==required[i]||!validBase58Bytes(x.signature,64)||x.confirmed!==true))errors.push("require six ordered confirmed stage records with signature");
if(new Set(stages.map(s=>s?.signature)).size!==6)errors.push("stage signatures must be unique");
if(e.audit?.tradeSettlementVerified!==true||e.audit?.finalBalancesReconciled!==true||e.audit?.bookStateVerified!==true)errors.push("independent trade, book, and final-balance audits required");
if(e.receipts?.verifiedOnPublicRpc!==true)errors.push("public-RPC receipt verification missing");
const summary={valid:errors.length===0,network:e.network??null,market:e.market??null,stages:stages.map(s=>s?.name),errors};
console.log(JSON.stringify(summary,null,2));if(errors.length)process.exitCode=1;
