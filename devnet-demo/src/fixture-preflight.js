// Read-only checklist before attempting a funded Surfpool lifecycle.
// No signing, key generation, token funding or deployment is performed.
import {spawnSync} from "node:child_process";
import {Connection,PublicKey} from "@solana/web3.js";
import {PROGRAM_ID} from "./abi.js";
export function validSidecarAddress(value){
 if(!value)return false;
 try{return new PublicKey(value).toBase58()===value&&value!==PublicKey.default.toBase58();}
 catch{return false;}
}
import {clusterConfig,assertCluster} from "./cluster.js";
export function fixturePreflightReport({tools,programExecutable,rpcReachable,fullBookConfigured}){
 const missing=[];
 for(const name of ["surfpool","solana","cargo"]){
  if(!tools[name])missing.push("Install "+name+" CLI");
 }
 if(!rpcReachable)missing.push("Start a reachable loopback Surfpool JSON-RPC");
 if(!programExecutable)missing.push("Deploy Hybrid SBF program at its fixed program ID");
 if(!fullBookConfigured)missing.push("Set HYBRID_BID_OWNER0 for full-book verification");
 return {readyForProvisioning:missing.length===0,missing,
  requires:"Provision SPL mints, funded wallets, market, custody, linked pages, owner sidecars and vaults before signed lifecycle",
  evidence:"No signed transactions or token conservation proven by this preflight"};
}
function installed(command){
 const check=spawnSync(command,["--version"],{stdio:"ignore",timeout:5000});
 return !check.error&&check.status===0;
}
if(process.argv[1]&&import.meta.url===new URL("file://"+process.argv[1]).href){
 const mode=process.env.HYBRID_CLUSTER||"surfpool";
 if(mode!=="surfpool")throw Error("Funded fixture preflight is Surfpool-only");
 const cfg=clusterConfig(mode,process.env.SOLANA_RPC_URL||"http://127.0.0.1:8899");
 const toolAvailability=Object.fromEntries(["surfpool","solana","cargo"].map(x=>[x,installed(x)]));
 let rpcReachable=false,programExecutable=false,rpcError=null,slot=null;
 try{
  const rpc=new Connection(cfg.url,"confirmed");
  await assertCluster(rpc,"surfpool");
  const [info,currentSlot]=await Promise.all([rpc.getAccountInfo(new PublicKey(PROGRAM_ID),"confirmed"),rpc.getSlot("confirmed")]);
  slot=currentSlot;rpcReachable=true;programExecutable=!!info?.executable;
 }catch(e){rpcError=e.message;}
 const report={cluster:"surfpool",rpc:cfg.url,slot,program:PROGRAM_ID,tools:toolAvailability,rpcError,
  ...fixturePreflightReport({tools:toolAvailability,rpcReachable,programExecutable,
   fullBookConfigured:validSidecarAddress(process.env.HYBRID_BID_OWNER0)})};
 console.log(JSON.stringify(report,null,2));
 if(!report.readyForProvisioning)process.exitCode=1;
}
