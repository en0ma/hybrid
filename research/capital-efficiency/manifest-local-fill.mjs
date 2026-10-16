import {sendLocalManifestInstruction,requireLocalRpc} from "./manifest-local-send.mjs";
import {correlateManifestTrade} from "./manifest-trade-correlation.mjs";
// Calls a supplied funded, isolated Manifest Swap instruction against loopback RPC.
// No fabricated local signer or market; caller must provision both.
export async function runLocalManifestFill({rpcUrl,connection,payer,instruction,Transaction,programId,market,taker,baseMint,quoteMint,makerOwner,vaultAccounts,snapshotMarket}){
 requireLocalRpc(rpcUrl);
 if(typeof snapshotMarket!=="function")throw Error("real Manifest market snapshot loader required");
 if(payer?.publicKey?.toBase58?.()!==taker)throw Error("transaction signer must equal expected taker");
 const before=await snapshotMarket();
 if(before?.market!==market||before?.baseMint!==baseMint||before?.quoteMint!==quoteMint)throw Error("pre-trade market identity mismatch");
 if(!Number.isSafeInteger(before.slot)||before.slot<=0)throw Error("pre-trade market slot required");
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
 if(receipt.meta?.computeUnitsConsumed!==executed.computeUnits||receipt.meta?.fee!==executed.feeLamports)
  throw Error("independent finalized receipt metrics disagree with submitted transaction");
 const audit=correlateManifestTrade({receipt,signature:executed.signature,programId,market,taker,baseMint,quoteMint,makerOwner,before,after,vaultAccounts});
 return {kind:"manifest-local-isolated-fill-evidence",verified:false,execution:{signature:executed.signature,slot:executed.slot,computeUnits:executed.computeUnits,feeLamports:executed.feeLamports},audit,limitations:["No runtime funded Manifest fixture is supplied by this module","Market snapshots may span unrelated slots or market events","Never treat this evidence as a completed paired Hybrid–Manifest benchmark"]};
}
