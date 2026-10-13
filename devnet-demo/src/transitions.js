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

export function compareOrderLockSnapshots(before,after,action,side,expectedAmount){
  if(!["place","cancel"].includes(action)||!["ask","bid"].includes(side))
    throw Error("Expected place/cancel and ask/bid");
  if(before.cluster!==after.cluster||before.rpc!==after.rpc||
     before.market!==after.market||before.maker!==after.maker)
    throw Error("Snapshots refer to different maker/market or RPC");
  if(!Number.isSafeInteger(before.slot)||!Number.isSafeInteger(after.slot)||
     after.slot<before.slot)throw Error("Invalid snapshot slot order");
  const a=before.makerState,b=after.makerState;
  if(!a||!b||a.market!==b.market||a.owner!==b.owner)
    throw Error("Maker account is missing or changed identity");
  if(expectedAmount!==undefined){
    const expected=BigInt(expectedAmount);
    if(expected<=0n)throw Error("Expected lock amount must be positive");
  }
  const asset=side==="ask"?"Base":"Quote";
  const other=side==="ask"?"Quote":"Base";
  const free=BigInt(b["free"+asset])-BigInt(a["free"+asset]);
  const locked=BigInt(b["locked"+asset])-BigInt(a["locked"+asset]);
  if(free+locked!==0n)throw Error("Order lock/unlock changed maker asset total");
  if(action==="place"&&(free>=0n||locked<=0n))
    throw Error("Placement did not move free collateral into locked collateral");
  if(action==="cancel"&&(free<=0n||locked>=0n))
    throw Error("Cancellation did not release remaining locked collateral");
  const actual=action==="place"?locked:-locked;
  if(expectedAmount!==undefined&&actual!==BigInt(expectedAmount))
    throw Error("Order collateral delta differs from expected raw amount");
  for(const key of ["free"+other,"locked"+other])
    if(BigInt(a[key])!==BigInt(b[key]))throw Error("Order lock changed opposite asset");
  for(const key of ["base","quote"]){
    if(!before.vaultBalances||!after.vaultBalances||
       BigInt(before.vaultBalances[key])!==BigInt(after.vaultBalances[key]))
      throw Error("Order lock/cancel unexpectedly moved SPL vault tokens");
    const field=key==="base"?"totalBase":"totalQuote";
    if(!before.custodyState||!after.custodyState||
       BigInt(before.custodyState[field])!==BigInt(after.custodyState[field]))
      throw Error("Order lock/cancel unexpectedly changed custody totals");
  }
  return {action,side,amountAtoms:actual.toString(),
    makerAssetConserved:true,vaultsAndCustodyUnchanged:true};
}
