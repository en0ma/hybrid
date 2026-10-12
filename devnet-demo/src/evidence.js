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
  // At least one token account is debited, and another credited.
  return deltas.some(x=>x.delta>0n)&&deltas.some(x=>x.delta<0n);
}
