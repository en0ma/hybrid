import test from "node:test";
import assert from "node:assert/strict";
import {
  cancelOrder, closePassivePosition, initMakerBalance, initPassivePool,
  openPassivePosition, placeOrder, swap, transferMakerCollateral, unsigned,
} from "../src/instructions.mjs";

const id = "hybrid-program";
function accounts(roles) {
  return Object.fromEntries(roles.map((role) => [role, `test-${role}`]));
}
const order = accounts(["market","page","ownerPage","maker","makerBalance"]);
const trade = {
  ...accounts(["market","custody","page","ownerPage","taker","takerBase","takerQuote",
    "baseVault","quoteVault","vaultAuthority","tokenProgram"]),
  makerBalances: ["maker-a","maker-b"],
};

test("maker place/cancel encode exact page-zero alpha ABI", () => {
  for (const [side, placeOp, cancelOp] of [["ask",6,7],["bid",8,9]]) {
    const place = placeOrder(id, order, {
      side, priceX64: 1n << 64n, sqrtPriceX64: 1n << 64n, baseQty: 42n,
    });
    assert.equal(place.programId, id);
    assert.equal(place.data.length, 41);
    assert.equal(place.data[0], placeOp);
    assert.equal(place.data.readBigUInt64LE(33), 42n);
    assert.equal(place.data.readBigUInt64LE(25), 1n);
    assert.deepEqual(place.keys.map(k => k.pubkey), [
      "test-market","test-page","test-ownerPage","test-maker","test-makerBalance",
    ]);
    assert.equal(place.keys[3].isSigner, true);
    assert.equal(place.keys[4].isWritable, true);
    const cancel = cancelOrder(id, order, { side, sequence: 65537n });
    assert.equal(cancel.data.length, 9);
    assert.equal(cancel.data[0], cancelOp);
    assert.equal(cancel.data.readBigUInt64LE(1), 65537n);
  }
});

test("all four swap modes use their distinct account ordering and limits", () => {
  for (const [side,mode,op] of [
    ["buy","exact-in",15],["buy","exact-out",16],
    ["sell","exact-in",17],["sell","exact-out",18],
  ]) {
    const ix = swap(id, trade, { side, mode, amount: 5n, limit: 7n });
    assert.equal(ix.data.length, 17);
    assert.equal(ix.data[0], op);
    assert.equal(ix.data.readBigUInt64LE(1), 5n);
    assert.equal(ix.data.readBigUInt64LE(9), 7n);
    assert.equal(ix.keys.length, 13);
    assert.equal(ix.keys[5].pubkey, side === "buy" ? "test-takerQuote" : "test-takerBase");
    assert.equal(ix.keys[7].pubkey, side === "buy" ? "test-quoteVault" : "test-baseVault");
    assert.equal(ix.keys[11].pubkey, "maker-a");
    assert.equal(ix.keys[12].pubkey, "maker-b");
    assert.equal(ix.keys[4].isSigner, true);
    assert.equal(ix.keys[10].isWritable, false);
  }
});

test("maker collateral transfers use exact ten-byte layout", () => {
  for (const [direction, op] of [["deposit",13],["withdraw",14]]) {
    const a = accounts(["market","custody","makerBalance","maker","source","vault",
      "destination","mint","vaultAuthority","tokenProgram"]);
    const ix = transferMakerCollateral(id, a, { direction, asset:"quote", amount: 9n });
    assert.equal(ix.data.length, 10);
    assert.equal(ix.data[0], op);
    assert.equal(ix.data[1], 1);
    assert.equal(ix.data.readBigUInt64LE(2), 9n);
    assert.equal(ix.keys.length, direction === "deposit" ? 8 : 9);
  }
});

test("passive position encodes all 73 bytes and authenticated close", () => {
  const a = accounts(["market","custody","pool","position","owner","baseSource",
    "quoteSource","baseVault","quoteVault","baseMint","quoteMint","tokenProgram",
    "systemProgram","baseDestination","quoteDestination","vaultAuthority"]);
  const open = openPassivePosition(id,a, {
    nonce: 9n, lowerSqrtPriceX64: 1n << 63n, upperSqrtPriceX64: 1n << 65n,
    liquidity: 100n, baseDeposit: 12n, quoteDeposit: 13n,
  });
  assert.equal(open.data.length,73);
  assert.equal(open.data[0],20);
  assert.equal(open.data.readBigUInt64LE(1),9n);
  assert.equal(open.data.readBigUInt64LE(41),100n);
  assert.equal(open.data.readBigUInt64LE(57),12n);
  assert.equal(open.data.readBigUInt64LE(65),13n);
  assert.equal(open.keys[4].isSigner,true);
  const close = closePassivePosition(id,a,{nonce:9n});
  assert.equal(close.data.length,9);
  assert.equal(close.data[0],21);
  assert.equal(close.keys[4].isSigner,true);
  assert.equal(close.keys[12].pubkey,"test-tokenProgram");
  assert.equal(initPassivePool(id,a).data[0],19);
  assert.equal(initMakerBalance(id,{...a, maker:"maker", makerBalance:"balance",payer:"payer"}).data[0],12);
});

test("reject imprecise numbers, overflow, missing accounts and invalid fan-out", () => {
  assert.throws(()=> unsigned(2 ** 64,64,"value"),TypeError);
  assert.throws(()=> unsigned(-1n,64,"value"),RangeError);
  assert.throws(()=> unsigned(1n << 64n,64,"value"),RangeError);
  assert.throws(()=> unsigned(1n << 128n,128,"value"),RangeError);
  assert.throws(()=> placeOrder(id,order,{side:"ask",priceX64:1n,sqrtPriceX64:1n,baseQty:-1n}),RangeError);
  assert.throws(()=> cancelOrder(id,order,{side:"other",sequence:1n}),RangeError);
  assert.throws(()=> swap(id,{...trade,makerBalances:Array(9).fill("maker")},{
    side:"buy",mode:"exact-in",amount:1n,limit:1n,
  }),RangeError);
  assert.throws(()=> swap(id,{...trade,taker:undefined},{
    side:"buy",mode:"exact-in",amount:1n,limit:1n,
  }),TypeError);
});
