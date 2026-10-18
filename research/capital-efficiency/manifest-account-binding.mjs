// A finalized Solana receipt's accountIndex is the static message-key index
// followed by loaded writable and loaded readonly keys. Verify that externally
// supplied vault indices actually denote the accounts used by the Swap.
export function bindManifestSwapTokenAccounts({receipt,programId,market,taker,vaultAccounts}){
 const message=receipt?.transaction?.message;
 if(!Array.isArray(message?.accountKeys)||!Array.isArray(message.instructions)||!vaultAccounts)throw Error("raw receipt and explicit vault account bindings required");
 const keys=[...message.accountKeys,...(receipt.meta?.loadedAddresses?.writable??[]),...(receipt.meta?.loadedAddresses?.readonly??[])];
 if(keys.some(x=>typeof x!=="string"||!x))throw Error("raw string account keys required");
 const matches=message.instructions.filter(ix=>keys[ix.programIdIndex]===programId&&keys[ix.accounts?.[1]]===market&&keys[ix.accounts?.[0]]===taker);
 if(matches.length!==1)throw Error("single matching Manifest Swap instruction required");
 const ix=matches[0];
 // Manifest Swap.ts generated account order: payer,market,system,
 // traderBase,traderQuote,baseVault,quoteVault,tokenProgramBase,...
 if(!Array.isArray(ix.accounts)||ix.accounts.length<8)throw Error("incomplete Manifest Swap account list");
 const expected={takerBaseIndex:ix.accounts[3],takerQuoteIndex:ix.accounts[4],baseVaultIndex:ix.accounts[5],quoteVaultIndex:ix.accounts[6]};
 for(const [field,index] of Object.entries(expected)){
  if(!Number.isSafeInteger(index)||index<0||index>=keys.length||index!==vaultAccounts[field])throw Error("Manifest Swap instruction token account does not match "+field);
 }
 if(new Set(Object.values(expected)).size!==4)throw Error("Manifest Swap token accounts must be distinct");
 return {instructionAccountsMatched:true,accountIndices:expected};
}
