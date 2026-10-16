#!/usr/bin/env node
// Read-only verification of a publicly collected, operator-supplied devnet evidence bundle.
// Does not provision wallets, accept secrets, or claim RPC confirmation independently.
import { readFileSync } from "node:fs";
const file=process.argv[2];
if(!file){console.error("Usage: node src/public-devnet-evidence.mjs <evidence.json>");process.exit(2);}
let e;try{e=JSON.parse(readFileSync(file,"utf8"));}catch(error){console.error(error.message);process.exit(2);}
const errors=[];
const required=["initialize","deposit","place","trade","cancel","withdraw"];
if(e.network!=="devnet")errors.push("network must be devnet");
if(typeof e.rpc!=="string"||!e.rpc.startsWith("https://"))errors.push("expected HTTPS devnet RPC URL");
if(typeof e.programId!=="string"||!e.programId.trim())errors.push("missing programId");
if(typeof e.market!=="string"||!e.market.trim())errors.push("missing market");
if(typeof e.maker!=="string"||typeof e.taker!=="string"||!e.maker||!e.taker||e.maker===e.taker)errors.push("distinct maker and taker addresses required");
const stages=Array.isArray(e.stages)?e.stages:[];
if(stages.length!==6||stages.some((x,i)=>x?.name!==required[i]||typeof x.signature!=="string"||x.signature.length<40||x.confirmed!==true))errors.push("require six ordered confirmed stage records with signature");
if(new Set(stages.map(s=>s?.signature)).size!==6)errors.push("stage signatures must be unique");
if(e.audit?.tradeSettlementVerified!==true||e.audit?.finalBalancesReconciled!==true||e.audit?.bookStateVerified!==true)errors.push("independent trade, book, and final-balance audits required");
if(e.receipts?.verifiedOnPublicRpc!==true)errors.push("public-RPC receipt verification missing");
const summary={valid:errors.length===0,network:e.network??null,market:e.market??null,stages:stages.map(s=>s?.name),errors};
console.log(JSON.stringify(summary,null,2));if(errors.length)process.exitCode=1;
