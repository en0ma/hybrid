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
 if(!Array.isArray(before?.asks)||!Array.isArray(after?.asks)||before.market!==after.market||before.market===undefined||!Number.isSafeInteger(before.slot)||!Number.isSafeInteger(after.slot)||before.slot>=receipt.slot||after.slot<receipt.slot)throw Error("invalid chronological market snapshots");
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
 const orderKey=o=>String(o.trader)+":"+String(o.sequence);
 const inspect=(asks,label)=>{
  const entries=new Map();
  for(const order of asks){
   if(!order?.trader||!numberString(String(order.baseAtoms))||!numberString(String(order.sequence))||!numberString(String(order.priceRaw))||BigInt(order.baseAtoms)<=0n)throw Error("malformed "+label+" order state");
   const id=orderKey(order);if(entries.has(id))throw Error("duplicate "+label+" order identity");
   entries.set(id,order);
  }
  return entries;
 };
 const oldOrders=inspect(before.asks,"before"),newOrders=inspect(after.asks,"after");
 let filled=0n,changed=0;
 for(const [id,old] of oldOrders){
  const next=newOrders.get(id);
  if(!next){if(old.trader!==makerOwner)throw Error("unrelated order removed during fill audit");throw Error("maker order removed: cannot distinguish fill from cancel without trade proof")}
  if(String(old.priceRaw)!==String(next.priceRaw)||old.trader!==next.trader)throw Error("order price or ownership changed");
  const delta=BigInt(old.baseAtoms)-BigInt(next.baseAtoms);
  if(delta<0n)throw Error("order quantity increased during fill");
  if(delta>0n){if(old.trader!==makerOwner)throw Error("unrelated maker order quantity changed");filled+=delta;changed++;}
 }
 for(const id of newOrders.keys())if(!oldOrders.has(id))throw Error("new order appeared during fill audit");
 if(changed!==1)throw Error("require exactly one identified maker ask partially filled");
 if(filled<=0n)throw Error("maker ask liquidity did not decrease; no fill established");
 const touchedOwners=new Set([...pre.values(),...post.values()].filter(x=>x.mint===baseMint||x.mint===quoteMint).map(x=>x.owner));
 if(!touchedOwners.has(makerOwner)||!touchedOwners.has(takerOwner))throw Error("maker/taker token accounts not both present in receipt");
 return {verified:false,consistencyChecked:true,market:before.market,slot:receipt.slot,filledBaseAtoms:filled.toString(),baseNetAtoms:base.toString(),quoteNetAtoms:quote.toString(),limitations:["Order-state partial-fill consistency is not proof of trade causation or execution price","Additional custody and market seat balance audits required"]};
}
