import { decodeMarketHeader, decodeOrderPage, decodeOwnerPage } from "./state.mjs";

const bytes = value => {
  if (typeof value?.toBytes === "function") return Buffer.from(value.toBytes());
  if (value instanceof Uint8Array) return Buffer.from(value);
  throw new TypeError("Expected PublicKey bytes");
};
const same = (a,b) => bytes(a).equals(bytes(b));
const validIndex = value => Number.isInteger(value) && value >= 0 && value < 0xffffffff;

/**
 * Read an ordered chain of active pages at one RPC context slot. This is a
 * read-only snapshot for a future bounded multi-page execution ABI: the
 * existing single-page swap instruction CANNOT consume this structure.
 *
 * The caller must derive and verify each page and owner-sidecar PDA using
 * the program ID, market address, and the chain's index before submitting
 * any on-chain instruction. verifyAddresses is mandatory to avoid treating
 * caller-controlled accounts as canonical PDAs.
 */
export async function loadLinkedBookSnapshot(connection, {
  programId, market, side, pages, verifyAddresses,
  minContextSlot = 0, commitment = "confirmed",
}) {
  if (side !== "ask" && side !== "bid") throw new RangeError("Invalid book side");
  if (!connection || typeof connection.getMultipleAccountsInfoAndContext !== "function") {
    throw new TypeError("Context account RPC is required");
  }
  if (typeof verifyAddresses !== "function") throw new TypeError("PDA verifier is required");
  if (!Array.isArray(pages) || pages.length < 1 || pages.length > 8) {
    throw new RangeError("Expected between one and eight page/sidecar pairs");
  }
  if (!Number.isSafeInteger(minContextSlot) || minContextSlot < 0) {
    throw new RangeError("Invalid minimum context slot");
  }
  if (!programId || !market) throw new TypeError("Missing program ID or market");
  const seen = new Set();
  for (const [index, pair] of pages.entries()) {
    if (!pair || !pair.page || !pair.ownerPage || !validIndex(pair.index)) {
      throw new TypeError("Invalid page/sidecar pair");
    }
    if (seen.has(pair.index) || pair.index !== index) {
      throw new Error("Duplicate index or missing page zero");
    }
    seen.add(pair.index);
    if (same(pair.page, pair.ownerPage)) throw new Error("Aliased page and sidecar");
    if (!await verifyAddresses({programId,market,side,index:pair.index,page:pair.page,ownerPage:pair.ownerPage})) {
      throw new Error("Unverified page or owner-sidecar address");
    }
  }
  const keys = [market, ...pages.flatMap(pair => [pair.page,pair.ownerPage])];
  const response = await connection.getMultipleAccountsInfoAndContext(
    keys, {commitment,minContextSlot},
  );
  if (!response || !Number.isSafeInteger(response.context?.slot) ||
      response.context.slot < minContextSlot ||
      !Array.isArray(response.value) || response.value.length !== keys.length) {
    throw new Error("Invalid or stale multi-page RPC snapshot");
  }
  const account = (i, role) => {
    const entry = response.value[i];
    if (!entry || !same(entry.owner,programId)) throw new Error("Missing or wrongly owned "+role);
    if (!(entry.data instanceof Uint8Array)) throw new Error("Non-binary "+role);
    return entry.data;
  };
  const header = decodeMarketHeader(account(0,"market"));
  if (!header.collateralizedActive) throw new Error("Inactive collateralized market");
  const all = [], used = new Set();
  let previous = null;
  for (let i = 0; i < pages.length; i++) {
    const pair = pages[i];
    const page = decodeOrderPage(account(1+i*2,"page"),side);
    const entries = decodeOwnerPage(account(2+i*2,"sidecar"),page);
    const links = page.links;
    if (links.pageIndex !== i ||
        links.prevPage !== (previous?.links.pageIndex ?? null) ||
        links.nextPage !== (pages[i+1]?.index ?? null)) {
      throw new Error("Broken page/owner chain or incomplete traversal");
    }
    if (i === 0) {
      const sidecar = bytes(pair.ownerPage);
      if (side === "ask" && !header.askOwnerKeyBytes.equals(sidecar)) {
        throw new Error("Invalid ask head sidecar binding");
      }
      if (side === "bid" && !header.bidOwnerTagBytes.equals(sidecar.subarray(0,16))) {
        throw new Error("Invalid bid head sidecar binding");
      }
    }
    if (i > 0 && i + 1 < pages.length && entries.length === 0) {
      throw new Error("Empty intermediate order page");
    }
    for (const entry of entries) {
      const last = all.at(-1);
      if (last) {
        const sorted = side === "ask"
          ? entry.priceX64 > last.priceX64 ||
            (entry.priceX64 === last.priceX64 && entry.sequence > last.sequence)
          : entry.priceX64 < last.priceX64 ||
            (entry.priceX64 === last.priceX64 && entry.sequence > last.sequence);
        if (!sorted) throw new Error("Cross-page price/time priority violation");
      }
      if (used.has(entry.sequence.toString())) throw new Error("Duplicate cross-page sequence");
      used.add(entry.sequence.toString());
      all.push({...entry,pageIndex:pair.index});
    }
    previous = page;
  }
  const expected = side === "ask" ? header.askCount : header.bidCount;
  if (all.length !== expected) throw new Error("Market total depth mismatch");
  return {slot:response.context.slot,market,side,header,pages:pages.map(p=>({...p})),orders:all};
}
