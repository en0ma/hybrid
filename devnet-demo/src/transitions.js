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
  if(before.cluster!==after.cluster||before.rpc!==after.rpc)throw Error("Snapshot network/RPC mismatch");
  if(!Number.isSafeInteger(before.slot)||!Number.isSafeInteger(after.slot)||after.slot<before.slot)
    throw Error("Snapshot slots invalid or out of order");
  if(!before.custodyState||!after.custodyState)throw Error("Custody snapshots required");
  const vault=asset==="base"?"base":"quote";
  if(!before.vaultBalances||!after.vaultBalances)throw Error("Vault balance snapshots required");
  const n=BigInt(amount),direction=action==="deposit"?1n:-1n;
  for(const key of ["base","quote"]){
    const beforeVault=BigInt(before.vaultBalances[key]);
    const afterVault=BigInt(after.vaultBalances[key]);
    const expected=key===vault?direction*n:0n;
    if(afterVault-beforeVault!==expected)throw Error("SPL vault transfer does not match maker collateral delta");
  }
  for(const key of ["base","quote"]){
    const field=key==="base"?"totalBase":"totalQuote";
    const expected=key===vault?direction*n:0n;
    if(BigInt(after.custodyState[field])-BigInt(before.custodyState[field])!==expected)
      throw Error("Custody total delta differs from maker and vault movement");
  }
  return {action,delta,checked:true,verifiedVaultTransfers:true,verifiedCustodyTotals:true};
}
