// Pure, exact integer-only conversions from quote planner targets to
// canonical Hybrid active-order fields. Does not derive PDAs, simulate,
// sign, submit, or mutate accounts. All live execution remains disabled.
const Q64=1n<<64n, MAX128=(1n<<128n)-1n, MAX64=(1n<<64n)-1n;
const DECIMAL=/^(?:0|[1-9][0-9]*)(?:\.[0-9]{1,18})?$/;
function positiveAtom(value,name){
  if(typeof value!=="string"||!/^[0-9]+$/.test(value))throw new Error("Invalid "+name);
  const n=BigInt(value);
  if(n===0n||n>MAX64)throw new RangeError(name+" outside positive u64");
  return n;
}
export function decimalToPriceX64(decimal,side){
  if(side!=="bid"&&side!=="ask")throw new RangeError("Invalid side");
  if(typeof decimal!=="string"||!DECIMAL.test(decimal))throw new Error("Invalid decimal price");
  const [whole,fraction=""]=decimal.split(".");
  const denominator=10n**BigInt(fraction.length);
  const numerator=BigInt(whole)*denominator+BigInt(fraction||"0");
  if(numerator===0n)throw new RangeError("Zero order price");
  const product=numerator*Q64;
  // A bid is never rounded UP above its requested price. An ask is never
  // rounded DOWN below its requested price.
  const value=side==="bid" ? product/denominator :
    (product+denominator-1n)/denominator;
  if(value===0n||value>MAX128)throw new RangeError("Q64 price out of range");
  return value;
}
function isqrt(n){
  if(n<0n)throw new RangeError("Negative square root");
  if(n<2n)return n;
  let x=1n<<BigInt(Math.ceil(n.toString(2).length/2));
  while(true){const y=(x+n/x)>>1n;if(y>=x)return x;x=y;}
}
export function canonicalSqrtPriceX64(priceX64){
  if(typeof priceX64!=="bigint"||priceX64===0n||priceX64>MAX128)
    throw new RangeError("Invalid Q64 price");
  // Canonical cache: floor(s*s/Q64) <= price
  // and floor((s+1)*(s+1)/Q64) > price.
  const root=isqrt((priceX64+1n)*Q64-1n);
  if(root===0n||root>MAX128 || ((root+1n)*(root+1n))/Q64>MAX128)
    throw new RangeError("Invalid cached sqrt");
  return root;
}
function nonnegativeAtoms(value,name){
  if(typeof value!=="string"||!/^[0-9]+$/.test(value))throw new Error("Invalid "+name);
  const n=BigInt(value);
  if(n>MAX64)throw new RangeError(name+" outside u64");
  return n;
}
/**
 * Convert one reference strategy plan to canonical order targets.
 * This is preflight-only: it conservatively checks new-order collateral
 * against authenticated FREE balances and cannot use existing locked funds.
 * Caller must prove price/lot rules, freshness, PDA identities, page capacity,
 * simulation and durable submission state before creating transactions.
 */
export function convertQuoteTargets({
  orders,baseLotSizeAtoms,freeBaseAtoms,freeQuoteAtoms,maxOrders=16,
}){
  if(!Array.isArray(orders)||!Number.isSafeInteger(maxOrders)||maxOrders<0
      ||orders.length>maxOrders)throw new RangeError("Invalid target count");
  const lot=positiveAtom(baseLotSizeAtoms,"baseLotSizeAtoms");
  const freeBase=nonnegativeAtoms(freeBaseAtoms,"freeBaseAtoms");
  const freeQuote=nonnegativeAtoms(freeQuoteAtoms,"freeQuoteAtoms");
  let reservedBase=0n,reservedQuote=0n;
  const desired=[];
  for(const order of orders){
    if(!order||!["bid","ask"].includes(order.side))throw new RangeError("Invalid order side");
    const qty=positiveAtom(order.sizeAtoms,"sizeAtoms");
    if(qty%lot!==0n)throw new RangeError("Order quantity must be a whole base lot");
    const priceX64=decimalToPriceX64(order.priceAtoms,order.side);
    const sqrtPriceX64=canonicalSqrtPriceX64(priceX64);
    // Ceil collateral requirement in quote atoms, not floating-point.
    if(order.side==="ask"){
      reservedBase+=qty;
      if(reservedBase>freeBase)throw new RangeError("Insufficient free base collateral");
    } else {
      reservedQuote+=(priceX64*qty+Q64-1n)/Q64;
      if(reservedQuote>freeQuote)throw new RangeError("Insufficient free quote collateral");
    }
    desired.push({side:order.side,priceX64,sqrtPriceX64,baseQty:qty});
  }
  return {
    desired,reservedBaseAtoms:reservedBase,reservedQuoteAtoms:reservedQuote,
    mode:"preflight-only",
  };
}
