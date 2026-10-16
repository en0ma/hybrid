// Conservation of the four explicitly identified SPL accounts in an isolated
// Manifest core Swap. Does not infer vault ownership from the token account owner.
const numeric=x=>typeof x==="string"&&/^(0|[1-9][0-9]*)$/.test(x);
export function auditManifestVaultFlows({receipt,baseMint,quoteMint,takerBaseIndex,takerQuoteIndex,baseVaultIndex,quoteVaultIndex}){
 const pairs=[[takerBaseIndex,baseMint,"taker base"],[takerQuoteIndex,quoteMint,"taker quote"],[baseVaultIndex,baseMint,"base vault"],[quoteVaultIndex,quoteMint,"quote vault"]];
 const indexes=pairs.map(x=>x[0]);
 if(new Set(indexes).size!==4||indexes.some(i=>!Number.isSafeInteger(i)||i<0))throw Error("four distinct SPL account indices required");
 if(!receipt||receipt.meta?.err!==null||!Array.isArray(receipt.meta.preTokenBalances)||!Array.isArray(receipt.meta.postTokenBalances))throw Error("successful receipt token balances required");
 const lookup=(rows,index,mint)=>{
  const matches=rows.filter(r=>r.accountIndex===index);
  if(matches.length!==1||matches[0].mint!==mint||!numeric(matches[0].uiTokenAmount?.amount))throw Error("missing or invalid "+mint+" token balance for index "+index);
  return matches[0];
 };
 const flows={};
 for(const [index,mint,name] of pairs){
  const a=lookup(receipt.meta.preTokenBalances,index,mint),b=lookup(receipt.meta.postTokenBalances,index,mint);
  if(a.owner!==b.owner||!a.owner||a.programId!==b.programId)throw Error("token account identity changed: "+name);
  flows[name]=BigInt(b.uiTokenAmount.amount)-BigInt(a.uiTokenAmount.amount);
 }
 if(flows["taker base"]<=0n||flows["base vault"]>=0n||flows["taker quote"]>=0n||flows["quote vault"]<=0n)throw Error("unexpected vault/taker trade directions");
 if(flows["taker base"]!==-flows["base vault"]||-flows["taker quote"]!==flows["quote vault"])throw Error("vault/taker token conservation mismatch; fee or extra account paths require separate audit");
 return {verified:false,vaultFlowChecked:true,baseAtomsReceived:flows["taker base"].toString(),quoteAtomsSpent:(-flows["taker quote"]).toString(),limitations:["Does not establish swap opcode, fee pathways, internal maker seat settlement, or a causal book transition","Only valid for isolated, fee-free four-token-account flow"]};
}
