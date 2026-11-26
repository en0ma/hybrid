import {keyBytes,decodeLazyReserve,decodeFrozenEpoch} from "./settlement.mjs";
import {decodeMarketHeader,decodeOrderPage,decodeOwnerPage} from "./state.mjs";

const hex=k=>keyBytes(k).toString("hex");
const eq=(a,b)=>hex(a)===hex(b);
const u32=n=>{const b=Buffer.alloc(4);b.writeUInt32LE(n);return b;};
const u64=n=>{const b=Buffer.alloc(8);b.writeBigUInt64LE(n);return b;};
const seed=s=>Buffer.from(s,"ascii");
function derive(adapter,programId,seeds){
  const [key,bump]=adapter.findProgramAddressSync(seeds,programId);
  keyBytes(key);
  if(!Number.isInteger(bump)||bump<0||bump>255)throw new Error("Invalid PDA bump");
  return {key,bump};
}
function owned(info,programId){
  if(!info||!eq(info.owner,programId)||info.executable)throw new Error("Missing or foreign account");
  if(!(info.data instanceof Uint8Array))throw new Error("Nonbinary account data");
  return info.data;
}
async function fetchBatch(connection,keys,minContextSlot,commitment){
  if(!Number.isSafeInteger(minContextSlot)||minContextSlot<0)throw new Error("Invalid slot");
  const r=await connection.getMultipleAccountsInfoAndContext(keys,{minContextSlot,commitment});
  if(!Number.isSafeInteger(r?.context?.slot)||r.context.slot<minContextSlot||
    !Array.isArray(r.value)||r.value.length!==keys.length)throw new Error("Stale or malformed RPC response");
  return {slot:r.context.slot,values:r.value};
}
// Adapter is a trusted Solana PublicKey implementation exposing
// findProgramAddressSync and fromBytes. No caller-supplied PDA addresses.
export async function discoverSettlementBook(connection,{adapter,programId,market,side,
  headOwnerPage,genesisHex,minContextSlot=0,commitment="confirmed"}){
  if(side!=="ask"&&side!=="bid")throw new Error("Invalid side");
  if(!/^[0-9a-f]{64}$/.test(genesisHex??""))throw new Error("Pinned genesis required");
  const marketBytes=keyBytes(market);
  const reserve=derive(adapter,programId,[seed("lazy-reserve"),marketBytes]);
  const pages=[0,1].map(index=>({index,
    page:derive(adapter,programId,[seed(side+"-page"),marketBytes,u32(index)]).key,
    ownerPage:index===0?headOwnerPage:
      derive(adapter,programId,[seed(side+"-owner-page"),marketBytes,u32(index)]).key}));
  const keys=[market,reserve.key,pages[0].page,pages[0].ownerPage,pages[1].page,pages[1].ownerPage];
  const read=await fetchBatch(connection,keys,minContextSlot,commitment);
  const header=decodeMarketHeader(owned(read.values[0],programId));
  const reserveState=decodeLazyReserve(owned(read.values[1],programId));
  if(!header.collateralizedActive||reserveState.marketHex!==hex(market)||
    reserveState.genesisHex!==genesisHex||reserveState.bump!==reserve.bump)throw new Error("Reserve domain mismatch");
  const binding=side==="ask"?header.askOwnerKeyBytes:header.bidOwnerTagBytes;
  if(!binding.equals(keyBytes(headOwnerPage).subarray(0,binding.length)))throw new Error("Head sidecar mismatch");
  const head=decodeOrderPage(owned(read.values[2],programId),side);
  if(head.links.pageIndex!==0||head.links.prevPage!==null||
    ![null,1].includes(head.links.nextPage))throw new Error("Unsupported head links");
  const count=head.links.nextPage===1?2:1;
  const orders=[];
  for(let i=0;i<count;i++){
    const book=decodeOrderPage(owned(read.values[2+i*2],programId),side);
    if(book.links.pageIndex!==i||book.links.prevPage!==(i?0:null)||
      book.links.nextPage!==(i===0&&count===2?1:null)||!book.entries.length)
      throw new Error("Broken page chain");
    orders.push(...decodeOwnerPage(owned(read.values[3+i*2],programId),book));
  }
  if(orders.length!==(side==="ask"?header.askCount:header.bidCount)||
    orders.length>(side==="ask"?8:16))throw new Error("Unsupported book roster");
  const owners=new Set(),sequences=new Set();
  for(let i=0;i<orders.length;i++){
    const o=orders[i],owner=hex(o.owner);
    if(owners.has(owner)||sequences.has(String(o.sequence)))throw new Error("Duplicate roster row");
    owners.add(owner);sequences.add(String(o.sequence));
    if(i){
      const prev=orders[i-1];
      if((side==="ask"?o.priceX64<prev.priceX64:o.priceX64>prev.priceX64)||
        (o.priceX64===prev.priceX64&&o.sequence<=prev.sequence))throw new Error("Unsorted chain");
    }
  }
  const makerBalances=orders.map(o=>derive(adapter,programId,[seed("maker-balance"),marketBytes,o.owner]).key);
  const epochs=Array.from({length:count},(_,i)=>derive(adapter,programId,
    [seed("frozen-epoch"),marketBytes,u64(reserveState.nextEpochId+BigInt(i))]).key);
  return {slot:read.slot,market,side,reserve:reserve.key,reserveState,header,pages:pages.slice(0,count),
    orders,makerBalances,epochs};
}
export async function loadSettlementEpochs(connection,{adapter,programId,market,genesisHex,
  epochIds,minContextSlot=0,commitment="finalized"}){
  if(!Array.isArray(epochIds)||!epochIds.length||epochIds.length>64||
    new Set(epochIds.map(String)).size!==epochIds.length)throw new Error("Invalid epoch batch");
  const marketBytes=keyBytes(market);
  const reserve=derive(adapter,programId,[seed("lazy-reserve"),marketBytes]);
  const addresses=epochIds.map(n=>derive(adapter,programId,[seed("frozen-epoch"),marketBytes,u64(BigInt(n))]));
  const r=await fetchBatch(connection,[reserve.key,...addresses.map(a=>a.key)],minContextSlot,commitment);
  const state=decodeLazyReserve(owned(r.values[0],programId));
  if(state.bump!==reserve.bump||state.marketHex!==hex(market)||state.genesisHex!==genesisHex)
    throw new Error("Reserve domain mismatch");
  const epochs=addresses.map((a,i)=>{
    const e=decodeFrozenEpoch(owned(r.values[i+1],programId));
    if(e.bump!==a.bump||e.epochId!==BigInt(epochIds[i])||e.epochId>=state.nextEpochId||
      e.marketHex!==hex(market)||e.authorityHex!==hex(market)||e.programHex!==hex(programId)||
      e.genesisHex!==genesisHex)throw new Error("Epoch domain mismatch");
    return {...e,address:a.key};
  });
  return {slot:r.slot,reserve:state,epochs};
}
