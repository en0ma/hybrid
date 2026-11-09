// Canonical binary frozen-epoch commitment, v2 research API.
// Explicit cluster, program and market identity, epoch number, Q64 tick, maker
// roster, and every settlement total are hashed together. Read-only.
// This is NOT a trusted epoch sealer, account checker, or SPL authority.
import {createHash} from "node:crypto";

const DOMAIN=Buffer.from("HYBRID_LAZY_EPOCH_V2\0","utf8");
const U64=(1n<<64n)-1n,U128=(1n<<128n)-1n;
const ASSETS=["baseDebit","baseCredit","quoteDebit","quoteCredit","baseFee","quoteFee"];
const ID=/^[A-Za-z0-9_-]{1,128}$/;
function le(value,max,length,name,{nonzero=false}={}){
  if(typeof value!=="string"||!/^(0|[1-9][0-9]*)$/.test(value))
    throw new Error("Noncanonical "+name);
  const n=BigInt(value);
  if(n>max||(nonzero&&n===0n))throw new Error("Out-of-range "+name);
  const bytes=Buffer.alloc(length);
  for(let v=n,i=0;i<length;i++,v>>=8n)bytes[i]=Number(v&255n);
  return bytes;
}
function checkedRoster(makers){
  if(!Array.isArray(makers)||makers.length===0||makers.length>4096)
    throw new Error("Invalid maker roster size");
  const seen=new Set();
  return makers.map((maker,i)=>{
    if(!maker||typeof maker.id!=="string"||!ID.test(maker.id)||
        seen.has(maker.id))throw new Error("Invalid or duplicate maker ID");
    seen.add(maker.id);
    const weight=le(maker.weight,U64,8,"maker weight",{nonzero:true});
    const id=Buffer.from(maker.id,"ascii");
    const ix=Buffer.alloc(4);ix.writeUInt32LE(i);
    return Buffer.concat([ix,Buffer.from([id.length]),id,weight]);
  });
}
/** Canonical commitment is independent of JS object key order. The roster
 * ordering is IMPORTANT: it is the exact settlement and dust-priority order,
 * not a mutable set. Callers must authenticate its maker/seat contents.
 */
export function encodeFrozenEpochV2({marketHex,programHex,genesisHex,epochId,tickSqrtPriceX64,side,
  makers,totals}){
  if(typeof marketHex!=="string"||!/^[0-9a-f]{64}$/.test(marketHex))
    throw new Error("Invalid canonical 32-byte market");
  if(typeof programHex!=="string"||!/^[0-9a-f]{64}$/.test(programHex)||
      typeof genesisHex!=="string"||!/^[0-9a-f]{64}$/.test(genesisHex))
    throw new Error("Invalid canonical program or genesis identity");
  if(!["ask","bid"].includes(side))throw new Error("Invalid epoch side");
  if(!totals||typeof totals!=="object"||Array.isArray(totals)||
      JSON.stringify(Object.keys(totals).sort())!==
        JSON.stringify([...ASSETS].sort()))throw new Error("Missing asset totals");
  const membership=checkedRoster(makers);
  const count=Buffer.alloc(4);count.writeUInt32LE(membership.length);
  const values=ASSETS.map(name=>le(totals[name],U64,8,name));
  const filledBase=BigInt(totals[side==="ask"?"baseDebit":"baseCredit"]);
  const totalWeight=makers.reduce((n,m)=>n+BigInt(m.weight),0n);
  if(filledBase===0n||filledBase>totalWeight)throw new Error("Invalid epoch filled base");
  if(side==="ask"&&(totals.baseCredit!=="0"||totals.quoteDebit!=="0"||
     totals.baseFee!=="0")||side==="bid"&&
     (totals.baseDebit!=="0"||totals.quoteCredit!=="0"||
      totals.quoteFee!=="0"))throw new Error("Cross-direction epoch asset totals");
  return Buffer.concat([DOMAIN,Buffer.from(marketHex,"hex"),
    Buffer.from(programHex,"hex"),Buffer.from(genesisHex,"hex"),
    le(epochId,U64,8,"epochId"),le(tickSqrtPriceX64,U128,16,
      "tick sqrt price",{nonzero:true}),Buffer.from([side==="ask"?0:1]),
    count,...membership,...values]);
}
export function hashFrozenEpochV2(spec){
  const encoded=encodeFrozenEpochV2(spec);
  return createHash("sha256").update(encoded).digest("hex");
}
