// SPL Token classic mint: 82 bytes; decimals at 44, initialized at 45.
export function verifyClassicMint(data,expectedDecimals){
 const b=new Uint8Array(data);
 if(b.length!==82)throw Error("Invalid classic SPL mint length");
 if(!Number.isInteger(expectedDecimals)||expectedDecimals<0||expectedDecimals>255)
  throw Error("Invalid expected mint decimals");
 if(b[44]!==expectedDecimals)throw Error("SPL mint decimals mismatch");
 if(b[45]!==1)throw Error("SPL mint not initialized");
 return true;
}
