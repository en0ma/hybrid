// Read-only decoders for the pinned Hybrid state v1 binary layout.
// Consumers MUST verify the Solana account owner and derived address before decoding.
function asBuffer(data, bytes, name) {
  const b = Buffer.from(data);
  if (b.length !== bytes) throw new RangeError(`${name} must be ${bytes} bytes`);
  return b;
}
function u128(b, i) {
  return b.readBigUInt64LE(i) | (b.readBigUInt64LE(i + 8) << 64n);
}
export function decodeMarketHeader(data) {
  const b = asBuffer(data, 128, "market");
  if (b.subarray(0,8).toString("ascii") !== "HYBRID01" || b[8] !== 1) {
    throw new Error("Unknown market magic or version");
  }
  const header = {
    bump:b[9],
    flags:b.readUInt16LE(10),
    askCount:b.readUInt32LE(12),
    boundaryCount:b.readUInt32LE(16),
    bidCount:b.readUInt32LE(20),
    nextSequence:b.readBigUInt64LE(32),
    sqrtPriceX64:u128(b,48),
    liquidity:u128(b,64),
    baseLotSize:b.readBigUInt64LE(80),
    quoteLotSize:b.readBigUInt64LE(88),
    askOwnerKeyBytes:Buffer.from(b.subarray(96,128)),
    bidOwnerTagBytes:Buffer.concat([b.subarray(24,32),b.subarray(40,48)]),
  };
  if (!header.nextSequence || !header.sqrtPriceX64 || !header.baseLotSize ||
      !header.quoteLotSize) throw new Error("Invalid market header");
  return { ...header, collateralizedActive: Boolean(header.flags & 1) };
}

function links(b) {
  const raw=b.subarray(2,16);
  if (raw.subarray(0,12).every(x=>x===0)) {
    return {pageIndex:0,prevPage:null,nextPage:null};
  }
  const value=i=>raw.readUInt32LE(i);
  const opt=i=>value(i)===0xffffffff ? null : value(i);
  return {pageIndex:value(0),prevPage:opt(4),nextPage:opt(8)};
}
function parseEntry(b, start) {
  return {
    priceX64:u128(b,start),
    sqrtPriceX64:u128(b,start+16),
    baseQty:b.readBigUInt64LE(start+32),
    sequence:b.readBigUInt64LE(start+40),
  };
}
export function decodeOrderPage(data, side) {
  if (side!=="ask" && side!=="bid") throw new RangeError("Invalid side");
  const b=asBuffer(data,1552,"order page");
  const len=b.readUInt16LE(0);
  if(len>32)throw new Error("Invalid page length");
  const entries=[];
  const sequences=new Set();
  for(let i=0;i<32;i++){
    const e=parseEntry(b,16+i*48);
    if(i>=len){
      if(Object.values(e).some(v=>v!==0n))throw new Error("Nonzero trailing order");
      continue;
    }
    if(!e.priceX64||!e.sqrtPriceX64||!e.baseQty||!e.sequence){
      throw new Error("Invalid order entry");
    }
    // Match hybrid_engine::validate_limit_ask: floor(s*s/Q64) <= price
    // and floor((s+1)*(s+1)/Q64) > price, with checked u128 bounds.
    const max128=(1n<<128n)-1n;
    const spot=(sqrt)=>(sqrt*sqrt)>>64n;
    if(e.sqrtPriceX64===max128||spot(e.sqrtPriceX64)>max128||
       spot(e.sqrtPriceX64+1n)>max128||
       spot(e.sqrtPriceX64)>e.priceX64||
       spot(e.sqrtPriceX64+1n)<=e.priceX64){
      throw new Error("Noncanonical cached sqrt price");
    }
    const key=String(e.sequence);
    if(sequences.has(key))throw new Error("Duplicate order sequence");
    sequences.add(key);
    if(entries.length){
      const p=entries.at(-1);
      const correctlySorted=side==="ask"
        ? e.priceX64>p.priceX64 || (e.priceX64===p.priceX64 && e.sequence>p.sequence)
        : e.priceX64<p.priceX64 || (e.priceX64===p.priceX64 && e.sequence>p.sequence);
      if(!correctlySorted) throw new Error("Out-of-order entries");
    }
    entries.push(e);
  }
  return {side,links:links(b),entries};
}
export function decodeOwnerPage(data, expectedOrderPage) {
  const b=asBuffer(data,1040,"owner page");
  const len=b.readUInt16LE(0);
  if(len!==expectedOrderPage.entries.length)throw new Error("Owner/page length mismatch");
  const pageLinks=links(b);
  if(JSON.stringify(pageLinks)!==JSON.stringify(expectedOrderPage.links)){
    throw new Error("Owner/page link mismatch");
  }
  const owners=[];
  for(let i=0;i<32;i++){
    const owner=Buffer.from(b.subarray(16+i*32,16+(i+1)*32));
    if(i<len){
      if(owner.every(x=>x===0))throw new Error("Empty owner");
      owners.push(owner);
    }else if(owner.some(x=>x!==0)){
      throw new Error("Nonzero trailing owner");
    }
  }
  return expectedOrderPage.entries.map((entry,i)=>({...entry,owner:owners[i]}));
}
