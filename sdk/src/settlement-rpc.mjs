import {decodeMakerBalance} from "./maker-balance.mjs";
import {keyBytes,decodeLazyReserve,decodeFrozenEpoch,reserveVaultFloor} from "./settlement.mjs";
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

export function decodeSettlementCustody(data){
  const b=Buffer.from(data);
  if(b.length!==192||b.subarray(0,8).toString()!=="HYBCUST1"||b[8]!==1||
    b.subarray(12,16).some(Boolean))throw new Error("Invalid custody layout");
  const [market,baseMint,quoteMint,baseVault,quoteVault]=[16,48,80,112,144].map(i=>Buffer.from(b.subarray(i,i+32)));
  if([market,baseMint,quoteMint,baseVault,quoteVault].some(k=>!k.some(Boolean))||
    eq(baseMint,quoteMint)||eq(baseVault,quoteVault))throw new Error("Invalid custody identity");
  return {bump:b[9],baseDecimals:b[10],quoteDecimals:b[11],market,baseMint,quoteMint,baseVault,quoteVault,
    totalBase:b.readBigUInt64LE(176),totalQuote:b.readBigUInt64LE(184)};
}
function token(info,program,mint,authority){
  const b=Buffer.from(owned(info,program));
  if(b.length!==165||b[108]!==1||!eq(b.subarray(0,32),mint)||!eq(b.subarray(32,64),authority))
    throw new Error("Invalid or frozen SPL account");
  return b.readBigUInt64LE(64);
}
function mint(info,program,decimals){
  const b=Buffer.from(owned(info,program));
  if(b.length!==82||b[45]!==1||b[44]!==decimals)throw new Error("Invalid SPL mint");
}
// Second context read repeats every book input. Reject changed data rather
// than combining an old roster with a new balance or reserve snapshot.
export async function authenticateSettlementCollateral(connection,{adapter,programId,market,side,
  headOwnerPage,genesisHex,taker,takerBase,takerQuote,minContextSlot=0,commitment="confirmed"}){
  const snapshot=await discoverSettlementBook(connection,{adapter,programId,market,side,headOwnerPage,
    genesisHex,minContextSlot,commitment});
  const custody=derive(adapter,programId,[seed("custody"),keyBytes(market)]);
  const authority=derive(adapter,programId,[seed("vault-authority"),keyBytes(market)]);
  const tokenProgram=adapter.fromBytes(Buffer.from("06ddf6e1d765a193d9cbe146ceeb79ac1cb485ed5f5b37913a8cf5857eff00a9","hex"));
  const first=await fetchBatch(connection,[custody.key],snapshot.slot,commitment);
  const initial=decodeSettlementCustody(owned(first.values[0],programId));
  const addresses=[initial.baseVault,initial.quoteVault,initial.baseMint,initial.quoteMint].map(k=>adapter.fromBytes(k));
  const bookKeys=[market,snapshot.reserve,...snapshot.pages.flatMap(p=>[p.page,p.ownerPage])];
  const pool=derive(adapter,programId,[seed("passive-pool"),keyBytes(market)]);
  const guarded=snapshot.header.aggregateGuardEnabled;
  const keys=[...bookKeys,custody.key,...addresses,takerBase,takerQuote,...snapshot.makerBalances,
    ...(guarded?[pool.key]:[])];
  if(new Set(keys.map(hex)).size!==keys.length)throw new Error("Aliased collateral accounts");
  const read=await fetchBatch(connection,keys,first.slot,commitment);
  const v=read.values,offset=bookKeys.length;
  const header=decodeMarketHeader(owned(v[0],programId));
  const reserve=decodeLazyReserve(owned(v[1],programId));
  if(!header.collateralizedActive||header.flags!==snapshot.header.flags||
    !eq(header.askOwnerKeyBytes,snapshot.header.askOwnerKeyBytes)||
    !header.bidOwnerTagBytes.equals(snapshot.header.bidOwnerTagBytes)||
    header.askCount!==snapshot.header.askCount||header.bidCount!==snapshot.header.bidCount||
    header.nextSequence!==snapshot.header.nextSequence||
    reserve.marketHex!==hex(market)||reserve.genesisHex!==genesisHex||reserve.bump!==snapshot.reserveState.bump||
    reserve.nextEpochId!==snapshot.reserveState.nextEpochId)throw new Error("Book or reserve changed");
  const rows=[];
  for(const [i,pair] of snapshot.pages.entries()){
    const page=decodeOrderPage(owned(v[2+i*2],programId),side);
    if(page.links.pageIndex!==i||page.links.prevPage!==(i?0:null)||
      page.links.nextPage!==(i+1<snapshot.pages.length?i+1:null))throw new Error("Page links changed");
    rows.push(...decodeOwnerPage(owned(v[3+i*2],programId),page));
  }
  if(rows.length!==snapshot.orders.length||rows.some((r,i)=>{
    const o=snapshot.orders[i];return !eq(r.owner,o.owner)||r.sequence!==o.sequence||
      r.baseQty!==o.baseQty||r.priceX64!==o.priceX64||r.sqrtPriceX64!==o.sqrtPriceX64;
  }))throw new Error("Book changed during discovery");
  const state=decodeSettlementCustody(owned(v[offset],programId));
  if(state.bump!==custody.bump||!eq(state.market,market)||
    ![state.baseVault,state.quoteVault,state.baseMint,state.quoteMint].every((k,i)=>eq(k,addresses[i])))
    throw new Error("Custody domain changed");
  const [baseVault,quoteVault,baseMint,quoteMint]=addresses;
  const base=token(v[offset+1],tokenProgram,baseMint,authority.key);
  const quote=token(v[offset+2],tokenProgram,quoteMint,authority.key);
  mint(v[offset+3],tokenProgram,state.baseDecimals);mint(v[offset+4],tokenProgram,state.quoteDecimals);
  const takerBaseAmount=token(v[offset+5],tokenProgram,baseMint,taker);
  const takerQuoteAmount=token(v[offset+6],tokenProgram,quoteMint,taker);
  const floor=reserveVaultFloor(reserve,state.totalBase,state.totalQuote);
  let aggregateGuard;
  if(guarded){
    const info=v.at(-1);
    // A passive-enabled market has a canonical pool; reject missing/foreign
    // state rather than silently dropping LP liabilities from the floor.
    const b=Buffer.from(owned(info,programId));
    if(b.length!==112||b.subarray(0,8).toString("ascii")!=="HYBPOOL1"||
      b[8]!==1||b[9]!==pool.bump||b.subarray(10,16).some(Boolean)||
      !eq(b.subarray(16,48),market)||b.subarray(96).some(Boolean))
      throw new Error("Invalid passive pool domain");
    floor.base+=b.readBigUInt64LE(48)+b.readBigUInt64LE(80);
    floor.quote+=b.readBigUInt64LE(56)+b.readBigUInt64LE(88);
    aggregateGuard={pool:pool.key,reserve:snapshot.reserve,baseVault,quoteVault};
  }
  if(base<floor.base||quote<floor.quote)throw new Error("Vault deficit");
  const balances=rows.map((r,i)=>{
    const expected=derive(adapter,programId,[seed("maker-balance"),keyBytes(market),r.owner]);
    const b=decodeMakerBalance(owned(v[offset+7+i],programId),{market,maker:r.owner});
    const required=side==="ask"?r.baseQty:(r.baseQty*r.priceX64+(1n<<64n)-1n)>>64n;
    if(b.bump!==expected.bump||!eq(expected.key,snapshot.makerBalances[i])||
      (side==="ask"?b.lockedBase:b.lockedQuote)<required)throw new Error("Maker collateral deficit");
    return b;
  });
  return {...snapshot,header,aggregateGuard,slot:read.slot,reserveState:reserve,custody:custody.key,custodyState:state,
    baseVault,quoteVault,baseMint,quoteMint,vaultAuthority:authority.key,tokenProgram,taker,takerBase,takerQuote,
    balances,vaultAmounts:{base,quote},takerAmounts:{base:takerBaseAmount,quote:takerQuoteAmount},floor};
}
