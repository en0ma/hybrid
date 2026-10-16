// Read-only Solana RPC receipt collector. Does not sign or submit transactions.
// Results are partial evidence; an independent token-conservation audit is required.
import {readFileSync,writeFileSync} from "node:fs";
import {pathToFileURL} from "node:url";
export async function collectReceipts({rpc,signatures,expectedProgramId,fetchImpl=fetch}){
 if(typeof rpc!=="string"||!/^https:\/\//.test(rpc))throw Error("HTTPS RPC endpoint required");
 if(typeof expectedProgramId!=="string"||!/^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(expectedProgramId))throw Error("expectedProgramId must be a base58 Solana program public key");
 if(!Array.isArray(signatures)||!signatures.length||signatures.some(s=>typeof s!=="string"||!/^[1-9A-HJ-NP-Za-km-z]{64,88}$/.test(s)))throw Error("nonempty base58 signatures required");
 if(new Set(signatures).size!==signatures.length)throw Error("duplicate signature");
 const records=[];
 for(let i=0;i<signatures.length;i++){
  const sig=signatures[i];
  const res=await fetchImpl(rpc,{method:"POST",headers:{"content-type":"application/json"},body:JSON.stringify({jsonrpc:"2.0",id:i+1,method:"getTransaction",params:[sig,{encoding:"json",commitment:"finalized",maxSupportedTransactionVersion:0}]})});
  if(!res.ok)throw Error("RPC HTTP "+res.status);
  const json=await res.json();if(json.error)throw Error("RPC error: "+JSON.stringify(json.error));
  const tx=json.result;
  if(!tx||tx.meta?.err!==null||!Number.isInteger(tx.slot)||!Number.isInteger(tx.meta?.computeUnitsConsumed))throw Error("unavailable, failed or unmetered finalized transaction: "+sig);
  const accountKeys=tx.transaction?.message?.accountKeys;
  if(!Array.isArray(accountKeys))throw Error("unsupported receipt account layout: "+sig);
  const loaded=tx.meta.loadedAddresses;
  const keys=[...accountKeys,...(loaded?.writable??[]),...(loaded?.readonly??[])];
  const referenced=new Set();
  for(const ix of tx.transaction.message.instructions??[]){
   if(!Number.isInteger(ix.programIdIndex)||ix.programIdIndex<0||ix.programIdIndex>=keys.length)throw Error("malformed program index: "+sig);
   referenced.add(keys[ix.programIdIndex]);
  }
  if(!referenced.has(expectedProgramId))throw Error("transaction did not directly invoke expected program: "+sig);
  const extra=(loaded?.writable?.length??0)+(loaded?.readonly?.length??0);
  records.push({signature:sig,expectedProgramId,slot:tx.slot,blockTime:tx.blockTime??null,computeUnits:tx.meta.computeUnitsConsumed,accountMetas:accountKeys.length+extra,feeLamports:tx.meta.fee??null});
 }
 return {kind:"executed-solana-receipts",rpc,expectedProgramId,transactions:records.length,receiptSignatures:signatures,computeUnitsTotal:records.reduce((a,x)=>a+x.computeUnits,0),accountMetasMaximum:Math.max(...records.map(x=>x.accountMetas)),records,limitations:["No signature ownership proof beyond on-chain transaction data","No independent token conservation or inventory audit","No signing latency or rent measurement inferred from receipts"]};
}
if(process.argv[1]&&import.meta.url===pathToFileURL(process.argv[1]).href){
 const [configFile,outFile]=process.argv.slice(2);
 if(!configFile){console.error("Usage: node collect-receipts.mjs <receipts-input.json> [result.json]");process.exit(2)}
 try{const {rpc,signatures,expectedProgramId}=JSON.parse(readFileSync(configFile,"utf8"));const result=await collectReceipts({rpc,signatures,expectedProgramId});const content=JSON.stringify(result,null,2)+"\n";if(outFile)writeFileSync(outFile,content);else process.stdout.write(content);}
 catch(error){console.error(error.message);process.exitCode=1}
}
