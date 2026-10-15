import {PublicKey} from "@solana/web3.js";
const str=(bytes)=>new TextDecoder().decode(bytes);
export function decodeCustody(data) {
  const b=new Uint8Array(data);
  if(b.length!==192||str(b.slice(0,8))!=="HYBCUST1"||b[8]!==1||
    b.slice(12,16).some(v=>v!==0))throw Error("Invalid Hybrid custody header");
  const key=(start)=>new PublicKey(b.slice(start,start+32)).toBase58();
  const view=new DataView(b.buffer,b.byteOffset,b.byteLength);
  const result={
    version:b[8],bump:b[9],baseDecimals:b[10],quoteDecimals:b[11],
    market:key(16),baseMint:key(48),quoteMint:key(80),
    baseVault:key(112),quoteVault:key(144),
    totalBase:view.getBigUint64(176,true).toString(),
    totalQuote:view.getBigUint64(184,true).toString()
  };
  if(result.baseMint===result.quoteMint||result.baseVault===result.quoteVault)
    throw Error("Aliased custody mints or vaults");
  return result;
}
export function verifyCustodyLinks(c,market,baseVault,quoteVault) {
  if(c.market!==market||c.baseVault!==baseVault||c.quoteVault!==quoteVault)
    throw Error("Custody record does not match the supplied market and token vaults");
  return true;
}
