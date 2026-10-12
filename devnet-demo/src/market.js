import {PublicKey} from "@solana/web3.js";
export function decodeMarketHeader(data){
 const b=new Uint8Array(data);
 const ascii=new TextDecoder().decode(b.slice(0,8));
 if(b.length!==128||ascii!=="HYBRID01"||b[8]!==1)throw Error("Invalid Rust market header");
 const v=new DataView(b.buffer,b.byteOffset,b.byteLength);
 const flags=v.getUint16(10,true);
 const askCount=v.getUint32(12,true);
 const bidCount=v.getUint32(20,true);
 const nextSequence=v.getBigUint64(32,true);
 const baseLotSize=v.getBigUint64(80,true);
 const quoteLotSize=v.getBigUint64(88,true);
 const askOwner=new PublicKey(b.slice(96,128)).toBase58();
 return {flags,collateralizedActive:(flags&1)!==0,askCount,bidCount,
  nextSequence:nextSequence.toString(),baseLotSize:baseLotSize.toString(),
  quoteLotSize:quoteLotSize.toString(),askOwner};
}
export function verifyMarketActivation(m,askOwner){
 if(!m.collateralizedActive)throw Error("Market is not activated for collateralized orders");
 if(m.askOwner!==askOwner)throw Error("Market ask owner sidecar mismatch");
 if(BigInt(m.baseLotSize)===0n||BigInt(m.quoteLotSize)===0n||BigInt(m.nextSequence)===0n)
  throw Error("Invalid market lots or next sequence");
 return true;
}
