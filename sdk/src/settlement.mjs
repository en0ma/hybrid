// Bounded settlement ABI descriptors. No signing or submission.
import {unsigned,appendAggregateGuard} from "./instructions.mjs";
import {encodeAtomicFrozenMakerClaim,hashPubkeyMakerClaim,hashPubkeyClaimNode,
  hashPubkeyFrozenEpochV1,hashPubkeyFrozenBidEpochV1} from "./frozen-pubkey-claim.mjs";

const MAX64=(1n<<64n)-1n;
const hex=b=>Buffer.from(b).toString("hex");
export function keyBytes(key){
  const b=typeof key?.toBytes==="function"?key.toBytes():key;
  if(!(b instanceof Uint8Array)||b.length!==32)throw new TypeError("Expected 32-byte public key");
  return Buffer.from(b);
}
const id=key=>hex(keyBytes(key));
const eq=(a,b)=>id(a)===id(b);
function positive(value,name){
  const n=unsigned(value,64,name);
  if(n===0n)throw new RangeError(name+" must be positive");
  return n;
}
const le64=n=>{const b=Buffer.alloc(8);b.writeBigUInt64LE(n);return b;};
const meta=(pubkey,isWritable=false,isSigner=false)=>{
  keyBytes(pubkey);return {pubkey,isWritable,isSigner};
};
function distinct(keys){
  if(new Set(keys.map(k=>id(k.pubkey))).size!==keys.length)throw new Error("Aliased settlement account");
}
export function buildSettlementExecution(programId,a,{side,baseFill,quoteLimit}){
  keyBytes(programId);
  if(side!=="ask"&&side!=="bid")throw new RangeError("Invalid side");
  if(!Array.isArray(a.pages)||![1,2].includes(a.pages.length))throw new Error("Expected one or two pages");
  const multi=a.pages.length===2,bid=side==="bid",cap=bid?16:8;
  if(!Array.isArray(a.epochs)||a.epochs.length!==a.pages.length)throw new Error("Epoch count mismatch");
  const balances=a.makerBalances??[];
  if(!Array.isArray(balances)||balances.length>cap||
      ((multi||bid)&&balances.length<(multi?2:1))||(!multi&&!bid&&balances.length))
    throw new Error("Invalid maker roster");
  const keys=[meta(a.market,true),meta(a.custody,true),meta(a.reserve,true),
    ...a.epochs.map(k=>meta(k,true))];
  for(const [index,p] of a.pages.entries()){
    if(p.index!==index)throw new Error("Noncanonical page order");
    keys.push(meta(p.page,true),meta(p.ownerPage,true));
  }
  keys.push(meta(a.taker,false,true),meta(a.takerQuote,true),meta(a.takerBase,true),
    meta(a.quoteVault,true),meta(a.baseVault,true),meta(a.quoteMint),meta(a.baseMint),
    meta(a.vaultAuthority),meta(a.tokenProgram),meta(a.payer,true,true),meta(a.systemProgram),
    ...balances.map(k=>meta(k,bid)));
  if(multi)distinct(keys);
  else {
    // Single-page ABI permits payer and taker to coincide.
    distinct(keys.filter((_,i)=>i!==keys.length-balances.length-2||!eq(a.payer,a.taker)));
  }
  return appendAggregateGuard({programId,keys,data:Buffer.concat([Buffer.from([multi?(bid?44:45):(bid?42:41)]),
    le64(positive(baseFill,"baseFill")),le64(positive(quoteLimit,"quoteLimit"))])},a);
}

function fixed(data,size,magic){
  if(!(data instanceof Uint8Array))throw new TypeError("Expected binary account data");
  const b=Buffer.from(data);
  if(b.length!==size||b.subarray(0,8).toString("ascii")!==magic||b[8]!==1)
    throw new Error("Invalid "+magic+" layout");
  return b;
}
const zero=b=>b.every(x=>x===0);
const vector=(b,start,count)=>Array.from({length:count},(_,i)=>b.readBigUInt64LE(start+i*8));
export function decodeLazyReserve(data){
  const b=fixed(data,144,"HYBLAZY1");
  if(!zero(b.subarray(10,16))||!zero(b.subarray(136))||
    zero(b.subarray(16,48))||zero(b.subarray(48,80)))throw new Error("Invalid reserve");
  const [nextEpochId,makerBaseCredit,makerQuoteCredit,makerBaseDebit,makerQuoteDebit,baseFees,quoteFees]=vector(b,80,7);
  return {bump:b[9],marketHex:hex(b.subarray(16,48)),genesisHex:hex(b.subarray(48,80)),
    nextEpochId,makerBaseCredit,makerQuoteCredit,makerBaseDebit,makerQuoteDebit,baseFees,quoteFees};
}
export function decodeFrozenEpoch(data){
  const b=fixed(data,912,"HYBEPOCH");
  const side=b[11],makerCount=b.readUInt32LE(232),claimedCount=b.readUInt32LE(348);
  const totals=vector(b,236,6),remaining=vector(b,352,6);
  const tickSqrtPriceX64=b.readBigUInt64LE(152)|(b.readBigUInt64LE(160)<<64n);
  const fields=[16,48,80,112,168,200,284,316].map(i=>b.subarray(i,i+32));
  if(b[10]!==1||side>1||!zero(b.subarray(12,16))||!makerCount||makerCount>4096||
    !tickSqrtPriceX64||fields.some(zero)||fields[6].equals(fields[7])||
    claimedCount>makerCount||remaining.some((v,i)=>v>totals[i])||
    (!claimedCount&&remaining.some((v,i)=>v!==totals[i]))||
    (side===0?(!totals[0]||totals[1]||totals[2]||totals[4]):
      (!totals[1]||totals[0]||totals[3]||totals[5])))throw new Error("Invalid frozen epoch");
  const claimed=[];
  for(let i=0;i<4096;i++)if(b[400+(i>>3)]&(1<<(i%8))){
    if(i>=makerCount)throw new Error("Claim outside roster");claimed.push(i);
  }
  if(claimed.length!==claimedCount)throw new Error("Bitmap count mismatch");
  const [marketHex,authorityHex,programHex,genesisHex,commitment,claimRoot,baseVaultHex,quoteVaultHex]=fields.map(hex);
  return {bump:b[9],side:side===0?"ask":"bid",marketHex,authorityHex,programHex,genesisHex,
    commitment,claimRoot,baseVaultHex,quoteVaultHex,epochId:b.readBigUInt64LE(144),
    tickSqrtPriceX64,makerCount,claimedCount,claimed,totals,remaining};
}

