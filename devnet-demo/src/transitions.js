// Pure collateral accounting transition checks on JSON snapshots.
// These checks do not establish correct fills or token conservation by themselves.
import {verifyOwnedOrderTransition} from "./book.js";
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

export function quoteForBaseAtPrice(baseQty,priceX64){
 const base=BigInt(baseQty),price=BigInt(priceX64),q64=1n<<64n;
 if(base<=0n||base>=(1n<<64n)||price<=0n||price>=(1n<<128n))throw Error("Invalid bid quantity/price");
 const quote=(base*price+q64-1n)/q64;
 if(quote<=0n||quote>=(1n<<64n))throw Error("Bid quote obligation out of range");
 return quote;
}

export function compareOrderLockSnapshots(before,after,action,side,expectedAmount,expectedSequence,expectedPageIndex){
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
  // Header checks are aggregate bookkeeping, not per-order identity proof.
  if(!before.marketState||!after.marketState)
    throw Error("Market header snapshots required for order transitions");
  const countField=side==="ask"?"askCount":"bidCount";
  const beforeCount=BigInt(before.marketState[countField]);
  const afterCount=BigInt(after.marketState[countField]);
  const beforeSeq=BigInt(before.marketState.nextSequence);
  const afterSeq=BigInt(after.marketState.nextSequence);
  if(expectedSequence!==undefined){
    const seq=BigInt(expectedSequence);
    if(seq<=0n)throw Error("Expected order sequence must be positive");
    if(action==="place"&&seq!==beforeSeq)
      throw Error("Placed order sequence must equal pre-placement nextSequence");
    if(action==="cancel"&&seq>=beforeSeq)
      throw Error("Canceled sequence must precede market nextSequence");
  }
  const countDelta=action==="place"?1n:-1n;
  if(afterCount-beforeCount!==countDelta)
    throw Error("Market order count delta inconsistent with placement/cancellation");
  if(afterSeq-beforeSeq!==(action==="place"?1n:0n))
    throw Error("Market next sequence delta inconsistent with placement/cancellation");
  const oppositeCount=side==="ask"?"bidCount":"askCount";
  if(BigInt(before.marketState[oppositeCount])!==BigInt(after.marketState[oppositeCount]))
    throw Error("Opposite-side market order count changed");
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
  if(before.bookOrders||after.bookOrders){
    for(const snapshot of [before,after]){
      if(!snapshot.bookOrders||!Number.isSafeInteger(snapshot.bookSlot)||snapshot.bookSlot!==snapshot.slot)
        throw Error("Book/collateral snapshot must use a single RPC context slot");
    }
  }
  let orderIdentityChecked=false;
  if(expectedSequence!==undefined&&(before.bookOrders||after.bookOrders)&&
     (!Array.isArray(before.bookOrders?.[side])||!Array.isArray(after.bookOrders?.[side])))
    throw Error("Selected-side book evidence missing; order identity cannot be verified");
  if(expectedSequence!==undefined&&before.bookOrders?.[side]&&after.bookOrders?.[side]){
    const detail=verifyOwnedOrderTransition(before.bookOrders[side],after.bookOrders[side],
      action,expectedSequence,before.maker,expectedPageIndex===undefined?
       Number(after.bookOrders[side].find(x=>x.sequence===String(expectedSequence))?.pageIndex ?? before.bookOrders[side].find(x=>x.sequence===String(expectedSequence))?.pageIndex):expectedPageIndex);
    const required=side==="ask"?BigInt(detail.order.baseQty):quoteForBaseAtPrice(detail.order.baseQty,detail.order.priceX64);
    if(required!==actual)throw Error("Order quantity/price does not match exact collateral movement");
    orderIdentityChecked=true;
  }
  return {action,side,amountAtoms:actual.toString(),
    makerAssetConserved:true,vaultsAndCustodyUnchanged:true,marketHeaderTransitionsChecked:true,sequenceExpectationChecked:expectedSequence!==undefined,orderIdentityChecked};
}
