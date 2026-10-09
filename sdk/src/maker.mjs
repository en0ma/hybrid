import { cancelOrder, placeOrder, unsigned } from "./instructions.mjs";

// Only reconciles the maker's own page-zero orders. It does not fetch a book,
// derive PDAs, check balances, sign, or send a transaction.
function normalize(order, desired) {
  if (order.side !== "ask" && order.side !== "bid") throw new RangeError("Invalid side");
  const priceX64 = unsigned(order.priceX64, 128, "priceX64");
  const sqrtPriceX64 = unsigned(order.sqrtPriceX64, 128, "sqrtPriceX64");
  const baseQty = unsigned(order.baseQty, 64, "baseQty");
  if (!priceX64 || !sqrtPriceX64 || !baseQty) throw new RangeError("Zero order field");
  const entry = { side:order.side, priceX64, sqrtPriceX64, baseQty };
  if (!desired) {
    entry.sequence = unsigned(order.sequence, 64, "sequence");
    if (!entry.sequence) throw new RangeError("Zero order sequence");
  }
  return entry;
}
function same(a,b) {
  return a.side === b.side && a.priceX64 === b.priceX64 &&
    a.sqrtPriceX64 === b.sqrtPriceX64 && a.baseQty === b.baseQty;
}
/**
 * Construct a conservative desired-state plan. Existing orders must be known
 * to belong to this maker; do not use a whole-market list. Duplicates are
 * matched one-to-one. Cancellations precede placements, and callers MUST
 * re-read chain state and simulate before submitting.
 *
 * page capacity and collateral are NOT checked by this module.
 */
export function reconcileMakerOrders({ existing, desired, maxChanges = 16 }) {
  if (!Array.isArray(existing) || !Array.isArray(desired)) throw new TypeError("Arrays required");
  if (!Number.isSafeInteger(maxChanges) || maxChanges < 0) throw new RangeError("Invalid maxChanges");
  const old = existing.map(x=>normalize(x,false));
  const next = desired.map(x=>normalize(x,true));
  const seen = new Set();
  for (const item of old) {
    const k = String(item.sequence);
    if (seen.has(k)) throw new Error("Duplicate maker order sequence");
    seen.add(k);
  }
  const remaining = [...next];
  const cancellations = [];
  for (const item of old) {
    const i = remaining.findIndex(want=>same(item,want));
    if (i >= 0) remaining.splice(i,1);
    else cancellations.push({ side:item.side, sequence:item.sequence });
  }
  const placements = remaining;
  if (cancellations.length + placements.length > maxChanges) {
    throw new RangeError("Plan exceeds maxChanges");
  }
  return { cancellations, placements, unchanged: old.length - cancellations.length };
}
export function buildMakerReconciliation(programId, { ask, bid }, plan) {
  if (!ask || !bid) throw new TypeError("Provide both ask and bid account roles");
  const roles = (side) => side === "ask" ? ask : bid;
  return [
    ...plan.cancellations.map(x=>cancelOrder(programId,roles(x.side),x)),
    ...plan.placements.map(x=>placeOrder(programId,roles(x.side),x)),
  ];
}
