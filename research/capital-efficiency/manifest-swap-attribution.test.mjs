import test from "node:test";import assert from "node:assert/strict";import {inspectManifestSwapReceipt} from "./manifest-swap-attribution.mjs";
const receipt=()=>({slot:9,meta:{err:null},transaction:{signatures:["sig"],message:{header:{numRequiredSignatures:1},accountKeys:["taker","program","market","base","quote","vaultA","vaultB","token"],instructions:[{programIdIndex:1,accounts:[0,2,3,4,5,6,7,7],data:"m8dYDRSpmjZnr9MWz9AxKzLtY"}]}}});
test("authenticates exact core Swap opcode while withholding settlement verification",()=>{const x=inspectManifestSwapReceipt({receipt:receipt(),signature:"sig",programId:"program",market:"market",taker:"taker"});assert.equal(x.verified,false);assert.equal(x.matchingSwaps,1);assert.equal(x.opcodeAuthenticated,true)});
test("rejects fake signature",()=>assert.throws(()=>inspectManifestSwapReceipt({receipt:receipt(),signature:"wrong",programId:"program",market:"market"}),/signature mismatch/));

test("rejects unrelated instructions with a different opcode",()=>{const r=receipt();r.transaction.message.instructions[0].data="3";assert.throws(()=>inspectManifestSwapReceipt({receipt:r,signature:"sig",programId:"program",market:"market",taker:"taker"}),/exactly one/) });

test("rejects a taker not in the signed account header",()=>{const r=receipt();r.transaction.message.header.numRequiredSignatures=1;r.transaction.message.accountKeys[0]="someoneElse";assert.throws(()=>inspectManifestSwapReceipt({receipt:r,signature:"sig",programId:"program",market:"market",taker:"taker"}),/not an authenticated transaction signer/) });
test("rejects impossible signer header",()=>{const r=receipt();r.transaction.message.header.numRequiredSignatures=2;assert.throws(()=>inspectManifestSwapReceipt({receipt:r,signature:"sig",programId:"program",market:"market",taker:"taker"}),/signer header/) });

test("decodes exact atom quantities and flags",()=>{const x=inspectManifestSwapReceipt({receipt:receipt(),signature:"sig",programId:"program",market:"market",taker:"taker"});assert.deepEqual(x.swapParams,{inAtoms:"40",outAtoms:"20",isBaseIn:false,isExactIn:true})});
test("rejects truncated swap parameters",()=>{const r=receipt();r.transaction.message.instructions[0].data="Jf";assert.throws(()=>inspectManifestSwapReceipt({receipt:r,signature:"sig",programId:"program",market:"market",taker:"taker"}),/exactly 18 parameter bytes/) });
