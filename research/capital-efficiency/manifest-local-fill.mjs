import {sendLocalManifestInstruction,requireLocalRpc} from "./manifest-local-send.mjs";
import {correlateManifestTrade} from "./manifest-trade-correlation.mjs";
// Calls a supplied funded, isolated Manifest Swap instruction against loopback RPC.
// No fabricated local signer or market; caller must provision both.
export async function runLocalManifestFill({rpcUrl,connection,payer,instruction,Transaction,programId,market,taker,baseMint,quoteMint,makerOwner,vaultAccounts,snapshotMarket}){
 requireLocalRpc(rpcUrl);
 if(typeof snapshotMarket!=="function")throw Error("real Manifest market snapshot loader required");
 if(payer?.publicKey?.toBase58?.()!==taker)throw Error("transaction signer must equal expected taker");
 // The loader must use the same loopback RPC as the sender; never accept
 // a second unverified data source for snapshots.
 if(connection?.rpcEndpoint!==rpcUrl&&connection?.rpcEndpoint!==new URL(rpcUrl).href)throw Error("snapshot and sender RPC endpoint mismatch");
 // Fail before a signer is used unless this is exactly a core Swap with the
 // generated account layout and fixed-length atom-safe SwapParams payload.
 const key=x=>typeof x==="string"?x:x?.toBase58?.();
 if(key(instruction?.programId)!==programId||!Array.isArray(instruction?.keys)||instruction.keys.length<8||
     key(instruction.keys[0]?.pubkey)!==taker||instruction.keys[0]?.isSigner!==true||
     key(instruction.keys[1]?.pubkey)!==market)
  throw Error("local Manifest fill requires an authenticated core Swap instruction");
 const tokenKeys=instruction.keys.slice(3,7).map(k=>key(k.pubkey));
 if(tokenKeys.some(k=>!k)||new Set(tokenKeys).size!==4)throw Error("local Manifest fill requires distinct taker/vault token keys");
 const bytes=instruction.data;
 if(!(bytes instanceof Uint8Array)||bytes.length!==19||bytes[0]!==4||
     bytes[17]>1||bytes[18]>1||new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength).getBigUint64(1,true)===0n||
     new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength).getBigUint64(9,true)===0n)
  throw Error("local Manifest fill requires valid atom-safe core SwapParams");
 const before=await snapshotMarket();
 if(before?.market!==market||before?.baseMint!==baseMint||before?.quoteMint!==quoteMint)throw Error("pre-trade market identity mismatch");
 if(!Number.isSafeInteger(before.slot)||before.slot<=0)throw Error("pre-trade market slot required");
 if(before.programId!==programId||!/^([a-f0-9]{40})$/i.test(before.commit??""))throw Error("pre-trade program identity and pinned source commit required");
 const executed=await sendLocalManifestInstruction({rpcUrl,connection,payer,instruction,Transaction,expectedProgramId:programId,expectedMarket:market});
 const after=await snapshotMarket();
 if(after?.market!==market||after?.baseMint!==baseMint||after?.quoteMint!==quoteMint)throw Error("post-trade market identity mismatch");
 if(before.commit!==after.commit||before.programId!==after.programId||before.programId!==programId)throw Error("market snapshot program or source provenance changed");
 if(!Number.isSafeInteger(after.slot)||after.slot<=executed.slot||before.slot>=executed.slot)throw Error("market snapshots must strictly bracket finalized execution slot");
 // Solana getTransaction JSON encoding gives raw base58 compiled instruction data,
 // not decoded SDK TransactionInstruction objects.
 const receipt=await connection.getTransaction(executed.signature,{commitment:"finalized",maxSupportedTransactionVersion:0,encoding:"json"});
 if(!receipt||receipt.slot!==executed.slot||receipt.transaction?.signatures?.[0]!==executed.signature||receipt.meta?.err!==null)
  throw Error("raw finalized Manifest receipt disagrees with signed submission");
 if(!Number.isSafeInteger(receipt.meta?.computeUnitsConsumed)||receipt.meta.computeUnitsConsumed<0||!Number.isSafeInteger(receipt.meta?.fee)||receipt.meta.fee<0)throw Error("independent receipt missing valid compute or fee accounting");
 if(receipt.meta?.computeUnitsConsumed!==executed.computeUnits||receipt.meta?.fee!==executed.feeLamports)
  throw Error("independent finalized receipt metrics disagree with submitted transaction");
 const audit=correlateManifestTrade({receipt,signature:executed.signature,programId,market,taker,baseMint,quoteMint,makerOwner,before,after,vaultAccounts});
 return {kind:"manifest-local-isolated-fill-evidence",verified:false,execution:{signature:executed.signature,slot:executed.slot,computeUnits:executed.computeUnits,feeLamports:executed.feeLamports},audit,limitations:["No runtime funded Manifest fixture is supplied by this module","Market snapshots may span unrelated slots or market events","Never treat this evidence as a completed paired Hybrid–Manifest benchmark"]};
}
