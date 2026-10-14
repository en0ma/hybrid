import test from "node:test";
import assert from "node:assert/strict";
import {selectIsolatedHybridInstruction} from "./isolated-receipt.js";
const program={equals:x=>x==="hybrid"};
const ix=(id,data)=>({programId:id,data});
const decode=s=>Uint8Array.from(s.split(",").map(Number));
test("accepts one expected Hybrid instruction with unrelated system instructions",()=>{
 const r=selectIsolatedHybridInstruction([ix("system","0"),ix("hybrid","30,0")],program,new Set([30]),decode);
 assert.equal(r.data[0],30);
});
test("rejects other Hybrid opcodes bundled in an apparently valid lifecycle receipt",()=>{
 assert.throws(()=>selectIsolatedHybridInstruction([ix("hybrid","30"),ix("hybrid","31")],program,new Set([30]),decode),/exactly one/);
 assert.throws(()=>selectIsolatedHybridInstruction([ix("hybrid","31")],program,new Set([30]),decode),/opcode/);
 assert.throws(()=>selectIsolatedHybridInstruction([],program,new Set([30]),decode),/exactly one/);
 assert.throws(()=>selectIsolatedHybridInstruction([ix("hybrid",null)],program,new Set([30]),decode),/encoded data/);
});
