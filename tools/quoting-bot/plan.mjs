// Deterministic, non-signing reference strategy.
// Prices are quote-token atoms per base-token atom, represented as exact
// decimal strings. No floating-point arithmetic or rounding of prices.
function parseUnsignedAtoms(value, name) {
  if (typeof value !== "string" && typeof value !== "bigint" && typeof value !== "number") throw Error("invalid " + name);
  const text = String(value);
  if (!/^[0-9]+$/.test(text)) throw Error("invalid " + name);
  return BigInt(text);
}
function parsePrice(value) {
  const text = String(value);
  if (!/^(?:0|[1-9][0-9]*)(?:\.[0-9]{1,18})?$/.test(text)) throw Error("invalid midPriceAtoms");
  const [whole, fraction = ""] = text.split(".");
  const scale = 10n ** BigInt(fraction.length);
  const numerator = BigInt(whole + fraction);
  if (numerator <= 0n) throw Error("invalid inventory");
  return {numerator, scale};
}
function decimalPrice(numerator, denominator) {
  // Denominator is 10^(input decimals) * 10_000, so the decimal is exact.
  const decimals = denominator.toString().length - 1;
  const padded = numerator.toString().padStart(decimals + 1, "0");
  if (!decimals) return padded;
  const head = padded.slice(0, -decimals);
  const tail = padded.slice(-decimals).replace(/0+$/, "");
  return tail ? head + "." + tail : head;
}
function ceilDiv(numerator, denominator) {
  return (numerator + denominator - 1n) / denominator;
}

export function planQuotes({midPriceAtoms,spreadBps,levels=1,stepBps=25,sizeAtoms,maxBaseAtoms,maxQuoteAtoms,stale=false}) {
  const number=(n,name)=>{if(!Number.isInteger(n)||n<0)throw Error("invalid "+name)};
  for(const [name,v] of Object.entries({spreadBps,levels,stepBps}))number(v,name);
  if(spreadBps<1||spreadBps>=10000||levels<1||levels>8||stepBps>10000)throw Error("strategy limits violated");
  if(stale)throw Error("refuse stale market snapshot");

  const mid=parsePrice(midPriceAtoms);
  const size=parseUnsignedAtoms(sizeAtoms,"sizeAtoms");
  const base=parseUnsignedAtoms(maxBaseAtoms,"maxBaseAtoms");
  const quote=parseUnsignedAtoms(maxQuoteAtoms,"maxQuoteAtoms");
  if(size<=0n)throw Error("invalid inventory");

  let usedBase=0n,usedQuote=0n;
  const orders=[];
  const denominator=mid.scale*10000n;
  for(let i=0;i<levels;i++){
    const offset=BigInt(spreadBps)+BigInt(i)*BigInt(stepBps);
    if(offset>=10000n)break;
    const bidNumerator=mid.numerator*(10000n-offset);
    const askNumerator=mid.numerator*(10000n+offset);
    // Round collateral UP: fractional quote atoms cannot be deposited.
    const bidQuoteCost=ceilDiv(bidNumerator*size,denominator);
    if(bidNumerator>0n && usedQuote+bidQuoteCost<=quote){
      orders.push({side:"bid",level:i,priceAtoms:decimalPrice(bidNumerator,denominator),sizeAtoms:size.toString()});
      usedQuote+=bidQuoteCost;
    }
    if(usedBase+size<=base){
      orders.push({side:"ask",level:i,priceAtoms:decimalPrice(askNumerator,denominator),sizeAtoms:size.toString()});
      usedBase+=size;
    }
  }
  return {orders,reservedBaseAtoms:usedBase.toString(),reservedQuoteAtoms:usedQuote.toString(),requiresFreshSnapshotAndSimulation:true};
}
