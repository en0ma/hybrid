// Concrete adapter for @solana/web3.js v1. Inject the pinned library from the
// host rather than adding a runtime dependency to the pure ABI package.
import {keyBytes} from "./settlement.mjs";
export function createSolanaSettlementAdapter(web3){
  const {PublicKey,AddressLookupTableAccount,VersionedTransaction,TransactionMessage,
    TransactionInstruction,AddressLookupTableProgram}=web3;
  if(!PublicKey||!AddressLookupTableAccount||!VersionedTransaction||!TransactionMessage||
    !TransactionInstruction||!AddressLookupTableProgram)throw new Error("Complete Solana web3 v1 required");
  const key=v=>new PublicKey(keyBytes(v));
  const lookup=t=>new AddressLookupTableAccount({key:key(t.address),state:t.state});
  function decodeSignedInstructions(bytes,tables=[]){
    const wire=Buffer.from(bytes),tx=VersionedTransaction.deserialize(wire);
    if(!Buffer.from(tx.serialize()).equals(wire)||tx.signatures.some(s=>s.every(x=>x===0)))
      throw new Error("Noncanonical or unsigned transaction");
    const expected=tx.message.addressTableLookups??[];
    if(expected.length!==tables.length||
      expected.some(x=>!tables.some(t=>key(t.address).equals(x.accountKey))))
      throw new Error("Lookup table set mismatch");
    if(new Set(tables.map(t=>key(t.address).toBase58())).size!==tables.length)
      throw new Error("Duplicate lookup table");
    const message=TransactionMessage.decompile(tx.message,{addressLookupTableAccounts:tables.map(lookup)});
    return message.instructions;
  }
  function compileUnsignedV0({payer,recentBlockhash,instruction,lookupTables=[],computeBudgetInstructions=[]}){
    const message=new TransactionMessage({payerKey:key(payer),recentBlockhash,
      instructions:[...computeBudgetInstructions,new TransactionInstruction({
        programId:key(instruction.programId),keys:instruction.keys.map(k=>({...k,pubkey:key(k.pubkey)})),
        data:Buffer.from(instruction.data)})]}).compileToV0Message(lookupTables.map(lookup));
    const tx=new VersionedTransaction(message);
    if(tx.serialize().length>1232)throw new Error("Settlement packet exceeds wire limit");
    return tx;
  }
  function buildLookupCreate({authority,payer,recentSlot}){
    if(!Number.isSafeInteger(recentSlot)||recentSlot<0)throw new Error("Invalid lookup creation slot");
    const [instruction,address]=AddressLookupTableProgram.createLookupTable({
      authority:key(authority),payer:key(payer),recentSlot});
    return {instruction,address};
  }
  function buildLookupExtend({address,authority,payer,addresses}){
    if(!Array.isArray(addresses)||!addresses.length||addresses.length>20||
      new Set(addresses.map(a=>key(a).toBase58())).size!==addresses.length)
      throw new Error("Invalid lookup extension batch");
    return AddressLookupTableProgram.extendLookupTable({lookupTable:key(address),authority:key(authority),
      payer:key(payer),addresses:addresses.map(key)});
  }
  return Object.freeze({
    fromBytes:key,fromBase58:value=>new PublicKey(value),
    findProgramAddressSync:(seeds,program)=>PublicKey.findProgramAddressSync(seeds,key(program)),
    deserializeLookupTable:data=>AddressLookupTableAccount.deserialize(data),
    decodeSignedInstructions,compileUnsignedV0,buildLookupCreate,buildLookupExtend,
  });
}
