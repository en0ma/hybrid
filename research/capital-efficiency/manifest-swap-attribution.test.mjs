import test from "node:test";import assert from "node:assert/strict";import {inspectManifestSwapReceipt} from "./manifest-swap-attribution.mjs";
const receipt=()=>({slot:9,meta:{err:null},transaction:{signatures:["sig"],message:{accountKeys:["program","taker","market","base","quote","vaultA","vaultB","token"],instructions:[{programIdIndex:0,accounts:[1,2,3,4,5,6,7,7],data:"2g"}]}}});
test("authenticates exact core Swap opcode while withholding settlement verification",()=>{const x=inspectManifestSwapReceipt({receipt:receipt(),signature:"sig",programId:"program",market:"market",taker:"taker"});assert.equal(x.verified,false);assert.equal(x.matchingSwaps,1);assert.equal(x.opcodeAuthenticated,true)});
test("rejects fake signature",()=>assert.throws(()=>inspectManifestSwapReceipt({receipt:receipt(),signature:"wrong",programId:"program",market:"market"}),/signature mismatch/));

test("rejects unrelated instructions with a different opcode",()=>{const r=receipt();r.transaction.message.instructions[0].data="3";assert.throws(()=>inspectManifestSwapReceipt({receipt:r,signature:"sig",programId:"program",market:"market",taker:"taker"}),/exactly one/) });
