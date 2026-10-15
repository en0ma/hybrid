import test from "node:test";
import assert from "node:assert/strict";
import {clusterConfig,assertCluster,DEVNET_GENESIS} from "./cluster.js";
test("Surfpool is explicitly restricted to local HTTP RPC",()=>{
 assert.equal(clusterConfig("surfpool","http://127.0.0.1:8899").cluster,"surfpool");
 assert.equal(clusterConfig("surfpool","http://localhost:8899").cluster,"surfpool");
 for(const url of ["https://api.mainnet-beta.solana.com","http://evil.example:8899","http://127.0.0.1.evil.example:8899","http://user:pw@127.0.0.1:8899"])
  assert.throws(()=>clusterConfig("surfpool",url));
 assert.throws(()=>clusterConfig("mainnet","https://api.mainnet-beta.solana.com"));
});
test("public devnet validates genesis; Surfpool remains explicitly local",async()=>{
 const local={getGenesisHash:async()=> "simulator-genesis"};
 assert.equal(await assertCluster(local,"surfpool"),"simulator-genesis");
 await assert.rejects(()=>assertCluster(local,"devnet"));
 assert.equal(await assertCluster({getGenesisHash:async()=>DEVNET_GENESIS},"devnet"),DEVNET_GENESIS);
});
