// Read-only maker collateral snapshot for Hybrid state v1.
// Caller MUST derive and verify canonical maker-balance PDA and expected market.
// No token movement, signing, or order-submission capabilities.
function keyBytes(key, role) {
  const bytes = typeof key?.toBytes === "function" ? key.toBytes() :
    (Buffer.isBuffer(key) || key instanceof Uint8Array ? key : null);
  if (!bytes || bytes.length !== 32) throw new TypeError("Expected 32-byte " + role);
  return Buffer.from(bytes);
}

export function decodeMakerBalance(data, {market, maker}) {
  if (!(Buffer.isBuffer(data) || data instanceof Uint8Array)) {
    throw new TypeError("Expected decoded maker-balance account bytes");
  }
  const b = Buffer.from(data);
  if (b.length !== 112 || b.subarray(0, 8).toString("ascii") !== "HYBBAL01"
      || b[8] !== 1 || b.subarray(10,16).some(x => x !== 0)) {
    throw new Error("Invalid maker-balance magic, version, reserved bytes or length");
  }
  if (!b.subarray(16,48).equals(keyBytes(market,"market")) ||
      !b.subarray(48,80).equals(keyBytes(maker,"maker"))) {
    throw new Error("Maker-balance market or owner mismatch");
  }
  return {
    bump:b[9],
    freeBase:b.readBigUInt64LE(80),
    lockedBase:b.readBigUInt64LE(88),
    freeQuote:b.readBigUInt64LE(96),
    lockedQuote:b.readBigUInt64LE(104),
  };
}

/** RPC must return account owner + bytes from one context request.
 * This read validates account owner and contents but NOT PDA derivation.
 * A trusted caller must validate expected maker balance PDA first.
 */
export async function loadMakerBalanceSnapshot(connection, {
  programId, market, maker, makerBalance, minContextSlot=0,
  commitment="confirmed",
}) {
  if (!Number.isSafeInteger(minContextSlot) || minContextSlot < 0) {
    throw new RangeError("Invalid minContextSlot");
  }
  if (!connection || typeof connection.getAccountInfoAndContext !== "function") {
    throw new TypeError("Connection must support getAccountInfoAndContext");
  }
  const expectedProgram=keyBytes(programId,"programId");
  const response=await connection.getAccountInfoAndContext(makerBalance,{
    commitment,minContextSlot
  });
  if (!Number.isSafeInteger(response?.context?.slot) ||
      response.context.slot < minContextSlot) {
    throw new Error("Invalid or stale maker-balance RPC context");
  }
  const account=response.value;
  if (!account || !keyBytes(account.owner,"account owner").equals(expectedProgram)) {
    throw new Error("Missing or incorrectly owned maker-balance account");
  }
  return {
    slot:response.context.slot,
    ...decodeMakerBalance(account.data,{market,maker}),
  };
}
