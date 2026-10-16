// Authenticate an isolated Manifest core Swap (generated Swap.ts: discriminator 4).
const alphabet="123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
export function decodeBase58(value){
 if(typeof value!=="string"||!value)throw Error("invalid base58 instruction bytes");
 let n=0n;for(const ch of value){const i=alphabet.indexOf(ch);if(i<0)throw Error("invalid base58 instruction bytes");n=n*58n+BigInt(i)}
 const bytes=[];while(n>0n){bytes.unshift(Number(n&255n));n>>=8n}
 return Uint8Array.from([...Array([...value].findIndex(ch=>ch!=="1")===-1?value.length:[...value].findIndex(ch=>ch!=="1")).fill(0),...bytes]);
}
export function inspectManifestSwapReceipt({receipt,signature,programId,market,taker}){
 if(!receipt||receipt.meta?.err!==null||!Number.isSafeInteger(receipt.slot)||receipt.slot<=0)throw Error("successful finalized receipt required");
 if(receipt.transaction?.signatures?.[0]!==signature)throw Error("transaction signature mismatch");
 const message=receipt.transaction?.message,keys=message?.accountKeys;
 if(!Array.isArray(keys)||!Array.isArray(message.instructions))throw Error("raw JSON instruction layout required");
 const required=message.header?.numRequiredSignatures;
 if(!Number.isSafeInteger(required)||required<1||required>keys.length||receipt.transaction.signatures.length!==required)throw Error("invalid Solana message signer header");
 const signedKeys=new Set(keys.slice(0,required));
 if(!signedKeys.has(taker))throw Error("Manifest Swap payer is not an authenticated transaction signer");
 const loaded=receipt.meta?.loadedAddresses;const accounts=[...keys,...(loaded?.writable??[]),...(loaded?.readonly??[])];
 let count=0;
 for(const ix of message.instructions){
  if(!Number.isSafeInteger(ix.programIdIndex)||ix.programIdIndex<0||ix.programIdIndex>=accounts.length)throw Error("malformed program index");
  if(accounts[ix.programIdIndex]!==programId)continue;
  if(!Array.isArray(ix.accounts)||ix.accounts.some(i=>!Number.isSafeInteger(i)||i<0||i>=accounts.length))throw Error("malformed Manifest accounts");
  const data=decodeBase58(ix.data);
  if(data[0]!==4)continue;
  if(ix.accounts.length<8||accounts[ix.accounts[1]]!==market||accounts[ix.accounts[0]]!==taker)throw Error("Manifest Swap market/payer mismatch");
  // SwapParams must be nonempty; parsing full params is a separate required gate.
  if(data.length<2)throw Error("empty Swap params");
  count++;
 }
 if(count!==1)throw Error("require exactly one authenticated direct Manifest core Swap instruction");
 return {verified:false,opcodeAuthenticated:true,signerHeaderAuthenticated:true,matchingSwaps:count,slot:receipt.slot,warning:"Instruction attributed, but SwapParams, cryptographic signature verification by RPC, custody transitions and fill causation are not yet verified"};
}
