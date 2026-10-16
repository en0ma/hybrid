// Manifest SDK backed one-maker instruction construction. No signing, broadcasting, or
// synthetic receipts. Execute these instructions only against an audited test fixture.
export function buildManifestMakerInstructions({client,maker,baseAtoms,baseDecimals,priceQuotePerBase,orderId=1,OrderType}){
 if(!client||!maker||!OrderType||OrderType.Limit===undefined)throw Error("Manifest SDK client, maker and Limit order type required");
 if(!Number.isInteger(baseDecimals)||baseDecimals<0||baseDecimals>9)throw Error("Unsupported base mint precision");
 if(typeof baseAtoms!=="string"||! /^[1-9][0-9]*$/.test(baseAtoms))throw Error("baseAtoms must be a positive exact integer string");
 const amount=BigInt(baseAtoms),divisor=10n**BigInt(baseDecimals);
 // SDK wrapper methods currently consume JS token floats, unlike the integer core ABI.
 // Refuse quantities outside exact binary fraction range to avoid biased comparisons.
 if(amount>BigInt(Number.MAX_SAFE_INTEGER)||divisor>BigInt(Number.MAX_SAFE_INTEGER))throw Error("Use integer core instruction API for large quantities");
 const tokenAmount=Number(amount)/Number(divisor);
 if(!Number.isFinite(tokenAmount)||Number.isInteger(tokenAmount*Number(divisor))===false||BigInt(Math.round(tokenAmount*Number(divisor)))!==amount)throw Error("SDK token amount loses integer-atom precision");
 if(typeof priceQuotePerBase!=="number"||!Number.isFinite(priceQuotePerBase)||priceQuotePerBase<=0)throw Error("positive price required");
 if(!Number.isSafeInteger(orderId)||orderId<0)throw Error("invalid order ID");
 const deposit=client.depositIx(maker,client.market.baseMint(),tokenAmount);
 const place=client.placeOrderIx({numBaseTokens:tokenAmount,tokenPrice:priceQuotePerBase,isBid:false,lastValidSlot:0,orderType:OrderType.Limit,clientOrderId:orderId});
 const cancel=client.cancelOrderIx(orderId);
 if(!deposit||!place||!cancel)throw Error("Manifest SDK did not build required instructions");
 return {kind:"manifest-sdk-instructions-unsubmitted",deposit,place,cancel,baseAtoms,orderId,warning:"This is instruction construction, not executed or independently verified benchmark evidence."};
}
