// PURE, NON-EXECUTING pro-rata maker fill allocation prototype.
// Matching does not write maker balances; claims are possible only after
// fill totals are FROZEN for the epoch. No SPL token, custody or PDA actions.
import {createHash} from "node:crypto";

const MAX64=(1n<<64n)-1n, MAX128=(1n<<128n)-1n;
const ASSETS=["baseDebit","baseCredit","quoteDebit","quoteCredit","baseFee","quoteFee"];
const ID=/^[A-Za-z0-9_-]{1,128}$/;
function atom(value,limit,name){
  if(typeof value!=="string"||!/^(0|[1-9][0-9]*)$/.test(value))
    throw new Error("Invalid unsigned integer "+name);
  const result=BigInt(value);
  if(result>limit)throw new RangeError(name+" exceeds precision bound");
  return result;
}
function hash(value){return createHash("sha256").update(JSON.stringify(value)).digest("hex");}
function canonical(epoch){
  if(!epoch||epoch.version!==1||epoch.status!=="frozen"||
      !Number.isSafeInteger(epoch.epochId)||epoch.epochId<0||
      !ID.test(epoch.tickId)||!Array.isArray(epoch.makers)||epoch.makers.length===0||
      !epoch.totals||typeof epoch.totals!=="object")throw new Error("Invalid frozen epoch");
  const ids=new Set();
  let weight=0n;
  for(const maker of epoch.makers){
    if(!maker||!ID.test(maker.id)||ids.has(maker.id))
      throw new Error("Duplicate or invalid maker identity");
    ids.add(maker.id);
    const individual=atom(maker.weight,MAX64,"maker weight");
    if(individual===0n)throw new Error("Zero maker weight");
    weight+=individual;
    if(weight>MAX128)throw new RangeError("Weight accumulator overflow");
  }
  if(weight===0n||atom(epoch.totalWeight,MAX128,"totalWeight")!==weight)
    throw new Error("Unbalanced epoch weights");
  if(JSON.stringify(Object.keys(epoch.totals).sort())!==
      JSON.stringify([...ASSETS].sort()))
    throw new Error("Exact six asset totals required");
  for(const name of ASSETS)atom(epoch.totals[name],MAX64,name);
  if(epoch.totals.baseDebit!=="0"&&epoch.totals.quoteDebit!=="0")
    throw new Error("One epoch cannot mix buy and sell settlement directions");
  if(BigInt(epoch.totals.baseDebit)>weight||
      BigInt(epoch.totals.baseCredit)>weight)
    throw new Error("Debited quantity exceeds active epoch weight");
  return epoch;
}
/** A tick/price step has its own immutable maker weights and fill totals.
 * Deterministic rotation of the sorted maker list changes dust priority
 * between epochs. Do not allow makers to enter/exit or claim while live.
 */
export function freezeProRataEpoch({epochId,tickId,makers,totals}){
  if(!Array.isArray(makers)||makers.length===0||makers.length>4096)
    throw new RangeError("Invalid maker count");
  const sorted=makers.map(m=>({id:m?.id,weight:m?.weight}))
    .sort((a,b)=>String(a.id)<String(b.id)?-1:String(a.id)>String(b.id)?1:0);
  const shift=Number.isSafeInteger(epochId)&&epochId>=0?epochId%sorted.length:0;
  const rotated=[...sorted.slice(shift),...sorted.slice(0,shift)];
  let weight=0n;
  for(const m of rotated)weight+=atom(m.weight,MAX64,"maker weight");
  const record={version:1,status:"frozen",epochId,tickId,
    makers:rotated,totalWeight:weight.toString(),
    totals:{...totals}};
  canonical(record);
  const commitment=hash(record);
  return Object.freeze({...record,makers:Object.freeze(rotated.map(m=>Object.freeze(m))),
    totals:Object.freeze(record.totals),commitment});
}
function verify(epoch){
  canonical(epoch);
  const {commitment,...record}=epoch;
  if(typeof commitment!=="string"||commitment!==hash(record))
    throw new Error("Frozen epoch commitment mismatch");
  return epoch;
}
/** Prefix difference computed ONLY for immutable total, not intermediate F.
 * For each asset: sum_i(floor(T*(p+w)/W)-floor(T*p/W))=T.
 * Floating-point operations and settlement-order-dependent dust forbidden.
 */
export function frozenMakerClaim(epoch,makerId){
  verify(epoch);
  let prefix=0n;
  const W=BigInt(epoch.totalWeight);
  for(const maker of epoch.makers){
    const next=prefix+BigInt(maker.weight);
    if(maker.id===makerId){
      const claim={};
      for(const asset of ASSETS){
        const T=BigInt(epoch.totals[asset]);
        claim[asset]=((T*next)/W-(T*prefix)/W).toString();
      }
      return Object.freeze(claim);
    }
    prefix=next;
  }
  throw new Error("Maker not active in frozen epoch");
}
export function newFrozenSettlementLedger(epoch){
  verify(epoch);
  return Object.freeze({epochCommitment:epoch.commitment,
    claimed:Object.freeze([])});
}
/** Pure idempotency guard; on chain the ledger update AND SPL withdrawal
 * MUST commit atomically. These functions provide neither authorization nor
 * protection against independently submitted stale ledger snapshots.
 */
export function claimFrozenMaker(epoch,ledger,makerId){
  verify(epoch);
  if(!ledger||ledger.epochCommitment!==epoch.commitment||
      !Array.isArray(ledger.claimed)||ledger.claimed.some(id=>!ID.test(id))||
      new Set(ledger.claimed).size!==ledger.claimed.length)
    throw new Error("Invalid or mismatched settlement ledger");
  if(ledger.claimed.includes(makerId))throw new Error("Maker already claimed epoch");
  const claim=frozenMakerClaim(epoch,makerId);
  return Object.freeze({claim,ledger:Object.freeze({
    epochCommitment:epoch.commitment,
    claimed:Object.freeze([...ledger.claimed,makerId]),
  })});
}
export function auditFrozenProRataEpoch(epoch){
  verify(epoch);
  const sums=Object.fromEntries(ASSETS.map(a=>[a,0n]));
  for(const maker of epoch.makers){
    const claim=frozenMakerClaim(epoch,maker.id);
    for(const asset of ASSETS)sums[asset]+=BigInt(claim[asset]);
  }
  for(const asset of ASSETS){
    if(sums[asset]!==BigInt(epoch.totals[asset]))
      throw new Error("Epoch dust conservation failed: "+asset);
  }
  return Object.freeze({mode:"allocation-audit-only",
    commitment:epoch.commitment,makerCount:epoch.makers.length,
    exactlyAllocated:true,canWithdraw:false});
}
