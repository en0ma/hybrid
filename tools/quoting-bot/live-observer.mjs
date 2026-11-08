// Public RPC market-data observer. It NEVER signs or submits orders.
// Trust root: caller supplies already-derived canonical PDAs and program ID.
// The SDK loader validates program ownership, sidecar binding and page layout.
import {loadActivePageSnapshot} from "../../sdk/src/rpc.mjs";
import {planQuotes} from "./plan.mjs";

const Q64 = 1n << 64n;
const SCALE = 10n ** 18n;
function asPriceX64(value) {
  const n = BigInt(value);
  if (n <= 0n || n >= (1n << 128n)) throw Error("Invalid book price");
  return n;
}
export function decimalFromQ64(value) {
  const n = asPriceX64(value);
  // Deterministic floor to 18 decimal places. Never use IEEE-754 floats.
  const whole = n / Q64;
  const fractional = ((n % Q64) * SCALE / Q64).toString().padStart(18, "0").replace(/0+$/, "");
  return fractional ? whole + "." + fractional : whole.toString();
}
export function verifiedMidPrice(ask, bid) {
  if (!ask?.orders?.length || !bid?.orders?.length) throw Error("Both book sides required");
  if (ask.side !== "ask" || bid.side !== "bid" || ask.slot !== bid.slot) {
    throw Error("Mixed sides or RPC slots");
  }
  if (!Number.isSafeInteger(ask.slot) || ask.slot <= 0) throw Error("Invalid book slot");
  const bestAsk = asPriceX64(ask.orders[0].priceX64);
  const bestBid = asPriceX64(bid.orders[0].priceX64);
  if (bestBid >= bestAsk) throw Error("Crossed or locked book");
  return decimalFromQ64((bestBid + bestAsk) / 2n);
}
export function checkSlotFreshness(bookSlot, currentSlot, maxLagSlots) {
  if (![bookSlot,currentSlot,maxLagSlots].every(x=>Number.isSafeInteger(x)&&x>=0)
      || bookSlot===0 || currentSlot < bookSlot || currentSlot-bookSlot>maxLagSlots) {
    throw Error("Stale or invalid market slot");
  }
}

/**
 * Observe actual RPC book accounts through the authenticated SDK decoder.
 * Generates NON-EXECUTABLE quote targets from operator-supplied risk caps.
 * These caps are not authenticated maker balances; do not turn this function's
 * return value into automatic transactions. Add custody verification,
 * Q64/lot rounding, simulation, approvals and reconciliation first.
 */
export async function observeLiveQuotes(connection, config) {
  const {programId,market,askPage,askOwnerPage,bidPage,bidOwnerPage,
    minContextSlot=0,maxLagSlots=10,strategy} = config;
  if (!connection || typeof connection.getSlot !== "function" || !strategy) {
    throw Error("Connection and strategy required");
  }
  const ask=await loadActivePageSnapshot(connection,{
    programId,market,page:askPage,ownerPage:askOwnerPage,side:"ask",minContextSlot
  });
  const bid=await loadActivePageSnapshot(connection,{
    programId,market,page:bidPage,ownerPage:bidOwnerPage,side:"bid",
    minContextSlot:Math.max(minContextSlot,ask.slot)
  });
  const observedSlot=await connection.getSlot("confirmed");
  checkSlotFreshness(ask.slot,observedSlot,maxLagSlots);
  const midPriceAtoms=verifiedMidPrice(ask,bid);
  const plan=planQuotes({...strategy,midPriceAtoms,stale:false});
  return {
    mode:"observation-only",slot:ask.slot,observedSlot,midPriceAtoms,
    plan,transactionsSubmitted:0,
  };
}
