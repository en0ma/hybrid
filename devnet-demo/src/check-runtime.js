// Non-mutating local-runtime smoke gate. Does not provision or sign.
import {Connection,PublicKey} from "@solana/web3.js";
import {PROGRAM_ID} from "./abi.js";
import {clusterConfig,assertCluster} from "./cluster.js";

const mode=process.env.HYBRID_CLUSTER||"surfpool";
const cfg=clusterConfig(mode,process.env.SOLANA_RPC_URL||
  (mode==="surfpool"?"http://127.0.0.1:8899":"https://api.devnet.solana.com"));
const connection=new Connection(cfg.url,"confirmed");
const genesis=await assertCluster(connection,mode);
const program=new PublicKey(PROGRAM_ID);
const [slot,info]=await Promise.all([
  connection.getSlot("confirmed"),connection.getAccountInfo(program,"confirmed")
]);
const ready=!!info?.executable;
console.log(JSON.stringify({
 cluster:mode,rpc:cfg.url,genesis,slot,program:program.toBase58(),
 programExists:!!info,programExecutable:ready,
 note:ready?"Program deployment is present; initialized market and funded trades still require separate checks.":
 "Deploy the Hybrid SBF program at its compiled program ID before attempting lifecycle tests."
},null,2));
if(!ready)process.exitCode=1;
