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
