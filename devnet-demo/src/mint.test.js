import test from "node:test";
import assert from "node:assert/strict";
import {verifyClassicMint} from "./mint.js";
test("classic SPL mint decimals 44 and initialized byte 45",()=>{
 const b=new Uint8Array(82);b[44]=6;b[45]=1;
 assert.equal(verifyClassicMint(b,6),true);
 assert.throws(()=>verifyClassicMint(b,9),/decimals/);
 b[45]=0;assert.throws(()=>verifyClassicMint(b,6),/initialized/);
 assert.throws(()=>verifyClassicMint(new Uint8Array(81),6),/length/);
});
