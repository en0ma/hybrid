// Integer-only mirror of bounded execution; no submission capability.
import {unsigned} from "./instructions.mjs";
import {keyBytes,buildSettlementExecution} from "./settlement.mjs";
import {canonicalSqrtPriceX64} from "./quote-orders.mjs";
const Q=1n<<64n,MAX=(1n<<64n)-1n;
const checked=n=>{if(n<0n||n>MAX)throw new RangeError("u64 overflow");return n;};
const quote=(n,p)=>checked((n*p+Q-1n)/Q);
export function planSettlement(snapshot,{baseFill,quoteLimit}){
  const {side,pages,orders}=snapshot;
  if(!["ask","bid"].includes(side)||![1,2].includes(pages?.length)||
    !Array.isArray(orders)||!orders.length||orders.length>(side==="bid"?16:8))
    throw new Error("Unsupported settlement book");
  const bid=side==="bid",fill=unsigned(baseFill,64,"baseFill"),limit=unsigned(quoteLimit,64,"quoteLimit");
  if(!fill||!limit)throw new Error("Zero fill or limit");
  const seen=new Set();
  for(const [i,o] of orders.entries()){
    const maker=keyBytes(o.owner).toString("hex");
    if(seen.has(maker)||!o.baseQty||o.baseQty>MAX||
      canonicalSqrtPriceX64(o.priceX64)!==o.sqrtPriceX64)throw new Error("Invalid roster");
    seen.add(maker);
    const prev=orders[i-1];
    if(prev&&((bid?prev.priceX64<o.priceX64:prev.priceX64>o.priceX64)||
      (prev.priceX64===o.priceX64&&prev.sequence>=o.sequence)))throw new Error("Unsorted roster");
  }
  let remaining=fill,quoteTotal=0n;const ticks=[];
  for(let i=0;i<orders.length&&remaining;){
    const start=i,p=orders[i].priceX64,sqrt=orders[i].sqrtPriceX64;
    while(i<orders.length&&orders[i].priceX64===p)i++;
    const rows=orders.slice(start,i),capacity=checked(rows.reduce((n,o)=>n+o.baseQty,0n));
    const f=remaining<capacity?remaining:capacity,q=quote(f,p),fee=(bid?f:q)/100n;
    let w=0n,prev=0n;
    const allocations=rows.map(o=>{
      w+=o.baseQty;const next=f*w/capacity,filled=next-prev;
      const spent=q*next/f-q*prev/f,charge=fee*next/f-fee*prev/f;
      prev=next;
      if(filled&&(!spent||(bid&&filled===charge)))throw new Error("Unclaimable allocation");
      return {filled,spent,charge,cap:bid?quote(o.baseQty,p)-quote(o.baseQty-filled,p):0n};
    });
    if(bid){
      let spill=0n;
      for(const a of allocations)if(a.spent>a.cap){spill+=a.spent-a.cap;a.spent=a.cap;}
      for(const a of allocations){const room=a.cap-a.spent,extra=room<spill?room:spill;a.spent+=extra;spill-=extra;}
      if(spill||allocations.some(a=>a.filled&&!a.spent))throw new Error("Insufficient rounding collateral");
    }
    const makers=rows.map((o,j)=>{
      const a=allocations[j];
      return {makerHex:keyBytes(o.owner).toString("hex"),weight:String(o.baseQty),
        amounts:(bid?[0n,a.filled-a.charge,a.spent,0n,a.charge,0n]:
          [a.filled,0n,0n,a.spent,0n,a.charge]).map(String)};
    });
    const totals=bid?[0n,f-fee,q,0n,fee,0n]:[f,0n,0n,q,0n,fee];
    ticks.push({priceX64:p,tickSqrtPriceX64:sqrt,filledBase:f,totals,makers});
    quoteTotal=checked(quoteTotal+q+(bid?0n:fee));remaining-=f;
  }
  if(remaining||ticks.length!==pages.length||
    (pages.length===1&&orders.some(o=>o.priceX64!==ticks[0].priceX64))||
    (bid?quoteTotal<limit:quoteTotal>limit))throw new Error("Unsupported fill or slippage");
  return {side,baseFill:fill,quoteLimit:limit,quoteTotal,ticks};
}
export function prepareSettlement(programId,snapshot,accounts,request){
  const plan=planSettlement(snapshot,request);
  const instruction=buildSettlementExecution(programId,{...accounts,market:snapshot.market,
    reserve:snapshot.reserve,pages:snapshot.pages,epochs:snapshot.epochs,
    makerBalances:snapshot.side==="ask"&&snapshot.pages.length===1?[]:snapshot.makerBalances},
    {side:snapshot.side,baseFill:plan.baseFill,quoteLimit:plan.quoteLimit});
  return {mode:"unsigned-only",slot:snapshot.slot,plan,instruction};
}
