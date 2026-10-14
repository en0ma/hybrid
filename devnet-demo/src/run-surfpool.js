// One-command local fixture provisioning followed by signed six-stage execution.
// The Hybrid program must be deployed to the fixed ID before running.
import {spawnSync} from "node:child_process";
import {resolve,join} from "node:path";
import {mkdir,readFile,writeFile} from "node:fs/promises";
import {Connection,PublicKey} from "@solana/web3.js";
import {PROGRAM_ID} from "./abi.js";
import {clusterConfig,assertCluster} from "./cluster.js";
const dir=resolve(process.env.HYBRID_FIXTURE_DIR||"./.surfpool-fixture");
const url=clusterConfig("surfpool",process.env.SOLANA_RPC_URL||"http://127.0.0.1:8899").url;
const exec=(stage,env)=>{
 const result=spawnSync(process.execPath,[resolve("src",stage)],{
  cwd:process.cwd(),env:{...process.env,...env},stdio:"inherit",timeout:180000,
 });
 if(result.error||result.status!==0)throw Error(stage+" failed: "+(result.error?.message||"exit "+result.status));
};
async function main(){
 const rpc=new Connection(url,"confirmed");await assertCluster(rpc,"surfpool");
 await mkdir(dir,{recursive:true,mode:0o700});
 const env={HYBRID_CLUSTER:"surfpool",SOLANA_RPC_URL:url,HYBRID_FIXTURE_DIR:dir,
  HYBRID_FIXTURE_CONFIG:join(dir,"fixture.json"),
  HYBRID_MAKER_KEYPAIR:join(dir,"maker.json"),
  HYBRID_TAKER_KEYPAIR:join(dir,"taker.json"),
  HYBRID_EVIDENCE_FILE:join(dir,"lifecycle-evidence.json")};
 const report={program:PROGRAM_ID,rpc:url,deployed:false,provisioned:false,lifecycleCompleted:false,
  startedAt:new Date().toISOString()};
 try{
  if(!(await rpc.getAccountInfo(new PublicKey(PROGRAM_ID),"confirmed"))?.executable)
   exec("deploy-surfpool.js",{SOLANA_RPC_URL:url});
  if(!(await rpc.getAccountInfo(new PublicKey(PROGRAM_ID),"confirmed"))?.executable)
   throw Error("Hybrid SBF program deployment did not complete");
  report.deployed=true;
  exec("provision-surfpool.js",env);
  report.provisioned=true;
  exec("funded-lifecycle.js",env);
  const evidence=JSON.parse(await readFile(env.HYBRID_EVIDENCE_FILE,"utf8"));
  if(evidence.completed!==true||evidence.tradeSettlement?.verified!==true||
   evidence.tradeSettlement?.partialBookFillVerified!==true||evidence.stages?.length!==6||
   evidence.stages.some(s=>!s.signature)||
   new Set(evidence.stages.map(s=>s.signature)).size!==6||
   evidence.stages.map(s=>s.stage).join(",")!=="init,deposit,place,trade,cancel,withdraw")throw Error("Incomplete six-stage on-chain evidence");
  report.lifecycleCompleted=true;
  report.signatures=evidence.stages.map(s=>({stage:s.stage,signature:s.signature,slot:s.slot}));
 }catch(error){
  report.error=error.message;
  process.exitCode=1;
 }finally{
  report.finishedAt=new Date().toISOString();
  await writeFile(join(dir,"run-report.json"),JSON.stringify(report,null,2)+"\n",{mode:0o600});
 }
 console.log(JSON.stringify(report,null,2));
}
main().catch(e=>{console.error(e.message);process.exitCode=1;});