// Reconstruct the exact V1 roster from durable pre-execution data. Never
// accept a proof unless BOTH commitment and root match the authenticated epoch.
export function recoverEpochProofs(epoch,makers){
  if(!Array.isArray(makers)||makers.length!==epoch.makerCount)throw new Error("Roster length mismatch");
  const spec={marketHex:epoch.marketHex,programHex:epoch.programHex,genesisHex:epoch.genesisHex,
    epochId:String(epoch.epochId),tickSqrtPriceX64:String(epoch.tickSqrtPriceX64),
    makers,totals:epoch.totals.map(String)};
  const digest=(epoch.side==="ask"?hashPubkeyFrozenEpochV1:hashPubkeyFrozenBidEpochV1)(spec);
  if(digest!==epoch.commitment)throw new Error("Epoch commitment mismatch");
  const sums=Array(6).fill(0n);
  for(const m of makers){
    if(!Array.isArray(m.amounts)||m.amounts.length!==6)throw new Error("Invalid claim amounts");
    m.amounts.forEach((v,i)=>{sums[i]+=BigInt(v);});
  }
  if(sums.some((v,i)=>v!==epoch.totals[i]))throw new Error("Claim totals mismatch");
  const levels=[makers.map((m,seat)=>hashPubkeyMakerClaim(digest,{...m,seat}))];
  while(levels.at(-1).length>1){
    const row=levels.at(-1),next=[];
    for(let i=0;i<row.length;i+=2)next.push(hashPubkeyClaimNode(row[i],row[i+1]??row[i]));
    levels.push(next);
  }
  if(levels.at(-1)[0]!==epoch.claimRoot)throw new Error("Claim root mismatch");
  return makers.map((m,seat)=>{
    let index=seat;const siblings=[];
    for(const row of levels.slice(0,-1)){
      siblings.push(row[index^1]??row[index]);index>>=1;
    }
    return {...m,seat,siblings,genesisHex:epoch.genesisHex,claimed:epoch.claimed.includes(seat)};
  });
}
export function buildSettlementClaim(programId,a,epoch,proof){
  if(id(programId)!==epoch.programHex||id(a.market)!==epoch.marketHex||
    id(a.maker)!==proof.makerHex||proof.genesisHex!==epoch.genesisHex||
    proof.seat>=epoch.makerCount||epoch.claimed.includes(proof.seat))
    throw new Error("Claim identity or replay mismatch");
  let node=hashPubkeyMakerClaim(epoch.commitment,proof),index=proof.seat,count=epoch.makerCount,depth=0;
  while(count>1){
    const sibling=proof.siblings[depth++];
    if(index%2===0&&index+1>=count&&sibling!==node)throw new Error("Invalid odd proof");
    node=index%2?hashPubkeyClaimNode(sibling,node):hashPubkeyClaimNode(node,sibling);
    index>>=1;count=Math.ceil(count/2);
  }
  if(depth!==proof.siblings.length||node!==epoch.claimRoot)throw new Error("Claim proof mismatch");
  if(id(a.vault)!==(epoch.side==="ask"?epoch.quoteVaultHex:epoch.baseVaultHex))throw new Error("Wrong payout vault");
  const keys=[meta(a.market),meta(a.epoch,true),meta(a.makerBalance,true),meta(a.maker,false,true),
    meta(a.custody),meta(a.reserve,true),meta(a.vault,true),meta(a.destination,true),
    meta(a.mint),meta(a.vaultAuthority),meta(a.tokenProgram)];
  distinct(keys);
  if(a.aggregateGuard&&(id(a.aggregateGuard.baseVault)!==epoch.baseVaultHex||
    id(a.aggregateGuard.quoteVault)!==epoch.quoteVaultHex))throw new Error("Claim guard vault mismatch");
  return appendAggregateGuard({programId,keys,data:encodeAtomicFrozenMakerClaim(proof)},a);
}
export function reserveVaultFloor(reserve,base,quote){
  const b=unsigned(base,64,"base")+reserve.makerBaseCredit+reserve.baseFees;
  const q=unsigned(quote,64,"quote")+reserve.makerQuoteCredit+reserve.quoteFees;
  if(b>MAX64||q>MAX64)throw new Error("Vault floor overflow");
  return {base:b,quote:q};
}
