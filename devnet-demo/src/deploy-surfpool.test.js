import test from "node:test";
import assert from "node:assert/strict";
import {Keypair} from "@solana/web3.js";
import {checkedProgramKeypair} from "./deploy-surfpool.js";
test("fixed-ID deployer refuses unrelated and malformed program keypairs",()=>{
 assert.throws(()=>checkedProgramKeypair([...Keypair.generate().secretKey]),/pinned Hybrid/);
 assert.throws(()=>checkedProgramKeypair([1,2,3]),/64 octets/);
});

import {programChunks,uploadSurfpoolProgram} from "./deploy-surfpool.js";
import {PROGRAM_ID} from "./abi.js";
const elf=Buffer.concat([Buffer.from([0x7f,0x45,0x4c,0x46]),Buffer.alloc(12,9)]);
test("program chunks preserve exact ELF bytes and ordered offsets",()=>{
 const chunks=programChunks(elf,5);
 assert.deepEqual(chunks.map(x=>x.offset),[0,5,10,15]);
 assert.ok(Buffer.concat(chunks.map(x=>Buffer.from(x.data,"hex"))).equals(elf));
 assert.throws(()=>programChunks(Buffer.from("not an ELF")) ,/ELF/);
});
test("Surfpool upload uses fixed ID without secret key and detects RPC failure",async()=>{
 const requests=[];
 const rpc=async(_,options)=>{requests.push(JSON.parse(options.body));return {ok:true,json:async()=>({result:{}})}};
 const result=await uploadSurfpoolProgram("http://127.0.0.1:8899",elf,rpc);
 assert.equal(result.chunks,1);
 assert.equal(requests[0].method,"surfnet_writeProgram");
 assert.equal(requests[0].params[0],PROGRAM_ID);
 assert.equal(requests[0].params[2],0);
 await assert.rejects(uploadSurfpoolProgram("http://127.0.0.1:8899",elf,
  async()=>({ok:true,json:async()=>({error:{code:-1,message:"bad write"}})})),/bad write/);
});
