// Fail-closed, read-only comparison of two authenticated maker snapshots.
// The expected deltas must be derived independently from finalized on-chain
// receipts, custody ledger and authorized deposits/withdrawals. This helper
// does NOT verify that provenance and NEVER grants execution permission.
const U64=(1n<<64n)-1n;
const FIELDS=["freeBase","lockedBase","freeQuote","lockedQuote"];
const decimal=x=>typeof x==="string"&&/^(0|[1-9][0-9]*)$/.test(x);
function amount(x,name){
  if(typeof x!=="bigint"||x<0n||x>U64)
    throw new Error("Invalid authenticated "+name);
  return x;
}
function signedDelta(value,name){
  if(typeof value!=="string"||!/^(0|-[1-9][0-9]*|[1-9][0-9]*)$/.test(value))
    throw new Error("Expected signed integer delta for "+name);
  const n=BigInt(value);
  if(n< -U64||n>U64)
    throw new Error("Expected delta exceeds u64 bound: "+name);
  return n;
}
function orderKey(order){
  if(!order||!["ask","bid"].includes(order.side)||
      !decimal(order.sequence)||order.sequence==="0"||
      !decimal(order.baseQty)||order.baseQty==="0"||
      !decimal(order.priceX64)||order.priceX64==="0"||
      BigInt(order.baseQty)>U64)
    throw new Error("Invalid maker owned order");
  return order.side+":"+order.sequence;
}
function orderMap(list){
  if(!Array.isArray(list))throw new Error("Missing authenticated owned orders");
  const map=new Map();
  for(const item of list){
    const id=orderKey(item);
    if(map.has(id))throw new Error("Duplicate maker order identity");
    map.set(id,item);
  }
  return map;
}
function snapshot(value,name){
  if(!value||value.mode!=="post-finality-audit-only"||value.canSubmit!==false||
      !Number.isSafeInteger(value.slot)||value.slot<=0||
      !Number.isSafeInteger(value.finalizedTransactionSlot)||
      value.finalizedTransactionSlot<=0||
      value.slot<value.finalizedTransactionSlot)
    throw new Error("Invalid finalized "+name+" snapshot");
  for(const key of FIELDS)amount(value[key],name+" "+key);
  return orderMap(value.ownedOrders);
}
/**
 * Compare independently authenticated before/after state and a separately
 * verified ledger of expected asset changes. A lack of ledger evidence
 * MUST NOT be silently treated as zero. The exact set of maker-owned
 * resting orders must also match an independently verified expected set.
 * No fill model, market price, fee or transfer is inferred.
 */
export function reconcileMakerInventory({
  before,after,expectedAssetDeltas,expectedOwnedOrders,
}){
  const oldOrders=snapshot(before,"before");
  const newOrders=snapshot(after,"after");
  if(after.slot<=before.slot||
      after.finalizedTransactionSlot<=before.finalizedTransactionSlot)
    throw new Error("Post-trade state did not advance past the previous finalized state");
  if(!expectedAssetDeltas||typeof expectedAssetDeltas!=="object"||
      Array.isArray(expectedAssetDeltas)||
      JSON.stringify(Object.keys(expectedAssetDeltas).sort())!==
      JSON.stringify([...FIELDS].sort()))
    throw new Error("Complete independently verified collateral deltas required");
  const expected=orderMap(expectedOwnedOrders);
  if(expected.size!==newOrders.size)
    throw new Error("Unexpected maker resting order count");
  for(const [id,want] of expected){
    const got=newOrders.get(id);
    if(!got||got.priceX64!==want.priceX64||got.baseQty!==want.baseQty)
      throw new Error("Unexpected maker resting order identity or contents");
  }
  const actualDeltas={};
  for(const field of FIELDS){
    const actual=after[field]-before[field];
    const requested=signedDelta(expectedAssetDeltas[field],field);
    if(actual!==requested)
      throw new Error("Unexplained "+field+" collateral delta");
    actualDeltas[field]=actual.toString();
  }
  return Object.freeze({
    mode:"inventory-reconciliation-only",
    beforeSlot:before.slot,afterSlot:after.slot,
    oldOwnedOrders:oldOrders.size,newOwnedOrders:newOrders.size,
    assetDeltas:Object.freeze(actualDeltas),
    canSubmit:false,requiresIndependentReceiptAuthentication:true,
  });
}
