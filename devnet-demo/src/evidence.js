// Pure receipt checks. These prove bounded SPL movement in a transaction,
// not the full correctness of the order-book state transition.
export function tokenBalanceDeltas(pre=[],post=[]) {
  const get=(rows)=>new Map(rows.map(x=>[
    x.accountIndex,BigInt(x.uiTokenAmount?.amount ?? "0")
  ]));
  const a=get(pre),b=get(post);
  return [...new Set([...a.keys(),...b.keys()])].sort((x,y)=>x-y)
    .map(index=>({accountIndex:index,delta:(b.get(index)??0n)-(a.get(index)??0n)}))
    .filter(x=>x.delta!==0n);
}
export function verifiesTradeMovement(pre=[],post=[]) {
  const a=new Map(pre.map(x=>[x.accountIndex,x]));
  const b=new Map(post.map(x=>[x.accountIndex,x]));
  if(a.size!==pre.length||b.size!==post.length)return false;
  // Comparing mismatched account indices as zero can fabricate transfers.
  if(a.size!==b.size||[...a.keys()].some(index=>!b.has(index)))return false;
  const byMint=new Map();
  for(const [index,before] of a){
    const after=b.get(index);
    if(!before.mint||before.mint!==after.mint||!before.owner||before.owner!==after.owner)return false;
    if(!/^[0-9]+$/.test(before.uiTokenAmount?.amount??"")||
       !/^[0-9]+$/.test(after.uiTokenAmount?.amount??""))return false;
    const delta=BigInt(after.uiTokenAmount.amount)-BigInt(before.uiTokenAmount.amount);
    if(!byMint.has(before.mint))byMint.set(before.mint,[]);
    byMint.get(before.mint).push(delta);
  }
  // Each distinct mint must show both debit and credit in the same receipt.
  // A single-sided mint change could be a mint/burn or unrelated transfer.
  const traded=[...byMint.values()].filter(d=>d.some(x=>x<0n)&&d.some(x=>x>0n));
  return traded.length>=2;
}
