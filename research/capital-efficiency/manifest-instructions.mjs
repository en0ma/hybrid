// Real Manifest SDK instruction builder for the single-maker ask workload.
// No signing, network submission, or fabricated receipts.
const atoms=/^[1-9][0-9]*$/;
export function buildManifestMakerInstructions({client,sdk,maker,baseAtoms,priceTokens,clientOrderId=1}){
 if(!client?.market||typeof client.depositAtomsIx!=="function"||typeof client.placeOrderIx!=="function")throw Error("initialized ManifestClient with wrapper and claimed seat required");
 if(!sdk?.OrderType||sdk.OrderType.Limit===undefined)throw Error("Manifest SDK OrderType.Limit required");
 if(!atoms.test(String(baseAtoms)))throw Error("positive integer base atoms required");
 if(!Number.isFinite(priceTokens)||priceTokens<=0)throw Error("positive finite limit price required");
 if(!Number.isSafeInteger(clientOrderId)||clientOrderId<1)throw Error("positive safe clientOrderId required");
 const decimals=client.market.baseDecimals();
 if(!Number.isInteger(decimals)||decimals<0||decimals>18)throw Error("invalid base decimals");
 const amount=BigInt(baseAtoms);
 // SDK's placeOrderIx takes decimal tokens via JavaScript number.
 // Reject orders not exactly representable after converting atoms to JS Number.
 if(amount>BigInt(Number.MAX_SAFE_INTEGER))throw Error("baseAtoms exceeds exact JS Number range of SDK placeOrderIx");
 const amountTokens=Number(amount)/10**decimals;
 if(!Number.isFinite(amountTokens)||amountTokens<=0||BigInt(Math.round(amountTokens*10**decimals))!==amount)throw Error("atom quantity loses precision in Manifest placeOrderIx");
 const baseMint=client.market.baseMint(),market=client.market.address;
 const deposit=client.depositAtomsIx(maker,baseMint,amount);
 const place=client.placeOrderIx({numBaseTokens:amountTokens,tokenPrice:priceTokens,isBid:false,lastValidSlot:0,orderType:sdk.OrderType.Limit,clientOrderId});
 for(const [label,ix] of [["deposit",deposit],["place",place]]){
  if(!ix||!ix.programId||!Array.isArray(ix.keys)||!ix.keys.some(key=>key.pubkey.equals(market)))throw Error(label+" instruction missing target market");
 }
 return {kind:"manifest-unsigned-maker-instructions",market:market.toBase58(),maker:maker.toBase58(),baseAtoms:String(amount),priceTokens,clientOrderId,instructions:{deposit,place},warning:"Unsigned SDK instructions only; no execution or benchmark result"};
}
export function buildManifestTakerInstruction({client,taker,params}){
 if(!client?.market||typeof client.swapIx!=="function")throw Error("ManifestClient.swapIx required");
 if(typeof params?.isBaseIn!=="boolean")throw Error("swap direction required");
 const ix=client.swapIx(taker,params);
 if(!ix?.keys?.some(key=>key.pubkey.equals(client.market.address)))throw Error("swap instruction missing target market");
 return ix;
}
