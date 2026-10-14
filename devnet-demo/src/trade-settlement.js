// Exact-atom conservation checks for an ask-side funded taker fill.
const n=x=>BigInt(x);
export function verifyAskTradeSettlement(before,after){
 if(before.market!==after.market||before.maker!==after.maker||
   before.cluster!==after.cluster||before.rpc!==after.rpc)
  throw Error("Trade snapshots differ in market, maker or RPC");
 const token=(s,k)=>n(s.takerBalances?.[k]);
 const vault=(s,k)=>n(s.vaultBalances[k]);
 const custody=(s,k)=>n(s.custodyState[k]);
 const maker=(s,k)=>n(s.makerState[k]);
 if(!before.takerBalances||!after.takerBalances||!before.makerState||!after.makerState)
  throw Error("Trade settlement requires maker and taker snapshots");
 const baseOut=token(after,"base")-token(before,"base");
 const quoteIn=token(before,"quote")-token(after,"quote");
 if(baseOut<=0n||quoteIn<=0n)throw Error("Trade did not move positive base and quote");
 if(vault(before,"base")-vault(after,"base")!==baseOut||
    vault(after,"quote")-vault(before,"quote")!==quoteIn)
  throw Error("Trade taker and vault atom deltas do not conserve");
 if(custody(before,"totalBase")-custody(after,"totalBase")!==baseOut||
    custody(after,"totalQuote")-custody(before,"totalQuote")!==quoteIn)
  throw Error("Trade custody totals do not match vault changes");
 if(maker(before,"lockedBase")-maker(after,"lockedBase")!==baseOut||
    maker(after,"freeQuote")-maker(before,"freeQuote")!==quoteIn)
  throw Error("Trade maker settlement differs from taker fill");
 if(maker(before,"freeBase")!==maker(after,"freeBase")||
    maker(before,"lockedQuote")!==maker(after,"lockedQuote"))
  throw Error("Trade changed unrelated maker collateral");
 if(n(before.marketState.nextSequence)!==n(after.marketState.nextSequence))
  throw Error("Trade unexpectedly allocated a new order sequence");
 if(n(after.marketState.askCount)>n(before.marketState.askCount))
  throw Error("Trade unexpectedly increased the ask order count");
 return {verified:true,baseOutAtoms:baseOut.toString(),quoteInAtoms:quoteIn.toString(),
  takerVaultConserved:true,custodyReconciled:true,makerSettlementReconciled:true};
}
