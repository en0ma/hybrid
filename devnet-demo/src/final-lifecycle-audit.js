// Final lifecycle conservation, applied only after six confirmed transactions.
// Checks net asset movements across deposit, partial trade, cancel and withdraw.
const bn=x=>BigInt(x);
export function verifyFinalLifecycle({initial,deposit,trade,final,placeAmount,withdrawAmount}){
 if(!initial||!deposit||!trade||!final)throw Error("Final audit requires lifecycle checkpoints");
 const fields=["market","maker","rpc","cluster"];
 for(const f of fields)if([deposit,trade,final].some(s=>s[f]!==initial[f]))
  throw Error("Final audit snapshot identity mismatch: "+f);
 const d=bn(placeAmount),w=bn(withdrawAmount);
 if(d<=w||w<=0n)throw Error("Final audit needs positive deposit larger than withdrawal");
 if(initial.makerState!==null)throw Error("Initial maker must be uninitialized");
 if(!final.makerState||!deposit.makerState||!trade.makerState)
  throw Error("Missing final maker accounting states");
 if(final.bookOrders?.ask?.length!==0||final.bookOrders?.bid?.length!==0||
   final.marketState.askCount!==0||final.marketState.bidCount!==0)
  throw Error("Final order book must be completely empty");
 if(final.makerState.lockedBase!=="0"||final.makerState.lockedQuote!=="0")
  throw Error("Final maker collateral must have no locked remainder");
 const baseOut=bn(trade.takerBalances.base)-bn(initial.takerBalances.base);
 const quoteIn=bn(initial.takerBalances.quote)-bn(trade.takerBalances.quote);
 if(baseOut<=0n||quoteIn<=0n||baseOut>=d)
  throw Error("Final audit requires a positive partial fill");
 const freeBase=d-baseOut-w;
 if(bn(final.makerState.freeBase)!==freeBase||bn(final.makerState.freeQuote)!==quoteIn)
  throw Error("Final free maker collateral does not reconcile to deposit, fill and withdrawal");
 const baseVaultNet=bn(final.vaultBalances.base)-bn(initial.vaultBalances.base);
 const quoteVaultNet=bn(final.vaultBalances.quote)-bn(initial.vaultBalances.quote);
 if(baseVaultNet!==freeBase||quoteVaultNet!==quoteIn)
  throw Error("Final vault delta does not equal remaining maker collateral");
 if(bn(final.custodyState.totalBase)-bn(initial.custodyState.totalBase)!==freeBase||
    bn(final.custodyState.totalQuote)-bn(initial.custodyState.totalQuote)!==quoteIn)
  throw Error("Final custody totals do not reconcile to free maker collateral");
 if(bn(final.takerBalances.base)-bn(initial.takerBalances.base)!==baseOut||
    bn(initial.takerBalances.quote)-bn(final.takerBalances.quote)!==quoteIn)
  throw Error("Non-trade stage changed taker token balances");
 if(bn(final.marketState.nextSequence)!==bn(initial.marketState.nextSequence)+1n)
  throw Error("Final market sequence did not advance exactly once");
 return {verified:true,depositBaseAtoms:d.toString(),withdrawBaseAtoms:w.toString(),
  fillBaseAtoms:baseOut.toString(),fillQuoteAtoms:quoteIn.toString(),
  remainingFreeBaseAtoms:freeBase.toString(),remainingFreeQuoteAtoms:quoteIn.toString(),
  zeroLockedOrders:true,finalCustodyAndVaultsReconciled:true};
}
