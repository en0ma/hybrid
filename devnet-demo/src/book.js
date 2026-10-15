import {PublicKey} from "@solana/web3.js";
const HEADER=16, CAPACITY=32, ENTRY=48, OWNER=32, NONE=0xffffffff;
function view(data,length){
 const b=new Uint8Array(data);
 if(b.length!==length)throw Error("Invalid book account length");
 return {b,v:new DataView(b.buffer,b.byteOffset,b.byteLength)};
}
function links(v){
 const index=v.getUint32(2,true),prev=v.getUint32(6,true),next=v.getUint32(10,true);
 return {index,prev:prev===NONE?null:prev,next:next===NONE?null:next};
}
function u128(v,o){return v.getBigUint64(o,true)+(v.getBigUint64(o+8,true)<<64n);}
export function decodeOwnedBookPage(pageData,ownerData,side,pageIndex){
 if(!["ask","bid"].includes(side)||![0,1].includes(pageIndex))throw Error("Invalid book side/page index");
 const p=view(pageData,HEADER+CAPACITY*ENTRY),o=view(ownerData,HEADER+CAPACITY*OWNER);
 const n=p.v.getUint16(0,true),on=o.v.getUint16(0,true);
 if(n>CAPACITY||n!==on)throw Error("Book/sidecar count mismatch");
 const a=links(p.v),b=links(o.v);
 if(a.index!==pageIndex||JSON.stringify(a)!==JSON.stringify(b))
  throw Error("Book and sidecar links mismatch");
 const orders=[];
 for(let i=0;i<n;i++){
  const start=HEADER+i*ENTRY,ownerStart=HEADER+i*OWNER;
  const price=u128(p.v,start),sqrt=u128(p.v,start+16);
  const qty=p.v.getBigUint64(start+32,true),seq=p.v.getBigUint64(start+40,true);
  const owner=o.b.slice(ownerStart,ownerStart+OWNER);
  if(!price||!sqrt||!qty||!seq||owner.every(x=>x===0))throw Error("Invalid active order");
  const item={pageIndex,side,priceX64:price.toString(),sqrtPriceX64:sqrt.toString(),
   baseQty:qty.toString(),sequence:seq.toString(),owner:new PublicKey(owner).toBase58()};
  const last=orders.at(-1);
  if(last){
   const lp=BigInt(last.priceX64),ls=BigInt(last.sequence);
   if(side==="ask"?(price<lp||(price===lp&&seq<=ls)):(price>lp||(price===lp&&seq<=ls)))
    throw Error("Book order sort violated");
  }
  orders.push(item);
 }
 for(let i=n;i<CAPACITY;i++){
  if(p.b.subarray(HEADER+i*ENTRY,HEADER+(i+1)*ENTRY).some(x=>x!==0)||
     o.b.subarray(HEADER+i*OWNER,HEADER+(i+1)*OWNER).some(x=>x!==0))
   throw Error("Nonzero inactive book slot");
 }
 return {side,pageIndex,links:a,orders};
}
export function combineOwnedBookPages(pages,side){
 if(pages.length!==2||pages.some((p,i)=>p.side!==side||p.pageIndex!==i))
  throw Error("Expected two canonical pages");
 const [a,b]=pages;
 if(a.links.prev!==null||a.links.next!==1||b.links.prev!==0||b.links.next!==null)
  throw Error("Expected linked page 0/1");
 const orders=[...a.orders,...b.orders],seen=new Set();
 for(let i=0;i<orders.length;i++){
  const v=orders[i],key=v.sequence;
  if(seen.has(key))throw Error("Duplicate book sequence");
  seen.add(key);
  if(i){
   const prior=orders[i-1],p=BigInt(prior.priceX64),s=BigInt(prior.sequence),q=BigInt(v.priceX64),t=BigInt(key);
   if(side==="ask"?(q<p||(q===p&&t<=s)):(q>p||(q===p&&t<=s)))
    throw Error("Cross-page book order violated");
  }
 }
 return orders;
}
export function verifyOwnedOrderTransition(beforeOrders,afterOrders,action,sequence,maker,pageIndex){
 if(!["place","cancel"].includes(action))throw Error("Expected place or cancel");
 const seq=BigInt(sequence).toString(),find=(xs)=>xs.filter(x=>x.sequence===seq);
 const a=find(beforeOrders),b=find(afterOrders);
 if(action==="place"?(a.length!==0||b.length!==1):(a.length!==1||b.length!==0))
  throw Error("Expected order insertion/removal missing");
 const item=action==="place"?b[0]:a[0];
 if(item.owner!==maker||item.pageIndex!==pageIndex)throw Error("Order owner/page mismatch");
 const others=(xs)=>xs.filter(x=>x.sequence!==seq).map(x=>JSON.stringify(x)).sort();
 if(JSON.stringify(others(beforeOrders))!==JSON.stringify(others(afterOrders)))
  throw Error("Unrelated book orders changed");
 return {order:item,sequence:seq,ownerVerified:true,bookIdentityChecked:true};
}
