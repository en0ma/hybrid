// Pure receipt checks: a successful instruction is not automatically
// evidence that SPL tokens actually changed hands.
export function tokenBalanceDeltas(pre=[],post=[]) {
  const get=(rows)=>new Map(rows.map(x=>[
    x.accountIndex,
    BigInt(x.uiTokenAmount?.amount ?? "0")
  ]));
  const a=get(pre),b=get(post);
  return [...new Set([...a.keys(),...b.keys()])].sort((x,y)=>x-y).map(index=>({
    accountIndex:index,delta:(b.get(index)??0n)-(a.get(index)??0n)
  })).filter(x=>x.delta!==0n);
}
export function verifiesTradeMovement(pre,post) {
  const deltas=tokenBalanceDeltas(pre,post);
  const mintFor=new Map([...pre,...post].map(x=>[x.accountIndex,x.mint]));
  // Settlement must change two distinct assets. Two transfers of only one
  // mint (or a token-amount change with missing mint metadata) are insufficient.
  const changedMints=new Set(deltas.map(x=>mintFor.get(x.accountIndex)));
  if(changedMints.has(undefined)||changedMints.size<2)return false;
  return deltas.some(x=>x.delta>0n)&&deltas.some(x=>x.delta<0n);
}
