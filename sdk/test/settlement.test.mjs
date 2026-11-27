import test from "node:test";
import assert from "node:assert/strict";
import {buildSettlementExecution,decodeLazyReserve,decodeFrozenEpoch,recoverEpochProofs,
  buildSettlementClaim,reserveVaultFloor} from "../src/settlement.mjs";
import {hashPubkeyFrozenEpochV1,hashPubkeyMakerClaim,hashPubkeyClaimNode} from "../src/frozen-pubkey-claim.mjs";
const k=n=>Buffer.alloc(32,n);
const a={market:k(1),custody:k(2),reserve:k(3),epochs:[k(4),k(5)],
  pages:[{index:0,page:k(6),ownerPage:k(7)},{index:1,page:k(8),ownerPage:k(9)}],
  taker:k(10),takerQuote:k(11),takerBase:k(12),quoteVault:k(13),baseVault:k(14),
  quoteMint:k(15),baseMint:k(16),vaultAuthority:k(17),tokenProgram:k(18),payer:k(19),
  systemProgram:k(0),makerBalances:[k(20),k(21)]};
test("multipage execution exact ABI and account flags",()=>{
  for(const side of ["ask","bid"]){
    const ix=buildSettlementExecution(k(99),a,{side,baseFill:350n,quoteLimit:505n});
    assert.equal(ix.data.length,17);
    assert.equal(ix.data[0],side==="ask"?45:44);
    assert.equal(ix.data.readBigUInt64LE(1),350n);
    assert.equal(ix.data.readBigUInt64LE(9),505n);
    assert.equal(ix.keys.length,22);
    assert.deepEqual(ix.keys.slice(0,20).map(x=>x.pubkey),[a.market,a.custody,a.reserve,
      ...a.epochs,a.pages[0].page,a.pages[0].ownerPage,a.pages[1].page,a.pages[1].ownerPage,
      a.taker,a.takerQuote,a.takerBase,a.quoteVault,a.baseVault,a.quoteMint,a.baseMint,
      a.vaultAuthority,a.tokenProgram,a.payer,a.systemProgram]);
    assert.equal(ix.keys[20].isWritable,side==="bid");
    assert.equal(ix.keys[9].isSigner,true);
    assert.equal(ix.keys[18].isSigner,true);
  }
  assert.throws(()=>buildSettlementExecution(k(99),{...a,payer:a.taker},{side:"ask",baseFill:1,quoteLimit:1}));
  assert.throws(()=>buildSettlementExecution(k(99),{...a,makerBalances:Array.from({length:9},(_,i)=>k(30+i))},{side:"ask",baseFill:1,quoteLimit:1}));
});
test("single-page opcodes and caps",()=>{
  const single={...a,pages:a.pages.slice(0,1),epochs:a.epochs.slice(0,1)};
  assert.equal(buildSettlementExecution(k(99),{...single,makerBalances:[]},{side:"ask",baseFill:1,quoteLimit:1}).data[0],41);
  assert.equal(buildSettlementExecution(k(99),single,{side:"bid",baseFill:1,quoteLimit:1}).data[0],42);
  assert.throws(()=>buildSettlementExecution(k(99),single,{side:"bid",baseFill:0,quoteLimit:1}));
});
test("reserve decoder and checked floors",()=>{
  const b=Buffer.alloc(144);b.write("HYBLAZY1");b[8]=1;k(1).copy(b,16);k(2).copy(b,48);
  [3n,149n,300n,300n,150n,1n,2n].forEach((n,i)=>b.writeBigUInt64LE(n,80+i*8));
  const r=decodeLazyReserve(b);assert.equal(r.nextEpochId,3n);
  assert.deepEqual(reserveVaultFloor(r,0n,150n),{base:150n,quote:452n});
  b[10]=1;assert.throws(()=>decodeLazyReserve(b));
});
test("epoch proof recovery, claim construction and replay rejection",()=>{
  const makers=[{makerHex:k(20).toString("hex"),weight:"100",amounts:["50","0","0","50","0","0"]},
    {makerHex:k(21).toString("hex"),weight:"200",amounts:["100","0","0","100","0","1"]}];
  const totals=["150","0","0","150","0","1"];
  const spec={marketHex:k(1).toString("hex"),programHex:k(99).toString("hex"),
    genesisHex:k(3).toString("hex"),epochId:"0",tickSqrtPriceX64:String(1n<<64n),makers,totals};
  const digest=hashPubkeyFrozenEpochV1(spec);
  const leaves=makers.map((m,seat)=>hashPubkeyMakerClaim(digest,{...m,seat}));
  const root=hashPubkeyClaimNode(...leaves);
  const b=Buffer.alloc(912);b.write("HYBEPOCH");b[8]=1;b[10]=1;
  for(const [off,v] of [[16,spec.marketHex],[48,spec.marketHex],[80,spec.programHex],
    [112,spec.genesisHex],[168,digest],[200,root],[284,k(14).toString("hex")],[316,k(13).toString("hex")]])
    Buffer.from(v,"hex").copy(b,off);
  b.writeBigUInt64LE(1n,160);b.writeUInt32LE(2,232);
  totals.forEach((n,i)=>{b.writeBigUInt64LE(BigInt(n),236+i*8);b.writeBigUInt64LE(BigInt(n),352+i*8);});
  const e=decodeFrozenEpoch(b),proofs=recoverEpochProofs(e,makers);
  assert.deepEqual(proofs[0].siblings,[leaves[1]]);
  const accounts={market:k(1),epoch:k(4),makerBalance:k(23),maker:k(20),custody:k(2),reserve:k(3),
    vault:k(13),destination:k(25),mint:k(15),vaultAuthority:k(17),tokenProgram:k(18)};
  assert.equal(buildSettlementClaim(k(99),accounts,e,proofs[0]).data[0],40);
  const guard={pool:k(22),reserve:accounts.reserve,baseVault:k(14),quoteVault:k(13)};
  const guarded=buildSettlementClaim(k(99),{...accounts,marketFlags:3,aggregateGuard:guard},e,proofs[0]);
  assert.deepEqual(guarded.keys.slice(-4).map(x=>x.pubkey),Object.values(guard));
  assert.throws(()=>buildSettlementClaim(k(99),{...accounts,marketFlags:3},e,proofs[0]),/guard required/);
  assert.throws(()=>buildSettlementClaim(k(99),{...accounts,aggregateGuard:{...guard,baseVault:k(26)}},e,proofs[0]),/vault mismatch/);
  assert.throws(()=>buildSettlementClaim(k(99),accounts,{...e,claimed:[0]},proofs[0]));
  assert.throws(()=>recoverEpochProofs({...e,claimRoot:k(77).toString("hex")},makers));
  assert.throws(()=>buildSettlementClaim(k(99),{...accounts,vault:k(14)},e,proofs[0]));
  b[400]=4;assert.throws(()=>decodeFrozenEpoch(b));
});

test("guarded settlement execution appends canonical ledger and vault tail after roster",()=>{
  const guard={pool:k(22),reserve:a.reserve,baseVault:a.baseVault,quoteVault:a.quoteVault};
  const params={side:"bid",baseFill:350n,quoteLimit:505n};
  const unguarded=buildSettlementExecution(k(99),a,params);
  const guarded=buildSettlementExecution(k(99),{...a,marketFlags:3,aggregateGuard:guard},params);
  assert.deepEqual(guarded.keys.slice(0,-4),unguarded.keys);
  assert.deepEqual(guarded.keys.slice(-4).map(x=>x.pubkey),Object.values(guard));
  assert.throws(()=>buildSettlementExecution(k(99),{...a,marketFlags:3},params),/guard required/);
  assert.throws(()=>buildSettlementExecution(k(99),{...a,marketFlags:3,aggregateGuard:{...guard,reserve:k(23)}},params),/mismatch/);
});
