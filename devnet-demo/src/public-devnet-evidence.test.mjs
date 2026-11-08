import test from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import bs58 from "bs58";

const names = ["initialize", "deposit", "place", "trade", "cancel", "withdraw"];
const address = byte => bs58.encode(Uint8Array.from({length:32}, () => byte));
const signature = byte => bs58.encode(Uint8Array.from({length:64}, () => byte));
const bundle = () => ({
  network: "devnet", rpc: "https://api.devnet.solana.com",
  programId: address(1), market: address(2), maker: address(3), taker: address(4),
  stages: names.map((name, i) => ({name, signature: signature(i+1), confirmed: true})),
  receipts: {verifiedOnPublicRpc: true},
  audit: {tradeSettlementVerified: true, finalBalancesReconciled: true, bookStateVerified: true}
});
function run(data) {
  const dir = mkdtempSync(join(tmpdir(), "hybrid-devnet-"));
  const path = join(dir, "evidence.json");
  try {
    writeFileSync(path, JSON.stringify(data));
    return spawnSync(process.execPath, [fileURLToPath(new URL("./public-devnet-evidence.mjs", import.meta.url)), path], {encoding:"utf8"});
  } finally { rmSync(dir, {recursive:true, force:true}); }
}
test("valid publicly audited bundle passes structural gate", () => assert.equal(run(bundle()).status, 0));
test("local Surfpool data is rejected", () => { const b=bundle(); b.network="surfpool"; assert.equal(run(b).status,1); });
test("reused signatures and unverifiable receipts fail", () => { const b=bundle(); b.stages[1].signature=b.stages[0].signature; b.receipts.verifiedOnPublicRpc=false; assert.equal(run(b).status,1); });
test("placeholder and malformed addresses are rejected", () => {
  for (const invalid of ["program", "0OIl", "1".repeat(33), "1".repeat(31), ""]) {
    const b=bundle(); b.programId=invalid; assert.equal(run(b).status,1,invalid);
  }
  const b=bundle(); b.market="market"; assert.equal(run(b).status,1);
  const c=bundle(); c.maker="1".repeat(33); assert.equal(run(c).status,1);
  const d=bundle(); d.taker="1".repeat(31); assert.equal(run(d).status,1);
});
test("malformed and non-64-byte stage signatures are rejected", () => {
  for (const invalid of ["signature-0".padEnd(64,"x"), "1".repeat(63), "1".repeat(65), ""]) {
    const b=bundle(); b.stages[0].signature=invalid; assert.equal(run(b).status,1,invalid);
  }
});
