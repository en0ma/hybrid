// Deterministic non-signing reference strategy. Price units are quote atoms per base atom;
// no float arithmetic, exchange rates or live signals are inferred.
export function planQuotes({midPriceAtoms,spreadBps,levels=1,stepBps=25,sizeAtoms,maxBaseAtoms,maxQuoteAtoms,stale=false}){
const number=(n,name)=>{if(!Number.isInteger(n)||n<0)throw Error("invalid "+name)};
for(const [name,v] of Object.entries({spreadBps,levels,stepBps}))number(v,name);
if(spreadBps<1||spreadBps>=10000||levels<1||levels>8||stepBps>10000)throw Error("strategy limits violated");
if(stale)throw Error("refuse stale market snapshot");
const mid=BigInt(midPriceAtoms),size=BigInt(sizeAtoms),base=BigInt(maxBaseAtoms),quote=BigInt(maxQuoteAtoms);
if(mid<=0n||size<=0n||base<0n||quote<0n)throw Error("invalid inventory");
let usedBase=0n,usedQuote=0n;const orders=[];
for(let i=0;i<levels;i++){const offset=BigInt(spreadBps)+BigInt(i*stepBps);
if(offset>=10000n)break;
const bid=mid*(10000n-offset)/10000n,ask=(mid*(10000n+offset)+9999n)/10000n;
if(bid>0n){const quoteCost=bid*size;if(usedQuote+quoteCost<=quote){orders.push({side:"bid",level:i,priceAtoms:bid.toString(),sizeAtoms:size.toString()});usedQuote+=quoteCost}}
if(usedBase+size<=base){orders.push({side:"ask",level:i,priceAtoms:ask.toString(),sizeAtoms:size.toString()});usedBase+=size}
}
return {orders,reservedBaseAtoms:usedBase.toString(),reservedQuoteAtoms:usedQuote.toString(),requiresFreshSnapshotAndSimulation:true};
}
