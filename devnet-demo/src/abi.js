// Mirrors program/src/client.rs opcodes 23–26, 30 and 31.
// No network or wallet calls here, to keep encoding independently testable.
export const PROGRAM_ID = "US517G5965aydkZ46HS38QLi7UQiSojurfbQfKCELFx";
export const TOKEN_PROGRAM_ID = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
const maxU64 = (1n << 64n) - 1n;
const maxU128 = (1n << 128n) - 1n;
export function uint(value, bits) {
  if (!/^(0|[1-9][0-9]*)$/.test(String(value).trim())) throw Error("Expected unsigned decimal integer");
  const n = BigInt(value);
  if (n < 1n || n > (bits === 64 ? maxU64 : maxU128)) throw Error("Nonzero amount exceeds u" + bits);
  return n;
}
export function le(value, bytes) {
  const n = uint(value, bytes === 8 ? 64 : 128);
  const b = new Uint8Array(bytes);
  for (let i = 0; i < bytes; i++) b[i] = Number((n >> BigInt(8 * i)) & 255n);
  return b;
}
export function placeData(side, index, price, sqrtPrice, quantity) {
  if (!["ask","bid"].includes(side) || ![0,1].includes(index)) throw Error("Invalid side/page");
  return Uint8Array.from([30, side === "ask" ? 0 : 1, index, ...le(price,16), ...le(sqrtPrice,16), ...le(quantity,8)]);
}
export function cancelData(side,index,sequence) {
  if (!["ask","bid"].includes(side) || ![0,1].includes(index)) throw Error("Invalid side/page");
  return Uint8Array.from([31,side === "ask" ? 0 : 1,index,...le(sequence,8)]);
}
export function tradeData(side,exactOut,pageCount,amount,limit) {
  if (!["ask","bid"].includes(side) || !Number.isInteger(pageCount) || pageCount < 1 || pageCount > 8) throw Error("Invalid side/pages");
  const opcode = side === "ask" ? (exactOut ? 24 : 23) : (exactOut ? 26 : 25);
  return Uint8Array.from([opcode,pageCount,...le(amount,8),...le(limit,8)]);
}
