import test from "node:test";
import assert from "node:assert/strict";
import {hashPubkeyMakerClaim,hashPubkeyClaimNode,encodeFrozenClaimPreflight,hashPubkeyFrozenEpochV1,hashPubkeyFrozenBidEpochV1,encodeLazyReserveInit,encodeLazyAskExecute,encodeAtomicFrozenMakerClaim} from "../src/frozen-pubkey-claim.mjs";

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

test("on-chain authenticated 2-maker ask epoch has canonical 289-byte digest vector",()=>{
  const spec={
    marketHex:"01".repeat(32),programHex:"02".repeat(32),
    genesisHex:"03".repeat(32),epochId:"0",
    tickSqrtPriceX64:(1n<<64n).toString(),
    makers:[{makerHex:"05".repeat(32),weight:"100"},
      {makerHex:"06".repeat(32),weight:"200"}],
    totals:["150","0","0","150","0","1"],
  };
  assert.equal(hashPubkeyFrozenEpochV1(spec),
    "5f36a8ed5802bab492090c2811ef96aa10e5a129c486d3d2a5ca99ad5ea677c7");
  const digest=hashPubkeyFrozenEpochV1(spec);
  const a=hashPubkeyMakerClaim(digest,{seat:0,makerHex:"05".repeat(32),
    weight:"100",amounts:["50","0","0","50","0","0"]});
  const b=hashPubkeyMakerClaim(digest,{seat:1,makerHex:"06".repeat(32),
    weight:"200",amounts:["100","0","0","100","0","1"]});
  assert.equal(a,"f14a0febecc126d1fdd742155a9569e1b297171ee0b047555837ed12bcb9bca4");
  assert.equal(b,"a66a8f3da75adc07be0ab5fadef63329d4117a9ed8714d55cd2e3c377c0acd8c");
  assert.equal(hashPubkeyClaimNode(a,b),
    "c44b9425f20815d96c14f59c47ba36f997cdfc25a86d2eb5481b701389c94b81");
  assert.notEqual(hashPubkeyFrozenEpochV1({...spec,epochId:"1"}),
    hashPubkeyFrozenEpochV1(spec));
  assert.notEqual(hashPubkeyFrozenEpochV1({...spec,programHex:"09".repeat(32)}),
    hashPubkeyFrozenEpochV1(spec));
  assert.throws(()=>hashPubkeyFrozenEpochV1({...spec,
    makers:[spec.makers[0],spec.makers[0]]}),/Duplicate/);
  assert.throws(()=>hashPubkeyFrozenEpochV1({...spec,
    totals:["301","0","0","150","0","1"]}),/Overfilled/);
});
test("execution and atomic claim instruction discriminators and bounds",()=>{
  const init=encodeLazyReserveInit("03".repeat(32));
  assert.equal(init.length,33);assert.equal(init[0],39);
  const seal=encodeLazyAskExecute("150","151");
  assert.equal(seal.length,17);assert.equal(seal[0],41);
  assert.equal(seal.readBigUInt64LE(1),150n);
  assert.equal(seal.readBigUInt64LE(9),151n);
  assert.throws(()=>encodeLazyAskExecute("0","151"),/Out-of-range/);
  const claim=encodeAtomicFrozenMakerClaim({
    genesisHex:"03".repeat(32),seat:0,weight:"100",
    amounts:["50","0","0","50","0","0"],siblings:["aa".repeat(32)],
  });
  assert.equal(claim[0],40);assert.equal(claim.length,126);
});

test("9/16-seat authenticated BID digests preserve the v1 owner/seat binding",()=>{
  const spec=(count)=>({
    marketHex:"11".repeat(32),programHex:"22".repeat(32),
    genesisHex:"33".repeat(32),epochId:"7",
    tickSqrtPriceX64:(1n<<64n).toString(),
    makers:Array.from({length:count},(_,i)=>({
      makerHex:(i+1).toString(16).padStart(2,"0").repeat(32),
      weight:"1",
    })),
    totals:["0",String(count),String(count),"0","0","0"],
  });
  assert.equal(hashPubkeyFrozenBidEpochV1(spec(9)),
    "81eaae5151672a4b1fb6a0cb1f41dcfc5173003cc6759024fcf91686550733c6");
  assert.equal(hashPubkeyFrozenBidEpochV1(spec(16)),
    "6c753eed7dd4ab15711ec2d48b77c4d3315b7d2839186e9a98a50738cbe14959");
  for(const [count,root] of [
    [9,"06e7d26453d0aaa9b13b40f2afb5c2136be470326d0e1a2a44e48f443d62a025"],
    [16,"7d6f22035f78d5898824a267180132f69fa6bb7c1539b480f88bf41d0551c9e2"],
  ]){
    const record=spec(count);
    const digest=hashPubkeyFrozenBidEpochV1(record);
    let nodes=record.makers.map((maker,seat)=>
      hashPubkeyMakerClaim(digest,{seat,makerHex:maker.makerHex,weight:maker.weight,
        amounts:["0","1","1","0","0","0"]}));
    while(nodes.length>1){
      const parents=[];
      for(let i=0;i<nodes.length;i+=2)
        parents.push(hashPubkeyClaimNode(nodes[i],nodes[i+1]??nodes[i]));
      nodes=parents;
    }
    assert.equal(nodes[0],root);
  }
  assert.throws(()=>hashPubkeyFrozenBidEpochV1(spec(17)),/roster/);
  assert.throws(()=>hashPubkeyFrozenBidEpochV1({...spec(9),
    makers:[...spec(9).makers.slice(0,8),spec(9).makers[0]]}),/Duplicate/);
  assert.throws(()=>hashPubkeyFrozenBidEpochV1({...spec(9),
    totals:["1","9","9","0","0","0"]}),/totals/);
  assert.throws(()=>hashPubkeyFrozenBidEpochV1({...spec(9),
    totals:["0","10","9","0","0","0"]}),/Overfilled/);
  // Net proceeds alone fit weight, but proceeds plus fee overfill it.
  assert.throws(()=>hashPubkeyFrozenBidEpochV1({...spec(9),
    totals:["0","9","10","0","1","0"]}),/Overfilled/);
  // The combined base proceeds and fee exactly fit the roster.
  assert.doesNotThrow(()=>hashPubkeyFrozenBidEpochV1({...spec(9),
    totals:["0","8","9","0","1","0"]}));
  assert.notEqual(hashPubkeyFrozenBidEpochV1({...spec(9),epochId:"8"}),
    hashPubkeyFrozenBidEpochV1(spec(9)));
});
