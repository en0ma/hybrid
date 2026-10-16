// Read-only independent SPL token delta audit for a finalized Manifest fill.
// Must be combined with program/market authenticated receipt and market snapshots.
const numberString=x=>typeof x==="string"&&/^(0|[1-9][0-9]*)$/.test(x);
function toMap(rows,label){
 if(!Array.isArray(rows))throw Error("missing "+label+" token balances");
 const m=new Map();
 for(const r of rows){
  const idx=r.accountIndex,mint=r.mint,owner=r.owner,amount=r.uiTokenAmount?.amount;
  if(!Number.isSafeInteger(idx)||idx<0||typeof mint!=="string"||!mint||typeof owner!=="string"||!owner||!numberString(amount))throw Error("malformed "+label+" SPL balance");
  const k=idx+":"+mint;
  if(m.has(k))throw Error("duplicate SPL balance "+k);
  m.set(k,{index:idx,mint,owner,amount:BigInt(amount)});
 }
 return m;
}
export function auditManifestFill({before,after,baseMint,quoteMint,makerOwner,takerOwner,receipt}){
 if(!baseMint||!quoteMint||baseMint===quoteMint||!makerOwner||!takerOwner||makerOwner===takerOwner)throw Error("distinct mints and traders required");
 if(!receipt||receipt.meta?.err!==null||!Number.isSafeInteger(receipt.slot)||receipt.slot<=0)throw Error("finalized successful receipt required");
 if(!Array.isArray(before?.asks)||!Array.isArray(after?.asks)||before.market!==after.market||before.market===undefined||after.slot<before.slot)throw Error("invalid chronological market snapshots");
 const pre=toMap(receipt.meta.preTokenBalances,"pre"),post=toMap(receipt.meta.postTokenBalances,"post");
 const all=new Set([...pre.keys(),...post.keys()]);let base=0n,quote=0n;const byMint=new Map();
 for(const key of all){
  const a=pre.get(key),b=post.get(key);
  if(!a||!b||a.owner!==b.owner||a.mint!==b.mint)throw Error("cannot audit created/closed/reassigned token account without explicit account-state proof");
  const delta=b.amount-a.amount;
  byMint.set(a.mint,(byMint.get(a.mint)??0n)+delta);
  if(a.mint===baseMint)base+=delta;
  if(a.mint===quoteMint)quote+=delta;
 }
 if(base!==0n||quote!==0n)throw Error("SPL token conservation failed: missing or inconsistent token balances");
 const makerBefore=before.asks.filter(x=>x.trader===makerOwner).reduce((sum,x)=>sum+BigInt(x.baseAtoms),0n);
 const makerAfter=after.asks.filter(x=>x.trader===makerOwner).reduce((sum,x)=>sum+BigInt(x.baseAtoms),0n);
 const filled=makerBefore-makerAfter;
 if(filled<=0n)throw Error("maker ask liquidity did not decrease; no fill established");
 const touchedOwners=new Set([...pre.values(),...post.values()].filter(x=>x.mint===baseMint||x.mint===quoteMint).map(x=>x.owner));
 if(!touchedOwners.has(makerOwner)||!touchedOwners.has(takerOwner))throw Error("maker/taker token accounts not both present in receipt");
 return {verified:true,market:before.market,slot:receipt.slot,filledBaseAtoms:filled.toString(),baseNetAtoms:base.toString(),quoteNetAtoms:quote.toString(),limitations:["Token conservation and ask reduction alone do not establish price execution, trade attribution or global collateral solvency","Additional custody and market seat balance audits required"]};
}
