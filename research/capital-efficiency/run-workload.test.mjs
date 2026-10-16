import test from "node:test";import assert from "node:assert/strict";import {readFileSync} from "node:fs";import {executeWorkload} from "./run-workload.mjs";
const contract=JSON.parse(readFileSync(new URL("./paired-workload.json",import.meta.url),"utf8"));
const mk=name=>({name,identity:async()=>({commit:"a".repeat(40),programId:"3".repeat(44)}),execute:async()=>({status:"executed",signature:"2".repeat(88),slot:123})});
test("requires both protocol adapters",async()=>assert.rejects(()=>executeWorkload(contract,[mk("hybrid")]),/require Hybrid and Manifest/));
test("requires executed signatures for every operation",async()=>{const r=await executeWorkload(contract,[mk("hybrid"),mk("manifest")]);assert.equal(r.outputs.length,10);assert.equal(r.kind,"adapter-executed-workload-receipts-unverified")});
test("refuses fabricated plan-only result",async()=>{const a=mk("hybrid");a.execute=async()=>({status:"simulated",signature:"2".repeat(88),slot:123});await assert.rejects(()=>executeWorkload(contract,[a,mk("manifest")]),/did not provide real executed evidence/)});
test("rejects unpinned implementation",async()=>{const a=mk("hybrid");a.identity=async()=>({commit:"main",programId:"3".repeat(44)});await assert.rejects(()=>executeWorkload(contract,[a,mk("manifest")]),/unpinned/)});
