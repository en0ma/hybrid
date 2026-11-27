// Coherent, observation-only maker book and inventory snapshot. The
// adapter must be a trusted Solana PublicKey implementation.
import {keyBytes,decodeLazyReserve,reserveVaultFloor} from "./settlement.mjs";
import {decodeSettlementCustody} from "./settlement-rpc.mjs";
import {decodeMarketHeader,decodeOrderPage} from "./state.mjs";
import {decodeMakerBalance} from "./maker-balance.mjs";
import {loadLinkedBookSnapshot} from "./multipage.mjs";

const id=k=>keyBytes(k).toString("hex");
const eq=(a,b)=>id(a)===id(b);
const slotOk=n=>Number.isSafeInteger(n)&&n>=0;
const Q=1n<<64n;
const MAX64=Q-1n;
const SYSTEM=Buffer.alloc(32);
const TOKEN=Buffer.from("06ddf6e1d765a193d9cbe146ceeb79ac1cb485ed5f5b37913a8cf5857eff00a9","hex");
function genesisBytes(value){
  const alphabet="123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
  let n=0n;
  for(const ch of value)n=n*58n+BigInt(alphabet.indexOf(ch));
  const out=[];
  while(n){out.unshift(Number(n&255n));n>>=8n;}
  for(const ch of value){if(ch!=="1")break;out.unshift(0);}
  if(out.length!==32)throw new Error("Invalid genesis hash length");
  return Buffer.from(out);
}
function optionalLedger(info,programId){
  if(info===null)return null;
  if(info&&!info.executable&&eq(info.owner,SYSTEM)&&
    info.data instanceof Uint8Array&&info.data.length===0)return null;
  return owned(info,programId);
}
function vaultAmount(info,mint,authority){
  const b=owned(info,TOKEN);
  if(b.length!==165||b[108]!==1||!eq(b.subarray(0,32),mint)||
    !eq(b.subarray(32,64),authority))throw new Error("Invalid or frozen SPL vault");
  // The program does not delegate its canonical vault authority.
  if(b.readUInt32LE(72)!==0||b.readUInt32LE(129)!==0)
    throw new Error("Unexpected SPL vault delegate or close authority");
  return b.readBigUInt64LE(64);
}
function verifyMint(info,decimals){
  const b=owned(info,TOKEN);
  if(b.length!==82||b[45]!==1||b[44]!==decimals)throw new Error("Invalid SPL mint");
}
async function batch(connection,keys,commitment,minContextSlot){
  if(new Set(keys.map(id)).size!==keys.length)throw new Error("Aliased snapshot accounts");
  const response=await connection.getMultipleAccountsInfoAndContext(keys,{commitment,minContextSlot});
  const slot=response?.context?.slot;
  if(!slotOk(slot)||slot<minContextSlot||!Array.isArray(response.value)||
    response.value.length!==keys.length)throw new Error("Stale or malformed maker snapshot");
  return {slot,values:response.value};
}
function derive(adapter,programId,seeds){
  const [key,bump]=adapter.findProgramAddressSync(seeds,programId);
  keyBytes(key);
  if(!Number.isInteger(bump)||bump<0||bump>255)throw new Error("Invalid PDA bump");
  return {key,bump};
}
function owned(info,programId){
  if(!info||info.executable||!eq(info.owner,programId)||
    !(info.data instanceof Uint8Array))throw new Error("Missing or foreign live account");
  return Buffer.from(info.data);
}

/** Discover vault/mint identities from custody, then re-read custody with
 * all canonical candidate pages, both ledgers, vaults and maker inventory
 * in one context. Traverse only the linked prefix.
 * The configured full head sidecar keys must match the market binding.
 * This deliberately cannot authorize, sign or submit a transaction.
 */
