import test from "node:test";
import assert from "node:assert/strict";
import {hashPubkeyMakerClaim,hashPubkeyClaimNode,encodeFrozenClaimPreflight} from "../src/frozen-pubkey-claim.mjs";

const epoch="04".repeat(32);
const leaf={seat:0,makerHex:"05".repeat(32),weight:"10",
  amounts:["4","0","0","12","0","1"]};
test("Rust-compatible public-key claim leaf and domain-separated node hashes",()=>{
  const digest=hashPubkeyMakerClaim(epoch,leaf);
  assert.equal(digest,
    "dd947a836deb66eec48699d9852f13979a08092a3831b707d87bb23defde0a84");
  assert.equal(hashPubkeyClaimNode(digest,digest),
    "58727584703157c37930af764088f159261d7dbfa9ee1f94a0d7dc840dc490c2");
});
test("key/seat/epoch/weight/amount changes cannot reuse the hash",()=>{
  const root=hashPubkeyMakerClaim(epoch,leaf);
  for(const [context,change] of [
    [epoch,{...leaf,seat:1}],
    [epoch,{...leaf,makerHex:"06".repeat(32)}],
    [epoch,{...leaf,weight:"11"}],
    [epoch,{...leaf,amounts:["4","0","0","12","0","2"]}],
    ["07".repeat(32),leaf],
  ])assert.notEqual(hashPubkeyMakerClaim(context,change),root);
  assert.throws(()=>hashPubkeyMakerClaim(epoch,{...leaf,weight:"0"}),/Out-of-range/);
  assert.throws(()=>hashPubkeyMakerClaim(epoch,{...leaf,seat:4096}),/Invalid/);
  assert.throws(()=>hashPubkeyMakerClaim(epoch,{...leaf,amounts:[
    "4","0","0","12","0",(1n<<64n).toString(),
  ]}),/Out-of-range/);
  assert.throws(()=>hashPubkeyMakerClaim(epoch,{...leaf,makerHex:"wrong"}),/Invalid/);
  assert.throws(()=>hashPubkeyMakerClaim(epoch,{...leaf,makerHex:"0".repeat(64)}),/Zero/);
  assert.throws(()=>hashPubkeyMakerClaim(epoch,{...leaf,amounts:[
    "4","0","0","12","0","01",
  ]}),/Noncanonical/);
});

test("opcode 38 encodes exact 94-byte read-only header and sibling path",()=>{
  const encoded=encodeFrozenClaimPreflight({
    genesisHex:"33".repeat(32),seat:4095,weight:"10",
    amounts:["4","0","0","12","0","1"],
    siblings:["aa".repeat(32),"bb".repeat(32)],
  });
  assert.equal(encoded.length,94+64);
  assert.equal(encoded[0],38);
  assert.equal(encoded.subarray(1,33).toString("hex"),"33".repeat(32));
  assert.equal(encoded.readUInt32LE(33),4095);
  assert.equal(encoded.readBigUInt64LE(37),10n);
  assert.equal(encoded.readBigUInt64LE(45),4n);
  assert.equal(encoded.readBigUInt64LE(69),12n);
  assert.equal(encoded[93],2);
  assert.equal(encoded.subarray(94,126).toString("hex"),"aa".repeat(32));
  assert.equal(encoded.subarray(126).toString("hex"),"bb".repeat(32));
  assert.throws(()=>encodeFrozenClaimPreflight({
    genesisHex:"33".repeat(32),seat:4096,weight:"10",
    amounts:["4","0","0","12","0","1"],siblings:[],
  }),/Invalid/);
  assert.throws(()=>encodeFrozenClaimPreflight({
    genesisHex:"33".repeat(32),seat:0,weight:"0",
    amounts:["4","0","0","12","0","1"],siblings:[],
  }),/Out-of-range/);
  assert.throws(()=>encodeFrozenClaimPreflight({
    genesisHex:"33".repeat(32),seat:0,weight:"1",
    amounts:["4","0","0","12","0","1"],siblings:Array(13).fill("aa".repeat(32)),
  }),/Invalid/);
});
