import { decodeMarketHeader, decodeOrderPage, decodeOwnerPage } from "./state.mjs";

function keyEquals(left, right) {
  if (left == null || right == null) return false;
  if (typeof left.equals === "function") return Boolean(left.equals(right));
  return String(left) === String(right);
}
function dataBytes(value) {
  if (value == null) throw new Error("Missing account data");
  // Accept binary account data, not RPC JSON encoded base64 pairs.
  if (!(Buffer.isBuffer(value) || value instanceof Uint8Array)) {
    throw new TypeError("Expected decoded account bytes");
  }
  return value;
}
function requireAccount(account, role, expectedOwner) {
  if (!account) throw new Error(`Missing account: ${role}`);
  if (!keyEquals(account.owner, expectedOwner)) {
    throw new Error(`Incorrect program owner: ${role}`);
  }
  return dataBytes(account.data);
}

/**
 * Fetch a market, one active order page, and its owner sidecar in ONE RPC
 * getMultipleAccountsInfoAndContext request for a consistent context slot.
 *
 * Addresses supplied here MUST come from trusted PDA derivation and the
 * market's validated sidecar binding, not untrusted user input.
 */
export async function loadActivePageSnapshot(connection, {
  programId, market, page, ownerPage, side, minContextSlot = 0,
  commitment = "confirmed",
}) {
  if (side !== "ask" && side !== "bid") throw new RangeError("Invalid side");
  if (!Number.isSafeInteger(minContextSlot) || minContextSlot < 0) {
    throw new RangeError("Invalid minContextSlot");
  }
  if (!connection || typeof connection.getMultipleAccountsInfoAndContext !== "function") {
    throw new TypeError("Connection must support context account loading");
  }
  if ([programId, market, page, ownerPage].some(x=>x==null)) {
    throw new TypeError("Program, market, page and sidecar addresses are required");
  }
  const response = await connection.getMultipleAccountsInfoAndContext(
    [market, page, ownerPage], {commitment, minContextSlot},
  );
  if (!response || !Number.isSafeInteger(response.context?.slot) ||
      response.context.slot < minContextSlot || !Array.isArray(response.value) ||
      response.value.length !== 3) {
    throw new Error("Invalid or stale RPC account snapshot");
  }
  const [marketInfo, pageInfo, ownerInfo] = response.value;
  const header = decodeMarketHeader(requireAccount(marketInfo, "market", programId));
  if (!header.collateralizedActive) throw new Error("Market is not collateralized");
  const book = decodeOrderPage(requireAccount(pageInfo, "orderPage", programId), side);
  if (book.links.pageIndex !== 0 || book.links.prevPage !== null ||
      book.links.nextPage !== null) {
    throw new Error("Active mutable trading supports only standalone page 0");
  }
  const sidecar = decodeOwnerPage(requireAccount(ownerInfo, "ownerPage", programId), book);
  if (side === "ask") {
    if (book.entries.length !== header.askCount) throw new Error("Ask count mismatch");
    if (!header.askOwnerKeyBytes.equals(Buffer.from(ownerPage.toBytes?.() ?? []))) {
      // For a real PublicKey, require exact binding, not just page compatibility.
      throw new Error("Ask sidecar binding mismatch");
    }
  } else if (book.entries.length !== header.bidCount) {
    throw new Error("Bid count mismatch");
  }
  return {
    slot:response.context.slot,
    side, market, page, ownerPage, header,
    orders:sidecar,
  };
}
