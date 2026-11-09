// PURE, NON-EXECUTING pro-rata maker fill allocation prototype.
// Matching does not write maker balances; claims are possible only after
// fill totals are FROZEN for the epoch. No SPL token, custody or PDA actions.
import {hashFrozenEpochV2} from "./pro-rata-epoch-abi.mjs";

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
function digest(epoch){
  return hashFrozenEpochV2({marketHex:epoch.marketHex,programHex:epoch.programHex,
    genesisHex:epoch.genesisHex,
    epochId:String(epoch.epochId),tickSqrtPriceX64:epoch.tickSqrtPriceX64,
    side:epoch.totals.baseDebit!=="0"?"ask":"bid",
    makers:epoch.makers,totals:epoch.totals});
}
function canonical(epoch){
  if(!epoch||epoch.version!==2||epoch.status!=="frozen"||
      !Number.isSafeInteger(epoch.epochId)||epoch.epochId<0||
      typeof epoch.marketHex!=="string"||
      typeof epoch.programHex!=="string"||typeof epoch.genesisHex!=="string"||
      typeof epoch.tickSqrtPriceX64!=="string"||!Array.isArray(epoch.makers)||epoch.makers.length===0||
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
  const ask=epoch.totals.baseDebit!=="0"&&epoch.totals.baseCredit==="0"&&
    epoch.totals.quoteDebit==="0"&&epoch.totals.baseFee==="0";
  const bid=epoch.totals.baseCredit!=="0"&&epoch.totals.baseDebit==="0"&&
    epoch.totals.quoteCredit==="0"&&epoch.totals.quoteFee==="0";
  if(!ask&&!bid)throw new Error("Epoch totals mix settlement directions or have no base fills");
  if(BigInt(epoch.totals.baseDebit)>weight||
      BigInt(epoch.totals.baseCredit)>weight)
    throw new Error("Debited quantity exceeds active epoch weight");
  return epoch;
}
/** A tick/price step has its own immutable maker weights and fill totals.
 * Deterministic rotation of the sorted maker list changes dust priority
 * between epochs. Do not allow makers to enter/exit or claim while live.
 */
export function freezeProRataEpoch({epochId,marketHex,programHex,genesisHex,tickSqrtPriceX64,makers,totals}){
  if(!Array.isArray(makers)||makers.length===0||makers.length>4096)
    throw new RangeError("Invalid maker count");
  const sorted=makers.map(m=>({id:m?.id,weight:m?.weight}))
    .sort((a,b)=>String(a.id)<String(b.id)?-1:String(a.id)>String(b.id)?1:0);
  const shift=Number.isSafeInteger(epochId)&&epochId>=0?epochId%sorted.length:0;
  const rotated=[...sorted.slice(shift),...sorted.slice(0,shift)];
  let weight=0n;
  for(const m of rotated)weight+=atom(m.weight,MAX64,"maker weight");
  const record={version:2,status:"frozen",epochId,marketHex,programHex,genesisHex,
    tickSqrtPriceX64,
    makers:rotated,totalWeight:weight.toString(),
    totals:{...totals}};
  canonical(record);
  const commitment=digest(record);
  return Object.freeze({...record,makers:Object.freeze(rotated.map(m=>Object.freeze(m))),
    totals:Object.freeze(record.totals),commitment});
}
function verify(epoch){
  canonical(epoch);
  if(typeof epoch.commitment!=="string"||epoch.commitment!==digest(epoch))
    throw new Error("Frozen epoch commitment mismatch");
  return epoch;
}
/** The base fill is first allocated by frozen resting weights. Proceeds
 * and fees are then allocated ONLY among makers with allocated base fill,
 * preventing a zero-fill maker from receiving proceeds. All allocations
 * telescope to immutable totals, regardless of later tick movements.
 */
export function frozenMakerClaim(epoch,makerId){
  verify(epoch);
  const W=BigInt(epoch.totalWeight);
  const ask=epoch.totals.baseDebit!=="0";
  const baseField=ask?"baseDebit":"baseCredit";
  const filledBase=BigInt(epoch.totals[baseField]);
  let weightPrefix=0n,fillPrefix=0n;
  for(const maker of epoch.makers){
    const weightNext=weightPrefix+BigInt(maker.weight);
    const filledNext=(filledBase*weightNext)/W;
    const filled=(filledNext-(filledBase*weightPrefix)/W);
    if(maker.id===makerId){
      const claim=Object.fromEntries(ASSETS.map(asset=>[asset,"0"]));
      claim[baseField]=filled.toString();
      // Every maker receives the delta of the cumulative *filled-base*
      // fraction, not the original resting-liquidity fraction.
      for(const asset of ask?["quoteCredit","quoteFee"]:["quoteDebit","baseFee"]){
        const total=BigInt(epoch.totals[asset]);
        claim[asset]=((total*(fillPrefix+filled))/filledBase-
          (total*fillPrefix)/filledBase).toString();
      }
      return Object.freeze(claim);
    }
    fillPrefix+=filled;
    weightPrefix=weightNext;
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
      new Set(ledger.claimed).size!==ledger.claimed.length||
      ledger.claimed.some(id=>!epoch.makers.some(m=>m.id===id)))
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
