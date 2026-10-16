const integer=s=>typeof s==="string"&&/^(0|[1-9][0-9]*)$/.test(s);
export function auditManifestMakerSeat({before,after,makerOwner,filledBaseAtoms,quoteSpentAtoms}){
 const find=(snapshot)=>{if(!Array.isArray(snapshot?.seats))throw Error("Manifest claimed seats missing from market snapshot");const seats=snapshot.seats.filter(x=>x.trader===makerOwner);if(seats.length!==1)throw Error("expected exactly one maker seat");const seat=seats[0];if(!integer(seat.baseFreeAtoms)||!integer(seat.quoteFreeAtoms))throw Error("invalid exact maker seat balances");return seat};
 const a=find(before),b=find(after);
 if(!integer(filledBaseAtoms)||!integer(quoteSpentAtoms))throw Error("invalid trade quantities");
 const baseFreeDelta=BigInt(b.baseFreeAtoms)-BigInt(a.baseFreeAtoms);
 const quoteFreeDelta=BigInt(b.quoteFreeAtoms)-BigInt(a.quoteFreeAtoms);
 if(baseFreeDelta!==0n)throw Error("maker base withdrawable balance changed during isolated ask fill");
 if(quoteFreeDelta!==BigInt(quoteSpentAtoms))throw Error("maker seat quote proceeds differ from taker quote expenditure");
 return {verified:false,makerSeatChecked:true,baseFreeDeltaAtoms:baseFreeDelta.toString(),quoteFreeDeltaAtoms:quoteFreeDelta.toString(),warning:"Only an isolated fee-free ask partial fill with no unrelated seat/account changes is supported"};
}
