// Research-only collateral accounting model; not an executable on-chain design.
// Each market's buy-side obligations are denominated in the SAME quote mint.
// Shared mode caps aggregate filled obligations, but is not Solana-safe without
// an atomic global ledger and a complete failure/replay policy.
const atoms=v=>{const x=BigInt(v);if(x<0n)throw Error("negative asset quantity");return x};
export function simulateCapital({inventoryAtoms,marketObligationsAtoms,fillSequence=[]}){
 const inventory=atoms(inventoryAtoms);
 const obligations=marketObligationsAtoms.map(atoms);
 if(!obligations.length)throw Error("at least one market required");
 const sum=obligations.reduce((a,b)=>a+b,0n);
 const max=obligations.reduce((a,b)=>a>b?a:b,0n);
 const reservedBaseline=sum;
 const worstCaseSimultaneous=sum;
 const optimisticSharedReserve=max;
 const baselineFullyCollateralized=inventory>=sum;
 const sharedCapacityCandidate=inventory>=max;
 const remaining=[...obligations];let spent=0n;const outcomes=[];
 for(const event of fillSequence){
   if(!Number.isSafeInteger(event.market)||event.market<0||event.market>=remaining.length)throw Error("invalid market index");
   const amount=atoms(event.amountAtoms);
   if(amount===0n)throw Error("zero fill");
   if(remaining[event.market]<amount)throw Error("fill exceeds resting order");
   const accepted=spent+amount<=inventory;
   outcomes.push({market:event.market,amountAtoms:String(amount),accepted});
   if(accepted){spent+=amount;remaining[event.market]-=amount;}
 }
 return {inventoryAtoms:String(inventory),baselineRequiredAtoms:String(reservedBaseline),
 optimisticSharedRequiredAtoms:String(optimisticSharedReserve),
 worstCaseSimultaneousAtoms:String(worstCaseSimultaneous),
 baselineFullyCollateralized,sharedCapacityCandidate,
 overcommitAtoms:String(sum>inventory?sum-inventory:0n),
 spentAtoms:String(spent),remainingInventoryAtoms:String(inventory-spent),outcomes};
}
export function formatReport(report){return JSON.stringify(report,null,2);}
