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
 // A partially filled one-maker ask must remain in the same canonical slot;
 // otherwise the next cancellation cannot safely target its original sequence.
 if(before.bookOrders||after.bookOrders){
  const beforeAsk=before.bookOrders?.ask,afterAsk=after.bookOrders?.ask;
  const beforeBid=before.bookOrders?.bid,afterBid=after.bookOrders?.bid;
  if(!Array.isArray(beforeAsk)||!Array.isArray(afterAsk)||
     !Array.isArray(beforeBid)||!Array.isArray(afterBid))
   throw Error("Trade requires complete ask and bid book snapshots");
  if(beforeAsk.length!==1||afterAsk.length!==1||
     beforeBid.length!==0||afterBid.length!==0)
   throw Error("Fixture trade must partially fill exactly one ask and no bids");
  const a=beforeAsk[0],b=afterAsk[0];
  if(a.sequence!==b.sequence||a.owner!==b.owner||a.owner!==before.maker||
     a.pageIndex!==0||b.pageIndex!==0||a.priceX64!==b.priceX64||
     a.sqrtPriceX64!==b.sqrtPriceX64)
   throw Error("Trade altered the placed order identity or price");
  if(n(a.baseQty)-n(b.baseQty)!==baseOut)
   throw Error("Trade book quantity change differs from actual taker base fill");
  if(n(after.marketState.askCount)!==1n||n(before.marketState.askCount)!==1n)
   throw Error("Fixture trade unexpectedly removed the partially filled ask");
 }

 return {verified:true,baseOutAtoms:baseOut.toString(),quoteInAtoms:quoteIn.toString(),
  takerVaultConserved:true,custodyReconciled:true,makerSettlementReconciled:true,partialBookFillVerified:!!before.bookOrders};
}
