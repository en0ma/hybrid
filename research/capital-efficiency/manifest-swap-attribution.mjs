// Authenticate a direct Manifest core Swap instruction from an RPC json receipt.
// Upstream generated Swap.ts specifies discriminator 4 and market account at index 1.
// This is a receipt attribution check, not by itself a custody/price proof.
export function inspectManifestSwapReceipt({receipt,signature,programId,market,taker}){
 if(!receipt||receipt.meta?.err!==null||!Number.isSafeInteger(receipt.slot)||receipt.slot<=0)throw Error("successful finalized receipt required");
 if(receipt.transaction?.signatures?.[0]!==signature)throw Error("transaction signature mismatch");
 const message=receipt.transaction?.message,keys=message?.accountKeys;
 if(!Array.isArray(keys)||!Array.isArray(message.instructions))throw Error("raw JSON instruction layout required");
 const loaded=receipt.meta?.loadedAddresses;
 const accounts=[...keys,...(loaded?.writable??[]),...(loaded?.readonly??[])];
 const swaps=[];
 for(const ix of message.instructions){
  if(!Number.isSafeInteger(ix.programIdIndex)||ix.programIdIndex<0||ix.programIdIndex>=accounts.length)throw Error("malformed instruction program index");
  if(accounts[ix.programIdIndex]!==programId)continue;
  if(!Array.isArray(ix.accounts)||ix.accounts.length<8||ix.accounts.some(i=>!Number.isSafeInteger(i)||i<0||i>=accounts.length))throw Error("malformed Manifest instruction accounts");
  if(accounts[ix.accounts[1]]!==market)continue;
  // Solana JSON encoding uses base58 for compiled instruction data.
  // Decode first byte only without dependencies; '5' is base58 encoding of u8 discriminator 4.
  if(ix.data==="5"||ix.data?.startsWith("5"))throw Error("ambiguous instruction encoding; require full byte decoder");
  swaps.push(ix);
 }
 return {verified:false,matchedProgramMarketInstructions:swaps.length,warning:"Cannot establish Swap opcode without decoding compiled base58 instruction bytes"};
}
