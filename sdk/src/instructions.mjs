// Hybrid alpha ABI instruction builders.
// The returned object can be passed to @solana/web3.js TransactionInstruction.
// This module does not derive addresses, fetch accounts, or sign transactions.
const MAX_U64 = (1n << 64n) - 1n;
const MAX_U128 = (1n << 128n) - 1n;

export function unsigned(value, bits, name) {
  if (typeof value !== "bigint" && !(typeof value === "number" && Number.isSafeInteger(value))) {
    throw new TypeError(`${name} must be bigint or a safe integer`);
  }
  const n = BigInt(value);
  if (n < 0n || n > (bits === 64 ? MAX_U64 : MAX_U128)) {
    throw new RangeError(`${name} is outside u${bits}`);
  }
  return n;
}

function le(value, bytes, name) {
  let v = unsigned(value, bytes * 8, name);
  const result = Buffer.alloc(bytes);
  for (let i = 0; i < bytes; i++) {
    result[i] = Number(v & 255n);
    v >>= 8n;
  }
  return result;
}
function required(value, name) {
  if (value === undefined || value === null || value === "") {
    throw new TypeError(`Missing account: ${name}`);
  }
  return value;
}
function meta(accounts, role, flags) {
  return {
    pubkey: required(accounts[role], role),
    isWritable: flags.includes("w"),
    isSigner: flags.includes("s"),
  };
}
function instruction(programId, accounts, roles, data) {
  required(programId, "programId");
  return {
    programId,
    keys: roles.map(([role, flags]) => meta(accounts, role, flags)),
    data: Buffer.concat(data),
  };
}

const activeOrderRoles = [
  ["market", "w"], ["page", "w"], ["ownerPage", "w"],
  ["makerBalance", "w"], ["maker", "s"],
];

export function placeOrder(programId, accounts, { side, priceX64, sqrtPriceX64, baseQty }) {
  if (side !== "ask" && side !== "bid") throw new RangeError("side must be ask or bid");
  return instruction(programId, accounts, activeOrderRoles, [
    Buffer.from([side === "ask" ? 6 : 8]),
    le(priceX64, 16, "priceX64"),
    le(sqrtPriceX64, 16, "sqrtPriceX64"),
    le(baseQty, 8, "baseQty"),
  ]);
}

export function cancelOrder(programId, accounts, { side, sequence }) {
  if (side !== "ask" && side !== "bid") throw new RangeError("side must be ask or bid");
  return instruction(programId, accounts, activeOrderRoles, [
    Buffer.from([side === "ask" ? 7 : 9]), le(sequence, 8, "sequence"),
  ]);
}

const swapRoles = {
  buy: [
    ["market", "w"], ["custody", "w"], ["page", "w"], ["ownerPage", "w"],
    ["taker", "s"], ["takerQuote", "w"], ["takerBase", "w"],
    ["quoteVault", "w"], ["baseVault", "w"], ["vaultAuthority", ""],
    ["tokenProgram", ""],
  ],
  sell: [
    ["market", "w"], ["custody", "w"], ["page", "w"], ["ownerPage", "w"],
    ["taker", "s"], ["takerBase", "w"], ["takerQuote", "w"],
    ["baseVault", "w"], ["quoteVault", "w"], ["vaultAuthority", ""],
    ["tokenProgram", ""],
  ],
};

/**
 * amount and limit correspond to the on-chain ABI:
 * buy exact-in: quote input, minimum base output; buy exact-out: base output, maximum quote input;
 * sell exact-in: base input, minimum quote output; sell exact-out: quote output, maximum base input.
 */
export function swap(programId, accounts, { side, mode, amount, limit }) {
  if (!(side in swapRoles) || !["exact-in", "exact-out"].includes(mode)) {
    throw new RangeError("Invalid swap side or mode");
  }
  if (!Array.isArray(accounts.makerBalances) || accounts.makerBalances.length > 8) {
    throw new RangeError("makerBalances must be an array with at most eight entries");
  }
  const opcode = side === "buy" ? (mode === "exact-in" ? 15 : 16) : (mode === "exact-in" ? 17 : 18);
  const ix = instruction(programId, accounts, swapRoles[side], [
    Buffer.from([opcode]), le(amount, 8, "amount"), le(limit, 8, "limit"),
  ]);
  for (const pubkey of accounts.makerBalances) {
    ix.keys.push({ pubkey: required(pubkey, "makerBalance"), isWritable: true, isSigner: false });
  }
  return ix;
}

const balanceRoles = {
  deposit: [
    ["market", ""], ["custody", "w"], ["makerBalance", "w"],
    ["maker", "s"], ["source", "w"], ["vault", "w"],
    ["mint", ""], ["tokenProgram", ""],
  ],
  withdraw: [
    ["market", ""], ["custody", "w"], ["makerBalance", "w"],
    ["maker", "s"], ["vault", "w"], ["destination", "w"],
    ["mint", ""], ["vaultAuthority", ""], ["tokenProgram", ""],
  ],
};
export function transferMakerCollateral(programId, accounts, { direction, asset, amount }) {
  if (!(direction in balanceRoles)) throw new RangeError("direction must be deposit or withdraw");
  if (asset !== "base" && asset !== "quote") throw new RangeError("asset must be base or quote");
  return instruction(programId, accounts, balanceRoles[direction], [
    Buffer.from([direction === "deposit" ? 13 : 14, asset === "base" ? 0 : 1]),
    le(amount, 8, "amount"),
  ]);
}

export function initMakerBalance(programId, accounts) {
  return instruction(programId, accounts, [
    ["market", ""], ["custody", ""], ["makerBalance", "w"],
    ["maker", "s"], ["payer", "ws"], ["systemProgram", ""],
  ], [Buffer.from([12])]);
}

export function initPassivePool(programId, accounts) {
  return instruction(programId, accounts, [
    ["market", ""], ["custody", ""], ["pool", "w"],
    ["payer", "ws"], ["systemProgram", ""],
  ], [Buffer.from([19])]);
}

export function openPassivePosition(programId, accounts, {
  nonce, lowerSqrtPriceX64, upperSqrtPriceX64, liquidity, baseDeposit, quoteDeposit,
}) {
  return instruction(programId, accounts, [
    ["market", ""], ["custody", ""], ["pool", "w"], ["position", "w"],
    ["owner", "ws"], ["baseSource", "w"], ["quoteSource", "w"],
    ["baseVault", "w"], ["quoteVault", "w"], ["baseMint", ""],
    ["quoteMint", ""], ["tokenProgram", ""], ["systemProgram", ""],
  ], [
    Buffer.from([20]), le(nonce, 8, "nonce"), le(lowerSqrtPriceX64, 16, "lowerSqrtPriceX64"),
    le(upperSqrtPriceX64, 16, "upperSqrtPriceX64"), le(liquidity, 16, "liquidity"),
    le(baseDeposit, 8, "baseDeposit"), le(quoteDeposit, 8, "quoteDeposit"),
  ]);
}

export function closePassivePosition(programId, accounts, { nonce }) {
  return instruction(programId, accounts, [
    ["market", ""], ["custody", ""], ["pool", "w"], ["position", "w"],
    ["owner", "ws"], ["baseVault", "w"], ["quoteVault", "w"],
    ["baseDestination", "w"], ["quoteDestination", "w"], ["baseMint", ""],
    ["quoteMint", ""], ["vaultAuthority", ""], ["tokenProgram", ""],
  ], [Buffer.from([21]), le(nonce, 8, "nonce")]);
}
