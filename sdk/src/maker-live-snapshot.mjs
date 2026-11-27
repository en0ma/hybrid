// Coherent, observation-only maker book and inventory snapshot. The
// adapter must be a trusted Solana PublicKey implementation.
import {keyBytes} from "./settlement.mjs";
import {decodeMarketHeader,decodeOrderPage} from "./state.mjs";
import {decodeMakerBalance} from "./maker-balance.mjs";
import {loadLinkedBookSnapshot} from "./multipage.mjs";

const id=k=>keyBytes(k).toString("hex");
const eq=(a,b)=>id(a)===id(b);
const slotOk=n=>Number.isSafeInteger(n)&&n>=0;
const Q=1n<<64n;
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

/** Fetch all canonical candidate pages once. Traverse only the linked prefix;
 * both sides and maker collateral must come from the same RPC context.
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
    typeof adapter?.findProgramAddressSync!=="function")
    throw new TypeError("Trusted PDA adapter and context RPC required");
  if(typeof expectedGenesisHash!=="string"||
    !/^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(expectedGenesisHash))
    throw new Error("Pinned genesis required");
  if(!slotOk(minContextSlot)||!slotOk(maxLagSlots)||
    !["confirmed","finalized"].includes(commitment))throw new Error("Invalid snapshot policy");
  if(await connection.getGenesisHash()!==expectedGenesisHash)throw new Error("RPC genesis mismatch");
  const marketBytes=keyBytes(market),makerBytes=keyBytes(maker);
  const balance=derive(adapter,programId,[Buffer.from("maker-balance"),marketBytes,makerBytes]);
  const candidates=Object.fromEntries(["ask","bid"].map(side=>[side,
    Array.from({length:8},(_,index)=>{
      const n=Buffer.alloc(4);n.writeUInt32LE(index);
      return {index,
        page:derive(adapter,programId,[Buffer.from(side+"-page"),marketBytes,n]).key,
        ownerPage:index===0?(side==="ask"?askHeadOwner:bidHeadOwner):
          derive(adapter,programId,[Buffer.from(side+"-owner-page"),marketBytes,n]).key};
    })]));
  const keys=[market,balance.key,...["ask","bid"].flatMap(side=>
    candidates[side].flatMap(p=>[p.page,p.ownerPage]))];
  if(new Set(keys.map(id)).size!==keys.length)throw new Error("Aliased snapshot accounts");
  const response=await connection.getMultipleAccountsInfoAndContext(keys,{commitment,minContextSlot});
  const slot=response?.context?.slot;
  if(!slotOk(slot)||slot<minContextSlot||!Array.isArray(response.value)||
    response.value.length!==keys.length)throw new Error("Stale or malformed maker snapshot");
  const accounts=new Map(keys.map((k,i)=>[id(k),response.value[i]]));
  const get=k=>accounts.get(id(k));
  const header=decodeMarketHeader(owned(get(market),programId));
  if(!header.collateralizedActive)throw new Error("Inactive collateralized market");
  if(!eq(header.askOwnerKeyBytes,askHeadOwner)||
    !header.bidOwnerTagBytes.equals(keyBytes(bidHeadOwner).subarray(0,16)))
    throw new Error("Head sidecar binding mismatch");
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
  const observedSlot=await connection.getSlot(commitment);
  if(!slotOk(observedSlot)||observedSlot<slot||observedSlot-slot>maxLagSlots)
    throw new Error("Maker snapshot is stale");
  return {mode:"observation-only",canSubmit:false,requiresFreshSimulationBeforeSubmission:true,
    slot,observedSlot,header,market,maker,makerBalance:balance.key,
    askPages:results.ask.pages,bidPages:results.bid.pages,
    asks:results.ask.orders,bids:results.bid.orders,ownedOrders,inventory,
    requiredLocked:{base:requiredBase,quote:requiredQuote}};
}
