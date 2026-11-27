// Read-only advisory plan. Never a submission permit. Policy counters and
// baseline are supplied by the trusted operator until durable integration.
import {loadLiveMakerSnapshot} from "./maker-live-snapshot.mjs";
import {convertQuoteTargets} from "./quote-orders.mjs";
import {reconcileMakerOrders} from "./maker.mjs";
import {placeOrder,cancelOrder} from "./instructions.mjs";

const Q=1n<<64n,MAX64=Q-1n;
const quoteNotional=o=>(o.baseQty*o.priceX64+Q-1n)/Q;
function atoms(value,name){
  if(typeof value!=="string"||!/^\d+$/.test(value)||BigInt(value)>MAX64)
    throw new Error("Invalid "+name);
  return BigInt(value);
}
function count(value,name){
  if(!Number.isSafeInteger(value)||value<0)throw new Error("Invalid "+name);
  return value;
}
function limitsOf(policy){
  if(!policy||policy.enabled!==true||policy.killSwitch!==false)
    throw new Error("Maker trading disabled by operator");
  if(policy.pendingMutation!==false)throw new Error("Pending mutation or missing journal state");
  return {
    maxLagSlots:count(policy.maxLagSlots,"maxLagSlots"),
    maxOrders:count(policy.maxOrders,"maxOrders"),
    maxChanges:count(policy.maxChanges,"maxChanges"),
    maxBaseExposure:atoms(policy.maxBaseExposure,"maxBaseExposure"),
    maxQuoteNotional:atoms(policy.maxQuoteNotional,"maxQuoteNotional"),
    maxLossQuote:atoms(policy.maxLossQuote,"maxLossQuote"),
    baselineEquityQuote:atoms(policy.baselineEquityQuote,"baselineEquityQuote"),
    maxOrdersPerWindow:count(policy.maxOrdersPerWindow,"maxOrdersPerWindow"),
    maxTransactionsPerWindow:count(policy.maxTransactionsPerWindow,"maxTransactionsPerWindow"),
    maxRetries:count(policy.maxRetries,"maxRetries"),
    ordersInWindow:count(policy.ordersInWindow,"ordersInWindow"),
    transactionsInWindow:count(policy.transactionsInWindow,"transactionsInWindow"),
    retries:count(policy.retries,"retries"),
  };
}
export async function planLiveMakerQuotes(connection,{snapshotOptions,orders,policy}){
  const limits=limitsOf(policy);
  const snapshot=await loadLiveMakerSnapshot(connection,{
    ...snapshotOptions,maxLagSlots:limits.maxLagSlots,
  });
  if(snapshot.pendingLazySettlement)
    throw new Error("Pending lazy claims: reconcile maker-specific inventory before mutation");
  // Convert the whole desired set before matching it, then check only new
  // placements against free funds. Locked collateral never funds a new order.
  const converted=convertQuoteTargets({orders,baseLotSizeAtoms:String(snapshot.header.baseLotSize),
    freeBaseAtoms:String(MAX64),freeQuoteAtoms:String(MAX64),maxOrders:limits.maxOrders});
  const existing=snapshot.ownedOrders;
  const plan=reconcileMakerOrders({existing,desired:converted.desired,maxChanges:limits.maxChanges});
  const inventory=snapshot.inventory;
  const totalBase=inventory.freeBase+inventory.lockedBase;
  const totalQuote=inventory.freeQuote+inventory.lockedQuote;
  const equity=totalQuote+(totalBase*snapshot.header.sqrtPriceX64**2n)/(Q*Q);
  const loss=limits.baselineEquityQuote>equity?limits.baselineEquityQuote-equity:0n;
  if(loss>limits.maxLossQuote)throw new Error("Marked inventory loss limit exceeded");
  // Check both current and desired exposure; cancellation-first is not
  // permission to exceed a limit between stages.
  for(const set of [existing,converted.desired]){
    const buyQty=set.filter(o=>o.side==="bid").reduce((a,o)=>a+o.baseQty,0n);
    const gross=set.reduce((a,o)=>a+quoteNotional(o),0n);
    if(totalBase+buyQty>limits.maxBaseExposure)throw new Error("Base exposure limit exceeded");
    if(gross>limits.maxQuoteNotional)throw new Error("Quote notional limit exceeded");
  }
  const phase=plan.cancellations.length?"cancel":plan.placements.length?"place":"noop";
  const actions=phase==="cancel"?plan.cancellations.map(o=>{
    const current=existing.find(x=>x.sequence===o.sequence);
    if(!current||current.pageIndex!==0)throw new Error("Unsupported multi-page cancellation");
    return {...o,pageIndex:current.pageIndex};
  }):phase==="place"?plan.placements:[];
  if(phase==="place"){
    const base=actions.filter(o=>o.side==="ask").reduce((a,o)=>a+o.baseQty,0n);
    const quote=actions.filter(o=>o.side==="bid").reduce((a,o)=>a+quoteNotional(o),0n);
    if(base>inventory.freeBase||quote>inventory.freeQuote)throw new Error("Insufficient free collateral");
    for(const side of ["ask","bid"]){
      const pageOrders=(side==="ask"?snapshot.asks:snapshot.bids).filter(o=>o.pageIndex===0);
      const newCount=actions.filter(o=>o.side===side).length;
      const pages=side==="ask"?snapshot.askPages:snapshot.bidPages;
      if(newCount&&(pages.length!==1||pageOrders.length+newCount>32))
        throw new Error("Unsupported placement page capacity or linked book");
    }
  }
  // Conservatively reserve one transaction per action, not an assumed batch.
  if(actions.length&&(limits.ordersInWindow+actions.length>limits.maxOrdersPerWindow||
    limits.transactionsInWindow+actions.length>limits.maxTransactionsPerWindow||
    limits.retries>limits.maxRetries))throw new Error("Order, transaction or retry rate limit exceeded");
  // Only the first action is eligible for a single-intent reservation.
  // A fresh snapshot and policy evaluation is required for every later action.
  const first=actions[0];
  let firstInstruction=null;
  if(first){
    const pages=first.side==="ask"?snapshot.askPages:snapshot.bidPages;
    if(pages.length!==1)throw new Error("Unsupported linked book mutation");
    const roles={market:snapshot.market,maker:snapshot.maker,makerBalance:snapshot.makerBalance,
      page:pages[0].page,ownerPage:pages[0].ownerPage};
    firstInstruction=(phase==="cancel"?cancelOrder:placeOrder)(
      snapshotOptions.programId,roles,first);
  }
  return {mode:"advisory-only",canSubmit:false,phase,actions,firstInstruction,
    snapshotSlot:snapshot.slot,observedSlot:snapshot.observedSlot,
    markedEquityQuote:equity,markedLossQuote:loss,
    requiresFreshSnapshot:phase==="cancel",requiresDurableJournalAndFreshSimulation:true,
    desired:converted.desired};
}
