import test from "node:test";
import assert from "node:assert/strict";
import {spawnSync} from "node:child_process";
import {fileURLToPath} from "node:url";
import bs58 from "bs58";
const cli=fileURLToPath(new URL("./cli.mjs",import.meta.url));
const run=(...args)=>spawnSync(process.execPath,[cli,...args],{encoding:"utf8",timeout:5000});
const key=(byte)=>bs58.encode(Uint8Array.from({length:32},()=>byte));
test("reject unknown subcommands",()=>assert.equal(run("unknown").status,2));
test("requires market and account arguments",()=>assert.equal(run("readiness","abc").status,2));
test("reject malformed public keys locally",()=>{
  for(const bad of ["not-a-pubkey","1".repeat(33),"1".repeat(31),"0OIl",""]) {
    const manifest=run("manifest",bad);
    assert.equal(manifest.status,2,bad);
    assert.match(manifest.stderr,/Invalid Solana public key/);
  }
});
test("readiness rejects malformed keys before calling RPC",()=>{
  const valid=key(7);
  for(let i=0;i<5;i++){
    const args=Array(5).fill(valid);
    args[i]="1".repeat(33);
    const result=run("readiness",...args);
    assert.equal(result.status,2,"bad argument index "+i);
    assert.match(result.stderr,/Invalid Solana public key/);
  }
});
test("valid key is not rejected by argument validation",()=>{
  // A valid key proceeds to the downstream script; it might fail for missing RPC config.
  const result=run("manifest",key(1));
  assert.doesNotMatch(result.stderr,/Invalid Solana public key/);
  assert.notEqual(result.status,2);
});
