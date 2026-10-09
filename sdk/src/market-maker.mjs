import { loadActivePageSnapshot } from "./rpc.mjs";
import { reconcileMakerOrders, buildMakerReconciliation } from "./maker.mjs";

function publicKeyBytes(pubkey) {
  const data = typeof pubkey?.toBytes === "function" ? pubkey.toBytes() : pubkey;
  if (!(Buffer.isBuffer(data) || data instanceof Uint8Array) || data.length !== 32) {
    throw new TypeError("Maker identity must be a 32-byte public key");
  }
  return Buffer.from(data);
}

/**
 * Build maker order intents from verified owner sidecars.
 * Both sides must come from the SAME RPC slot or the operation fails closed.
 * Never sign or send automatically; balances, page capacity and latest state
 * must still be checked and the transaction simulated before submission.
 */
export async function planAuthenticatedMakerUpdate(connection, {
  programId, market, askPage, askOwnerPage, bidPage, bidOwnerPage,
  maker, makerBalance, desired, minContextSlot = 0, maxChanges = 16,
}) {
  const makerBytes = publicKeyBytes(maker);
  const ask = await loadActivePageSnapshot(connection,{
    programId,market,page:askPage,ownerPage:askOwnerPage,side:"ask",minContextSlot,
  });
  const bid = await loadActivePageSnapshot(connection,{
    programId,market,page:bidPage,ownerPage:bidOwnerPage,side:"bid",
    minContextSlot:Math.max(minContextSlot,ask.slot),
  });
  if (bid.slot !== ask.slot) {
    throw new Error("Mixed RPC slots; retry with a coherent snapshot");
  }
  const ownedAsks=ask.orders.filter(order=>order.owner.equals(makerBytes))
    .map(({owner,...order})=>({...order,side:"ask"}));
  const ownedBids=bid.orders.filter(order=>order.owner.equals(makerBytes))
    .map(({owner,...order})=>({...order,side:"bid"}));
  const plan=reconcileMakerOrders({
    existing:[...ownedAsks,...ownedBids],desired,maxChanges,
  });
  return {
    slot:ask.slot,existingCount:ownedAsks.length+ownedBids.length,
    plan,
    descriptors:buildMakerReconciliation(programId,{
      ask:{market,page:askPage,ownerPage:askOwnerPage,maker,makerBalance},
      bid:{market,page:bidPage,ownerPage:bidOwnerPage,maker,makerBalance:undefined},
    },plan),
  };
}
