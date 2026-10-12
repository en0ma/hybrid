// Pure collateral accounting transition checks on JSON snapshots.
// These checks do not establish correct fills or token conservation by themselves.
const values=["freeBase","lockedBase","freeQuote","lockedQuote"];
export function compareMakerSnapshots(before,after,action,asset,amount){
  const a=before?.makerState,b=after?.makerState;
  if(!a||!b)throw Error("Both snapshots must have initialized maker balances");
  if(before.market!==after.market||before.maker!==after.maker||
     a.market!==b.market||a.owner!==b.owner)
    throw Error("Snapshot maker/market mismatch");
  const x=Object.fromEntries(values.map(k=>[k,BigInt(a[k])]));
  const y=Object.fromEntries(values.map(k=>[k,BigInt(b[k])]));
  const delta=Object.fromEntries(values.map(k=>[k,(y[k]-x[k]).toString()]));
  if(action==="deposit"||action==="withdraw"){
    if(!["base","quote"].includes(asset))throw Error("Select base or quote");
    const n=BigInt(amount);
    if(n<=0n)throw Error("Amount must be positive");
    const free=asset==="base"?"freeBase":"freeQuote";
    for(const key of values){
      const expected=key===free?(action==="deposit"?n:-n):0n;
      if(y[key]-x[key]!==expected)throw Error(action+" changed unexpected maker collateral "+key);
    }
  }
  return {action,delta,checked:action==="deposit"||action==="withdraw"};
}
