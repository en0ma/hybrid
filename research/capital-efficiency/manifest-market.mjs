// Read-only Manifest market adapter based on the official Manifest SDK Market API.
// Never creates an order, signs, or claims a workload has been executed.
const hash=/^[0-9a-f]{40}$/i;
export async function snapshotManifestMarket({connection,address,Market,PublicKey,expectedProgramId,expectedBaseMint,expectedQuoteMint,commit}){
 if(!hash.test(commit??""))throw Error("Pin the exact Manifest source commit");
 if(!expectedProgramId||!expectedBaseMint||!expectedQuoteMint)throw Error("Expected program and both token mints required");
 const pk=new PublicKey(address),program=new PublicKey(expectedProgramId);
 const info=await connection.getAccountInfoAndContext(pk,"confirmed");
 if(!info?.value||!info.value.owner.equals(program))throw Error("Manifest market missing or unexpected owner");
 const market=Market.loadFromBuffer({address:pk,buffer:info.value.data,slot:info.context.slot});
 if(market.baseMint().toBase58()!==expectedBaseMint||market.quoteMint().toBase58()!==expectedQuoteMint)throw Error("Manifest mint pair mismatch");
 const normalize=o=>({trader:o.trader.toBase58(),baseAtoms:String(o.numBaseAtoms),priceRaw:String(o.price),sequence:String(o.sequenceNumber),orderType:o.orderType});
 return {kind:"manifest-market-snapshot-readonly",commit,programId:program.toBase58(),market:pk.toBase58(),slot:info.context.slot,
  baseMint:market.baseMint().toBase58(),quoteMint:market.quoteMint().toBase58(),
  baseDecimals:market.baseDecimals(),quoteDecimals:market.quoteDecimals(),
  asks:market.asks().map(normalize),bids:market.bids().map(normalize),
  seats:market.claimedSeats().map(seat=>({trader:seat.publicKey.toBase58(),baseFreeAtoms:String(seat.baseBalance),quoteFreeAtoms:String(seat.quoteBalance)})),
  warning:"Read-only account snapshot, not an executed benchmark, transaction confirmation or global balance audit"};
}
export async function readManifestMarket(config,{sdkLoader=()=>import("@bonasa-tech/manifest-sdk"),web3Loader=()=>import("@solana/web3.js")}={}){
 const url=new URL(config.rpc);if(url.protocol!=="https:"&&!(url.protocol==="http:"&&["127.0.0.1","localhost"].includes(url.hostname)))throw Error("Require HTTPS remote or explicit loopback local RPC");
 const [{Market}, {Connection,PublicKey}]=await Promise.all([sdkLoader(),web3Loader()]);
 const connection=new Connection(config.rpc,"confirmed");
 return snapshotManifestMarket({...config,connection,Market,PublicKey});
}
