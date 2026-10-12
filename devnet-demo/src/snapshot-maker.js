// Reproducible non-signing Surfpool snapshot for before/after lifecycle comparison.
import {Connection,PublicKey} from "@solana/web3.js";
import {PROGRAM_ID} from "./abi.js";
import {clusterConfig,assertCluster} from "./cluster.js";
import {decodeMarketHeader} from "./market.js";
import {decodeCustody} from "./custody.js";
import {decodeMakerBalance} from "./maker-balance.js";
const args=process.argv.slice(2);
if(args.length!==2){console.error("Usage: npm run snapshot:maker -- <MARKET> <MAKER>");process.exit(2);}
const mode=process.env.HYBRID_CLUSTER||"surfpool";
const cfg=clusterConfig(mode,process.env.SOLANA_RPC_URL||(mode==="surfpool"?
 "http://127.0.0.1:8899":"https://api.devnet.solana.com"));
const rpc=new Connection(cfg.url,"confirmed");
await assertCluster(rpc,mode);
const [market,maker]=args.map(x=>new PublicKey(x));
const program=new PublicKey(PROGRAM_ID);
const derive=(seed)=>PublicKey.findProgramAddressSync(
 [new TextEncoder().encode(seed),market.toBytes()],program)[0];
const custody=derive("custody");
const balance=PublicKey.findProgramAddressSync(
 [new TextEncoder().encode("maker-balance"),market.toBytes(),maker.toBytes()],program)[0];
const result=await rpc.getMultipleAccountsInfoAndContext([market,custody,balance],"confirmed");
const [m,c,b]=result.value;
if(!m||!m.owner.equals(program)||!c||!c.owner.equals(program))
 throw Error("Market or custody absent / wrong program owner");
const marketState=decodeMarketHeader(m.data);
const custodyState=decodeCustody(c.data);
if(custodyState.market!==market.toBase58())throw Error("Custody market mismatch");
if(b&&!b.owner.equals(program))throw Error("Maker balance has wrong program owner");
const makerState=b?decodeMakerBalance(b.data,market.toBase58(),maker.toBase58()):null;
const report={cluster:mode,rpc:cfg.url,slot:result.context.slot,market:market.toBase58(),
 maker:maker.toBase58(),custody:custody.toBase58(),makerPda:balance.toBase58(),
 marketState:{...marketState,bidOwnerTag:[...marketState.bidOwnerTag]},
 custodyState,makerState,
 warning:"A point-in-time snapshot only. Compare multiple snapshots and actual signed transaction receipts to establish execution and conservation."};
console.log(JSON.stringify(report,null,2));
