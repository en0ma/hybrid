// Read-only, fail-closed Solana signature status observer.
// It NEVER authorizes expiry from an RPC null result, signs or broadcasts.
// The caller must independently verify the journalled signature and cluster.
const BASE58=/^[1-9A-HJ-NP-Za-km-z]{80,90}$/;
const validHeight=n=>Number.isSafeInteger(n)&&n>=0;
export async function observeSignatureStatus(connection,{
  signature,expectedGenesisHash,lastValidBlockHeight,
}){
  if(typeof signature!=="string"||!BASE58.test(signature))
    throw new TypeError("Invalid transaction signature");
  if(typeof expectedGenesisHash!=="string"||!BASE58.test(expectedGenesisHash))
    throw new TypeError("Expected cluster genesis hash required");
  if(!validHeight(lastValidBlockHeight))
    throw new RangeError("Invalid lastValidBlockHeight");
  if(!connection||typeof connection.getGenesisHash!=="function"||
     typeof connection.getSignatureStatuses!=="function"||
     typeof connection.getBlockHeight!=="function")
    throw new TypeError("Required RPC status methods unavailable");
  const actualGenesis=await connection.getGenesisHash();
  if(actualGenesis!==expectedGenesisHash)
    throw new Error("RPC cluster genesis hash mismatch");
  // Search historical statuses. A null result remains UNKNOWN and cannot
  // prove absence, even after blockhash expiry.
  const response=await connection.getSignatureStatuses(
    [signature],{searchTransactionHistory:true}
  );
  const contextSlot=response?.context?.slot;
  const statuses=response?.value;
  if(!validHeight(contextSlot)||contextSlot===0||
      !Array.isArray(statuses)||statuses.length!==1)
    throw new Error("Malformed transaction-status RPC response");
  const status=statuses[0];
  const currentBlockHeight=await connection.getBlockHeight("finalized");
  if(!validHeight(currentBlockHeight))
    throw new Error("Malformed finalized block height");
  const expiredBlockhash=currentBlockHeight>lastValidBlockHeight;
  if(status===null)return {
    state:"unknown",contextSlot,currentBlockHeight,expiredBlockhash,
    canConfirm:false,canExpire:false,
  };
  if(!status||!validHeight(status.slot)||status.slot===0||
      !["processed","confirmed","finalized",undefined].includes(status.confirmationStatus))
    throw new Error("Malformed signature status");
  // Null confirmations without an explicit 'finalized' commitment do not
  // authorize clearing a pending journal.
  if(status.confirmationStatus!=="finalized")return {
    state:"pending",contextSlot,currentBlockHeight,expiredBlockhash,
    canConfirm:false,canExpire:false,
  };
  if(status.slot>contextSlot)
    throw new Error("Finalized signature slot exceeds RPC context");
  if(status.err!==null&&status.err===undefined)
    throw new Error("Missing transaction error field");
  if(status.err!==null)return {
    state:"finalized-failure",slot:status.slot,contextSlot,currentBlockHeight,
    expiredBlockhash,canConfirm:false,canExpire:false,
  };
  return {
    state:"finalized-success",slot:status.slot,contextSlot,currentBlockHeight,
    expiredBlockhash,canConfirm:true,canExpire:false,
  };
}
