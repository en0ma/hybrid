import test from "node:test";import assert from "node:assert/strict";import {spawnSync} from "node:child_process";import {mkdtempSync,writeFileSync} from "node:fs";import {tmpdir} from "node:os";import {join} from "node:path";
const names=["initialize","deposit","place","trade","cancel","withdraw"];
const bundle=()=>({network:"devnet",rpc:"https://api.devnet.solana.com",programId:"program",market:"market",maker:"maker",taker:"taker",stages:names.map((name,i)=>({name,signature:("signature-"+i).padEnd(64,"x"),confirmed:true})),receipts:{verifiedOnPublicRpc:true},audit:{tradeSettlementVerified:true,finalBalancesReconciled:true,bookStateVerified:true}});
function run(data){const path=join(mkdtempSync(join(tmpdir(),"hybrid-devnet-")),"evidence.json");writeFileSync(path,JSON.stringify(data));return spawnSync(process.execPath,[new URL("./public-devnet-evidence.mjs",import.meta.url).pathname,path],{encoding:"utf8"});}
test("valid publicly audited bundle passes structural gate",()=>assert.equal(run(bundle()).status,0));
test("local Surfpool data is rejected",()=>{const b=bundle();b.network="surfpool";assert.equal(run(b).status,1)});
test("reused signatures and unverifiable receipts fail",()=>{const b=bundle();b.stages[1].signature=b.stages[0].signature;b.receipts.verifiedOnPublicRpc=false;assert.equal(run(b).status,1)});
