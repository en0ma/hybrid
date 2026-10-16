// Real Manifest SDK instruction construction; never signs, submits or fabricates receipts.
// Scope: a pre-provisioned, one-maker limit ask and exact atom deposit.
const uint=x=>typeof x==="string"&&/^[1-9][0-9]*$/.test(x)?BigInt(x):(()=>{throw Error("amount must be a positive decimal atom string")})();
export function makeManifestSingleMakerAsk({client,maker,baseAtoms,priceTokens,clientOrderId,OrderType}){
 if(!client||typeof client.depositAtomsIx!=="function"||typeof client.placeOrderIx!=="function")throw Error("Manifest SDK trading client required");
 if(!maker||typeof maker.toBase58!=="function"||!client.market||typeof client.market.baseMint!=="function"||typeof client.market.baseDecimals!=="function")throw Error("Manifest market and maker required");
 const decimals=client.market.baseDecimals();
 if(!Number.isInteger(decimals)||decimals<0||decimals>18)throw Error("invalid mint decimals");
 const atoms=uint(baseAtoms),scale=10n**BigInt(decimals);
 // The SDK's wrapper order entry point accepts token quantities as JS numbers.
 // Refuse any amount that cannot be represented exactly by that interface.
 if(atoms%scale!==0n||atoms/scale>BigInt(Number.MAX_SAFE_INTEGER))throw Error("non-exact SDK token amount; use protocol-specific atom-safe order builder");
 if(!Number.isSafeInteger(priceTokens)||priceTokens<=0)throw Error("priceTokens must be a positive safe integer");
 if(!Number.isSafeInteger(clientOrderId)||clientOrderId<=0)throw Error("invalid client order ID");
 if(!OrderType||OrderType.Limit===undefined)throw Error("Manifest SDK OrderType.Limit required");
 const deposit=client.depositAtomsIx(maker,client.market.baseMint(),atoms);
 const place=client.placeOrderIx({numBaseTokens:Number(atoms/scale),tokenPrice:priceTokens,isBid:false,lastValidSlot:0,orderType:OrderType.Limit,clientOrderId});
 if(!deposit?.programId||!Array.isArray(deposit.keys)||!place?.programId||!Array.isArray(place.keys))throw Error("Manifest SDK failed to build Solana instructions");
 return {kind:"manifest-single-maker-unsigned-instructions",deposit,place,baseAtoms,priceTokens,clientOrderId,limitations:["unsigned; not a transaction receipt","maker wrapper and claimed seat must be provisioned","integer-token-only limit orders until exact atom-safe place API is confirmed"]};
}
