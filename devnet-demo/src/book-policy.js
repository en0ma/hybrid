// Pure coverage checks for full-book Surfpool evidence.
export function requireBidSidecar(bidCount,requireFullBook,hasSidecar){
 const count=BigInt(bidCount);
 if(count<0n)throw Error("Invalid bid count");
 if(!hasSidecar&&(count>0n||requireFullBook))
  throw Error("Bid owner sidecar required for active bids or full-book verification; set HYBRID_BID_OWNER0");
 return true;
}
export function verifyFullBook(ownedOrders,requireFullBook){
 const complete=Array.isArray(ownedOrders?.ask)&&Array.isArray(ownedOrders?.bid);
 if(requireFullBook&&!complete)
  throw Error("Full-book snapshot requires both ask and bid ownership evidence");
 return complete;
}
