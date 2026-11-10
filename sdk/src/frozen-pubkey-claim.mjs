// Pure, read-only compatibility helper for the Solana pubkey-bound maker
// claim leaf ABI in crates/settlement/src/frozen_claim_gate.rs.
// This hash authenticates NOTHING without a trusted, on-chain epoch root.
import {createHash} from "node:crypto";

const LEAF=Buffer.from("HYBRID_MAKER_PUBKEY_CLAIM_V1\0","utf8");
const NODE=Buffer.from("HYBRID_MAKER_PUBKEY_NODE_V1\0","utf8");
const HEX=/^[0-9a-f]{64}$/;
const MAX64=(1n<<64n)-1n;
function pubkey(value,name){
  if(typeof value!=="string"||!HEX.test(value))
    throw new Error("Invalid "+name);
  if(name==="maker public key"&&value==="0".repeat(64))
    throw new Error("Zero maker public key");
  return Buffer.from(value,"hex");
}
function u64(value,name,{positive=false}={}){
  if(typeof value!=="string"||!/^(0|[1-9][0-9]*)$/.test(value))
    throw new Error("Noncanonical "+name);
  const n=BigInt(value);
  if(n>MAX64||(positive&&n===0n))throw new Error("Out-of-range "+name);
  const bytes=Buffer.alloc(8);bytes.writeBigUInt64LE(n);return bytes;
}
const sha=bytes=>createHash("sha256").update(bytes).digest("hex");
/** Binary bytes: domain || epoch digest || seat u32LE || maker pubkey
 * || weight u64LE || 6 ordered u64LE asset amounts.
 */
export function hashPubkeyMakerClaim(epochDigest,{seat,makerHex,weight,amounts}){
  if(!Number.isSafeInteger(seat)||seat<0||seat>=4096||
      !Array.isArray(amounts)||amounts.length!==6)
    throw new Error("Invalid pubkey claim seat/amount vector");
  const index=Buffer.alloc(4);index.writeUInt32LE(seat);
  return sha(Buffer.concat([LEAF,pubkey(epochDigest,"epoch digest"),
    index,pubkey(makerHex,"maker public key"),u64(weight,"weight",{positive:true}),
    ...amounts.map((a,i)=>u64(a,"amount "+i))]));
}
export function hashPubkeyClaimNode(left,right){
  return sha(Buffer.concat([NODE,pubkey(left,"left node"),pubkey(right,"right node")]));
}

/** Read-only Solana opcode 38 ABI. Account metas MUST be supplied and
 * authenticated separately: market, frozen epoch PDA, balance PDA, signer.
 * A successful preflight NEVER authorizes withdrawal or modifies balances.
 */
export function encodeFrozenClaimPreflight({
  genesisHex,seat,weight,amounts,siblings,
}){
  if(!Number.isSafeInteger(seat)||seat<0||seat>=4096||
      !Array.isArray(amounts)||amounts.length!==6||
      !Array.isArray(siblings)||siblings.length>12)
    throw new Error("Invalid frozen preflight input");
  const genesis=pubkey(genesisHex,"genesis hash");
  const index=Buffer.alloc(4);index.writeUInt32LE(seat);
  const nodes=siblings.map((value)=>pubkey(value,"Merkle sibling"));
  return Buffer.concat([
    Buffer.from([38]),genesis,index,u64(weight,"weight",{positive:true}),
    ...amounts.map((value,i)=>u64(value,"amount "+i)),
    Buffer.from([nodes.length]),...nodes,
  ]);
}

/** Program-derived single-tick ASK epoch commitment, distinct from the
 * earlier experimental text-identity commitment. No arbitrary-root sealing.
 * Preimage: domain, market[32], program[32], genesis[32], epoch u64LE,
 * tick sqrt Q64 u128LE, side 0, count u32LE, maker rows
 * (index u32LE, maker pubkey[32], weight u64LE), six u64LE totals.
 */
export function hashPubkeyFrozenEpochV1({
  marketHex,programHex,genesisHex,epochId,tickSqrtPriceX64,makers,totals,
}){
  if(!Array.isArray(makers)||makers.length<1||makers.length>8||
      !Array.isArray(totals)||totals.length!==6)
    throw new Error("Invalid on-chain frozen ask epoch");
  const le128=(value)=>{
    if(typeof value!=="string"||!/^(0|[1-9][0-9]*)$/.test(value)||
        BigInt(value)===0n||BigInt(value)>=(1n<<128n))
      throw new Error("Invalid tick Q64");
    let n=BigInt(value);const out=Buffer.alloc(16);
    for(let i=0;i<16;i++,n>>=8n)out[i]=Number(n&255n);
    return out;
  };
  const count=Buffer.alloc(4);count.writeUInt32LE(makers.length);
  const seen=new Set();
  const rows=makers.map((maker,i)=>{
    if(!maker||seen.has(maker.makerHex))throw new Error("Duplicate maker");
    seen.add(maker.makerHex);
    const index=Buffer.alloc(4);index.writeUInt32LE(i);
    return Buffer.concat([index,pubkey(maker.makerHex,"maker public key"),
      u64(maker.weight,"weight",{positive:true})]);
  });
  const totalBytes=totals.map((v,i)=>u64(v,"total "+i));
  if(totals[0]==="0"||totals[1]!=="0"||totals[2]!=="0"||totals[4]!=="0")
    throw new Error("Invalid ask epoch totals");
  const W=makers.reduce((n,m)=>n+BigInt(m.weight),0n);
  if(W>(1n<<64n)-1n||BigInt(totals[0])>W)throw new Error("Overfilled roster");
  return sha(Buffer.concat([
    Buffer.from("HYBRID_LAZY_PUBKEY_EPOCH_V1\0","utf8"),
    pubkey(marketHex,"market"),pubkey(programHex,"program"),
    pubkey(genesisHex,"genesis"),u64(epochId,"epoch ID"),
    le128(tickSqrtPriceX64),Buffer.from([0]),count,
    ...rows,...totalBytes,
  ]));
}

/** Market signer initializes the only market-wide cross-epoch liability PDA.
 * The genesis field is an authenticated deployment-domain assertion by
 * that signer, not a Solana genesis-hash syscall.
 */
export function encodeLazyReserveInit(genesisHex){
  return Buffer.concat([Buffer.from([39]),pubkey(genesisHex,"genesis")]);
}
/** The order page is authenticated on-chain; makers and the Merkle root
 * are never supplied by the taker or the SDK for sealing.
 */
export function encodeLazyAskExecute(filledBase,maxQuoteIn){
  return Buffer.concat([Buffer.from([41]),
    u64(filledBase,"base fill",{positive:true}),
    u64(maxQuoteIn,"max quote",{positive:true})]);
}
/** The claim transaction repeats all preflight checks and atomically
 * updates balances, reserves, bitmap and checked SPL token payout.
 */
export function encodeAtomicFrozenMakerClaim(spec){
  const bytes=encodeFrozenClaimPreflight(spec);
  bytes[0]=40;
  return bytes;
}
