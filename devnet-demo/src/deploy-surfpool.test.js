import test from "node:test";
import assert from "node:assert/strict";
import {Keypair} from "@solana/web3.js";
import {checkedProgramKeypair} from "./deploy-surfpool.js";
test("fixed-ID deployer refuses unrelated and malformed program keypairs",()=>{
 assert.throws(()=>checkedProgramKeypair([...Keypair.generate().secretKey]),/pinned Hybrid/);
 assert.throws(()=>checkedProgramKeypair([1,2,3]),/64 octets/);
});
