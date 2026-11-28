// Explicit, single-step operator CLI; no private keys, signing or polling loop.
import * as web3 from "@solana/web3.js";
import {readFile,stat} from "node:fs/promises";
import {isAbsolute} from "node:path";
import {fileURLToPath} from "node:url";
import {PROGRAM_ID} from "./abi.js";
import {createSolanaSettlementAdapter} from "../../sdk/src/settlement-solana.mjs";
import {loadLiveMakerSnapshot} from "../../sdk/src/maker-live-snapshot.mjs";
import {planLiveMakerQuotes} from "../../sdk/src/maker-live-plan.mjs";
import {initializeLiveMakerSession,inspectLiveMakerSession,reserveLiveMakerIntent,
 prepareLiveMakerIntent,submitSandboxMakerIntent,finalizeSandboxMakerIntent,
 reconcileSandboxMakerIntent,renewSandboxMakerRateWindow,reconcileFailedSandboxMakerIntent} from "../../sdk/src/maker-live-session.mjs";
const {PublicKey,Connection,VersionedTransaction}=web3;
const adapter=createSolanaSettlementAdapter(web3);
const commands=new Set(["observe","plan","inspect","sandbox-init","sandbox-reserve","sandbox-prepare","sandbox-submit","sandbox-finalize","sandbox-reconcile","sandbox-recover-failed","sandbox-renew"]);
const signedCommands=new Set(["sandbox-prepare","sandbox-submit"]);
const json=value=>JSON.stringify(value,(_,v)=>typeof v==="bigint"?String(v):v,2);
async function boundedText(path,max){
 const info=await stat(path);
 if(!info.isFile()||info.size>max)throw Error("Expected bounded regular file");
 const data=await readFile(path,"utf8");
 if(Buffer.byteLength(data)>max)throw Error("Oversized input");
 return data;
}
export function parseMakerConfig(config,command,env=process.env){
 if(!commands.has(command)||!config||typeof config!=="object"||Array.isArray(config))
  throw Error("Invalid maker command/config");
 const allowed=new Set(["rpcUrl","expectedGenesisHash","market","maker","askHeadOwner","bidHeadOwner",
  "sessionPath","policy","orders","acknowledgeEmptySession","isolatedTestWalletAcknowledged"]);
 if(Object.keys(config).some(k=>!allowed.has(k)))throw Error("Unexpected maker config field");
 const url=new URL(config.rpcUrl);
 if(!["http:","https:"].includes(url.protocol)||url.username||url.password||url.hash||url.search)
  throw Error("Invalid RPC URL");
 const sandbox=command.startsWith("sandbox-");
 if(sandbox&&(url.protocol!=="http:"||!["127.0.0.1","[::1]"].includes(url.hostname)||
   !url.port||url.pathname!=="/"||config.isolatedTestWalletAcknowledged!==true||
   env.HYBRID_SANDBOX_EXECUTION!=="1"||env.HYBRID_MAKER_KILL_SWITCH!=="off"))
  throw Error("Sandbox requires literal loopback, isolated test-wallet acknowledgement and explicit execution switches");
 const snapshotOptions={adapter,programId:new PublicKey(PROGRAM_ID),market:new PublicKey(config.market),
  maker:new PublicKey(config.maker),askHeadOwner:new PublicKey(config.askHeadOwner),
  bidHeadOwner:new PublicKey(config.bidHeadOwner),expectedGenesisHash:config.expectedGenesisHash};
 if(typeof config.expectedGenesisHash!=="string"||
   new PublicKey(config.expectedGenesisHash).toBase58()!==config.expectedGenesisHash)
  throw Error("Invalid pinned genesis");
 if((sandbox||command==="inspect")&&
   (typeof config.sessionPath!=="string"||!isAbsolute(config.sessionPath)))
  throw Error("Absolute private session path required");
 if(["plan","sandbox-reserve"].includes(command)&&!Array.isArray(config.orders))
  throw Error("Explicit desired orders required");
 // Process-level kill switch is an operator snapshot, not cross-process
 // realtime signaling. No background execution is provided.
 return {url:url.href,snapshotOptions,sessionPath:config.sessionPath,orders:config.orders,
  policy:config.policy,acknowledgeEmptySession:config.acknowledgeEmptySession===true,
  control:{enabled:sandbox,killSwitch:!sandbox,sandboxSubmit:sandbox,
   renewRateWindow:command==="sandbox-renew",recoverFinalizedFailure:command==="sandbox-recover-failed"}};
}
export async function runMakerCommand(command,config,signedPath,env=process.env,makeConnection=
 url=>new Connection(url,{commitment:"confirmed",disableRetryOnRateLimit:true})){
 const opts=parseMakerConfig(config,command,env);
 if(command==="inspect"){
  const state=await inspectLiveMakerSession(opts.sessionPath,opts.snapshotOptions);
  // Do not print signed wire or unsigned transaction messages during inspection.
  return {phase:state.phase,lastSlot:state.lastSlot,ordersReserved:state.ordersReserved,
   transactionsReserved:state.transactionsReserved,retries:state.retries,
   limits:state.limits,signature:state.intent?.signature,canSubmit:false,
   requiresManualRecovery:state.requiresManualRecovery};
 }
 const rpc=makeConnection(opts.url);
 if(command==="observe")return loadLiveMakerSnapshot(rpc,{...opts.snapshotOptions,
   maxLagSlots:opts.policy?.maxLagSlots??4});
 if(command==="plan"){
  // Explicitly advisory; policy must be supplied, never invent a baseline.
  const plan=await planLiveMakerQuotes(rpc,{snapshotOptions:opts.snapshotOptions,
   orders:opts.orders,policy:opts.policy});
  const {firstInstruction,...view}=plan;
  return {...view,canSubmit:false};
 }
 if(command==="sandbox-init"){
  // Validate pinned genesis and complete read-only market state first.
  await loadLiveMakerSnapshot(rpc,{...opts.snapshotOptions,maxLagSlots:opts.policy?.maxLagSlots??4});
  return initializeLiveMakerSession(opts);
 }
 if(command==="sandbox-reserve")return reserveLiveMakerIntent(rpc,opts);
 if(signedCommands.has(command)){
  const raw=(await boundedText(signedPath,2048)).trim();
  const wire=Buffer.from(raw,"base64");
  if(wire.length>1232||wire.toString("base64")!==raw)throw Error("Canonical signed wire base64 required");
  const transaction=VersionedTransaction.deserialize(wire);
  if(!Buffer.from(transaction.serialize()).equals(wire))throw Error("Noncanonical signed wire");
  return (command==="sandbox-prepare"?prepareLiveMakerIntent:submitSandboxMakerIntent)(
   rpc,{...opts,transaction});
 }
 if(command==="sandbox-finalize")return finalizeSandboxMakerIntent(rpc,opts);
 if(command==="sandbox-reconcile")return reconcileSandboxMakerIntent(rpc,opts);
 if(command==="sandbox-renew")return renewSandboxMakerRateWindow(rpc,opts);
 if(command==="sandbox-recover-failed")return reconcileFailedSandboxMakerIntent(rpc,opts);
 throw Error("Unsupported maker command");
}
async function main(){
 const [command,path,signedPath,...extra]=process.argv.slice(2);
 if(!commands.has(command)||!path||extra.length||Boolean(signedPath)!==signedCommands.has(command))
  throw Error("Usage: maker-session-cli <command> <config.json> [signed-wire.base64]");
 const config=JSON.parse(await boundedText(path,65536));
 console.log(json(await runMakerCommand(command,config,signedPath)));
}
if(process.argv[1]&&fileURLToPath(import.meta.url)===process.argv[1])
 main().catch(e=>{console.error(e.message);process.exitCode=1;});
