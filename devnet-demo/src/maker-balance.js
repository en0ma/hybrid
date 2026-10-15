import {PublicKey} from "@solana/web3.js";
export function decodeMakerBalance(data,market,owner){
 const b=new Uint8Array(data);
 if(b.length!==112||new TextDecoder().decode(b.slice(0,8))!=="HYBBAL01"||
    b[8]!==1||b.slice(10,16).some(v=>v!==0))
   throw Error("Invalid maker balance header");
 const key=(start)=>new PublicKey(b.slice(start,start+32)).toBase58();
 if(key(16)!==market||key(48)!==owner)throw Error("Maker balance has wrong market or signer binding");
 const v=new DataView(b.buffer,b.byteOffset,b.byteLength);
 return {market,owner,freeBase:v.getBigUint64(80,true).toString(),
   lockedBase:v.getBigUint64(88,true).toString(),
   freeQuote:v.getBigUint64(96,true).toString(),
   lockedQuote:v.getBigUint64(104,true).toString()};
}
