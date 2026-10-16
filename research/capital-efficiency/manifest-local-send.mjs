// Controlled signed transaction sender for Manifest benchmark instructions.
// Only loopback RPC is accepted; this is a building block, not a full benchmark.
export function requireLocalRpc(url){
 const u=new URL(url);
 if(u.protocol!=="http:"||!["127.0.0.1","localhost","[::1]"].includes(u.hostname)||u.username||u.password||u.search||u.hash)
  throw Error("Manifest benchmark submission requires credential-free loopback HTTP RPC");
 return u.href;
}
export async function sendLocalManifestInstruction({rpcUrl,connection,payer,instruction,Transaction}){
 requireLocalRpc(rpcUrl);
 if(!connection||!payer?.publicKey||!instruction||!Transaction)throw Error("missing signed transaction prerequisites");
 if(connection.rpcEndpoint!==rpcUrl&&connection.rpcEndpoint!==new URL(rpcUrl).href)throw Error("Connection endpoint mismatch");
 const {blockhash,lastValidBlockHeight}=await connection.getLatestBlockhash("confirmed");
 const tx=new Transaction({feePayer:payer.publicKey,recentBlockhash:blockhash}).add(instruction);
 tx.sign(payer);
 const sim=await connection.simulateTransaction(tx);
 if(sim.value?.err)throw Error("Manifest instruction simulation failed: "+JSON.stringify(sim.value.err));
 const signature=await connection.sendRawTransaction(tx.serialize(),{skipPreflight:false,maxRetries:0});
 const confirmation=await connection.confirmTransaction({signature,blockhash,lastValidBlockHeight},"confirmed");
 if(confirmation.value?.err)throw Error("Manifest transaction failed: "+signature);
 const receipt=await connection.getTransaction(signature,{commitment:"confirmed",maxSupportedTransactionVersion:0});
 if(!receipt||receipt.meta?.err!==null||!Number.isSafeInteger(receipt.slot)||!Number.isSafeInteger(receipt.meta?.computeUnitsConsumed))
  throw Error("Confirmed Manifest receipt missing, failed or unmetered");
 if(receipt.transaction?.signatures?.[0]!==signature)throw Error("Manifest receipt signature mismatch");
 return {status:"executed",signature,slot:receipt.slot,computeUnits:receipt.meta.computeUnitsConsumed,feeLamports:receipt.meta.fee??null,
  verification:"confirmed receipt only; independent finalized program/market and token conservation audit required"};
}
