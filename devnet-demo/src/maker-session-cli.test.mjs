import test from "node:test";
import assert from "node:assert/strict";
import {PublicKey} from "@solana/web3.js";
import {parseMakerConfig,runMakerCommand} from "./maker-session-cli.mjs";
const key=n=>new PublicKey(Buffer.alloc(32,n)).toBase58();
const cfg={rpcUrl:"http://127.0.0.1:8899",expectedGenesisHash:key(1),market:key(2),
 maker:key(3),askHeadOwner:key(4),bidHeadOwner:key(5),sessionPath:"/tmp/private/maker",
 isolatedTestWalletAcknowledged:true,orders:[]};
const env={HYBRID_SANDBOX_EXECUTION:"1",HYBRID_MAKER_KILL_SWITCH:"off"};
test("sandbox commands require both explicit switches and literal loopback",()=>{
 for(const command of ["sandbox-init","sandbox-reserve","sandbox-prepare","sandbox-submit",
   "sandbox-finalize","sandbox-reconcile","sandbox-renew"]){
  assert.throws(()=>parseMakerConfig(cfg,command,{}),/Sandbox requires/);
  for(const rpcUrl of ["https://api.devnet.solana.com","http://localhost:8899","http://127.0.0.1:8899/path"])
   assert.throws(()=>parseMakerConfig({...cfg,rpcUrl},command,env),/Sandbox requires/);
  assert.throws(()=>parseMakerConfig({...cfg,isolatedTestWalletAcknowledged:false},command,env),/Sandbox requires/);
  assert.equal(parseMakerConfig(cfg,command,env).control.sandboxSubmit,true);
 }
});
test("read-only commands do not enable submission even with switches",()=>{
 for(const command of ["observe","plan","inspect"]){
  const opts=parseMakerConfig({...cfg,rpcUrl:"https://api.devnet.solana.com"},command,env);
  assert.equal(opts.control.enabled,false);assert.equal(opts.control.sandboxSubmit,false);
 }
});
test("reject secret or unknown config fields, relative session paths and malformed genesis",()=>{
 assert.throws(()=>parseMakerConfig({...cfg,privateKey:"not allowed"},"observe",{}),/Unexpected/);
 assert.throws(()=>parseMakerConfig({...cfg,sessionPath:"relative"},"sandbox-init",env),/Absolute/);
 assert.throws(()=>parseMakerConfig({...cfg,expectedGenesisHash:"bad"},"observe",{}));
});
test("public sandbox command rejected before RPC construction",async()=>{
 let calls=0;
 await assert.rejects(runMakerCommand("sandbox-submit",{...cfg,rpcUrl:"https://api.devnet.solana.com"},
  "/tmp/wire",env,()=>{calls++;throw Error("Unexpected RPC");}),/Sandbox requires/);
 assert.equal(calls,0);
});