export async function loadLiveMakerSnapshot(connection,{
  adapter,programId,market,maker,askHeadOwner,bidHeadOwner,
  expectedGenesisHash,minContextSlot=0,maxLagSlots=4,commitment="confirmed",
}){
  if(!connection||typeof connection.getGenesisHash!=="function"||
    typeof connection.getSlot!=="function"||
    typeof connection.getMultipleAccountsInfoAndContext!=="function"||
    typeof adapter?.findProgramAddressSync!=="function"||typeof adapter?.fromBytes!=="function")
    throw new TypeError("Trusted PDA adapter and context RPC required");
  if(typeof expectedGenesisHash!=="string"||
    !/^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(expectedGenesisHash))
    throw new Error("Pinned genesis required");
  if(!slotOk(minContextSlot)||!slotOk(maxLagSlots)||
    !["confirmed","finalized"].includes(commitment))throw new Error("Invalid snapshot policy");
  const genesis=genesisBytes(expectedGenesisHash);
  if(await connection.getGenesisHash()!==expectedGenesisHash)throw new Error("RPC genesis mismatch");
  const marketBytes=keyBytes(market),makerBytes=keyBytes(maker);
  const custody=derive(adapter,programId,[Buffer.from("custody"),marketBytes]);
  const pool=derive(adapter,programId,[Buffer.from("passive-pool"),marketBytes]);
  const reserve=derive(adapter,programId,[Buffer.from("lazy-reserve"),marketBytes]);
  const authority=derive(adapter,programId,[Buffer.from("vault-authority"),marketBytes]);
  const discovery=await batch(connection,[custody.key],commitment,minContextSlot);
  const initial=decodeSettlementCustody(owned(discovery.values[0],programId));
  if(initial.bump!==custody.bump||!eq(initial.market,market))
    throw new Error("Custody domain mismatch");
  const identities=[initial.baseVault,initial.quoteVault,initial.baseMint,initial.quoteMint];
  const [baseVault,quoteVault,baseMint,quoteMint]=identities.map(k=>adapter.fromBytes(k));
  if(![baseVault,quoteVault,baseMint,quoteMint].every((k,i)=>eq(k,identities[i])))
    throw new Error("Invalid adapter key conversion");
  const balance=derive(adapter,programId,[Buffer.from("maker-balance"),marketBytes,makerBytes]);
  const candidates=Object.fromEntries(["ask","bid"].map(side=>[side,
    Array.from({length:8},(_,index)=>{
      const n=Buffer.alloc(4);n.writeUInt32LE(index);
      return {index,
        page:derive(adapter,programId,[Buffer.from(side+"-page"),marketBytes,n]).key,
        ownerPage:index===0?(side==="ask"?askHeadOwner:bidHeadOwner):
          derive(adapter,programId,[Buffer.from(side+"-owner-page"),marketBytes,n]).key};
    })]));
  const keys=[market,balance.key,custody.key,pool.key,reserve.key,baseVault,quoteVault,
    baseMint,quoteMint,...["ask","bid"].flatMap(side=>
      candidates[side].flatMap(p=>[p.page,p.ownerPage]))];
  const read=await batch(connection,keys,commitment,discovery.slot);
  const slot=read.slot;
  const accounts=new Map(keys.map((k,i)=>[id(k),read.values[i]]));
  const get=k=>accounts.get(id(k));
  const header=decodeMarketHeader(owned(get(market),programId));
  if(!header.collateralizedActive)throw new Error("Inactive collateralized market");
  if(!eq(header.askOwnerKeyBytes,askHeadOwner)||
    !header.bidOwnerTagBytes.equals(keyBytes(bidHeadOwner).subarray(0,16)))
    throw new Error("Head sidecar binding mismatch");
  const state=decodeSettlementCustody(owned(get(custody.key),programId));
  if(state.bump!==custody.bump||!eq(state.market,market)||
    ![state.baseVault,state.quoteVault,state.baseMint,state.quoteMint]
      .every((k,i)=>eq(k,identities[i]))||
    state.baseDecimals!==initial.baseDecimals||state.quoteDecimals!==initial.quoteDecimals)
    throw new Error("Custody identity changed during discovery");
  verifyMint(get(baseMint),state.baseDecimals);verifyMint(get(quoteMint),state.quoteDecimals);
  const vaultAmounts={base:vaultAmount(get(baseVault),baseMint,authority.key),
    quote:vaultAmount(get(quoteVault),quoteMint,authority.key)};
  const reserveData=optionalLedger(get(reserve.key),programId);
  const reserveState=reserveData?decodeLazyReserve(reserveData):null;
  if(reserveState&&(reserveState.bump!==reserve.bump||reserveState.marketHex!==id(market)||
    reserveState.genesisHex!==genesis.toString("hex")))throw new Error("Lazy reserve domain mismatch");
  const floor=reserveState?reserveVaultFloor(reserveState,state.totalBase,state.totalQuote):
    {base:state.totalBase,quote:state.totalQuote};
  const poolData=optionalLedger(get(pool.key),programId);
  if(header.aggregateGuardEnabled&&!poolData)throw new Error("Guarded market missing passive pool");
  if(poolData){
    if(!header.aggregateGuardEnabled||poolData.length!==112||
      poolData.subarray(0,8).toString("ascii")!=="HYBPOOL1"||
      poolData[8]!==1||poolData[9]!==pool.bump||poolData.subarray(10,16).some(Boolean)||
      !eq(poolData.subarray(16,48),market)||poolData.subarray(96).some(Boolean))
      throw new Error("Invalid passive pool domain");
    floor.base+=poolData.readBigUInt64LE(48)+poolData.readBigUInt64LE(80);
    floor.quote+=poolData.readBigUInt64LE(56)+poolData.readBigUInt64LE(88);
  }
  if(floor.base>MAX64||floor.quote>MAX64||vaultAmounts.base<floor.base||
    vaultAmounts.quote<floor.quote)throw new Error("Aggregate vault deficit or overflow");
  const results={};
  for(const side of ["ask","bid"]){
    const pages=[];
    for(const pair of candidates[side]){
      const page=decodeOrderPage(owned(get(pair.page),programId),side);
      owned(get(pair.ownerPage),programId);
      if(page.links.pageIndex!==pair.index||
        page.links.prevPage!==(pair.index===0?null:pair.index-1))
        throw new Error("Broken live page chain");
      pages.push(pair);
      if(page.links.nextPage===null)break;
      if(page.links.nextPage!==pair.index+1||pair.index===7)
        throw new Error("Unsupported or incomplete live page chain");
    }
    // Reuse the established linked-book validator on the exact same batch.
    const fixedRpc={async getMultipleAccountsInfoAndContext(request,opts){
      if(opts.commitment!==commitment||opts.minContextSlot!==minContextSlot)
        throw new Error("Inconsistent decoder context");
      return {context:{slot},value:request.map(get)};
    }};
    results[side]=await loadLinkedBookSnapshot(fixedRpc,{
      programId,market,side,pages,minContextSlot,commitment,
      verifyAddresses:({index,page,ownerPage})=>{
        const expected=candidates[side][index];
        return Boolean(expected&&eq(page,expected.page)&&eq(ownerPage,expected.ownerPage));
      },
    });
  }
  const all=[...results.ask.orders.map(o=>({...o,side:"ask"})),
    ...results.bid.orders.map(o=>({...o,side:"bid"}))];
  const sequences=new Set();
  for(const o of all){
    const seq=String(o.sequence);
    if(o.sequence>=header.nextSequence||sequences.has(seq))
      throw new Error("Invalid or duplicate global order sequence");
    sequences.add(seq);
  }
  const inventory=decodeMakerBalance(owned(get(balance.key),programId),{market,maker});
  if(inventory.bump!==balance.bump)throw new Error("Maker balance bump mismatch");
  const ownedOrders=all.filter(o=>o.owner.equals(makerBytes));
  let requiredBase=0n,requiredQuote=0n;
  for(const order of ownedOrders){
    if(order.side==="ask")requiredBase+=order.baseQty;
    else requiredQuote+=(order.baseQty*order.priceX64+Q-1n)/Q;
  }
  if(inventory.lockedBase<requiredBase||inventory.lockedQuote<requiredQuote)
    throw new Error("Maker locked collateral deficit");
  // Lazy fills debit custody immediately, but leave nominal maker
  // balances unchanged until their claims settle. Global pending debits
  // bound nominal inventory; they are not maker-specific spendable funds.
  const nominalBound={base:state.totalBase+(reserveState?.makerBaseDebit??0n),
    quote:state.totalQuote+(reserveState?.makerQuoteDebit??0n)};
  if(inventory.freeBase+inventory.lockedBase>nominalBound.base||
    inventory.freeQuote+inventory.lockedQuote>nominalBound.quote)
    throw new Error("Maker inventory exceeds custody liabilities and pending debits");
  const pendingLazySettlement=Boolean(reserveState&&[
    reserveState.makerBaseDebit,reserveState.makerQuoteDebit,
    reserveState.makerBaseCredit,reserveState.makerQuoteCredit,
  ].some(v=>v!==0n));
  const observedSlot=await connection.getSlot(commitment);
  if(!slotOk(observedSlot)||observedSlot<slot||observedSlot-slot>maxLagSlots)
    throw new Error("Maker snapshot is stale");
  return {mode:"observation-only",canSubmit:false,requiresFreshSimulationBeforeSubmission:true,
    slot,observedSlot,header,market,maker,makerBalance:balance.key,
    askPages:results.ask.pages,bidPages:results.bid.pages,
    asks:results.ask.orders,bids:results.bid.orders,ownedOrders,inventory,
    requiredLocked:{base:requiredBase,quote:requiredQuote},
    custody:custody.key,custodyState:state,baseVault,quoteVault,baseMint,quoteMint,
    vaultAuthority:authority.key,vaultAmounts,floor,reserveState,nominalBound,pendingLazySettlement,
    marketFlags:header.flags,aggregateGuard:header.aggregateGuardEnabled?
      {pool:pool.key,reserve:reserve.key,baseVault,quoteVault}:undefined};
}
