// Executes operator-supplied, protocol-specific adapters, never a mock result.
// Adapters may submit only to an explicitly configured test network.
import {readFileSync,writeFileSync} from "node:fs";
import {resolve} from "node:path";
import {pathToFileURL} from "node:url";
import {validateWorkload} from "./validate-workload.mjs";
const auditSteps=new Set(["audit","confirm","refresh","reconcile-signature","simulate-timeout","adverse-price-update","attempt-withdraw"]);
const operations=new Set(["initialize","deposit","place","trade","audit","partial-fill","cancel","confirm","refresh","replace","simulate-timeout","reconcile-signature","adverse-price-update","attempt-withdraw"]);
export async function executeWorkload(contract,adapters,{scenarioName}={}){
 const errors=validateWorkload(contract);if(errors.length)throw Error(errors.join("; "));
 if(!Array.isArray(adapters)||adapters.length!==2||adapters[0].name!=="hybrid"||adapters[1].name!=="manifest")throw Error("require Hybrid and Manifest adapters");
 const selected=scenarioName===undefined?contract.operations:contract.operations.filter(x=>x.name===scenarioName);
 if(selected.length===0)throw Error("unknown benchmark scenario: "+scenarioName);
 const outputs=[];
 const seenSignatures=new Set();
 for(const adapter of adapters){
  if(typeof adapter.execute!=="function"||typeof adapter.identity!=="function")throw Error("adapter interface incomplete");
  const identity=await adapter.identity();
  if(!/^[a-f0-9]{40}$/i.test(identity.commit??"")||typeof identity.programId!=="string"||!identity.programId)throw Error("unpinned adapter identity: "+adapter.name);
  for(const scenario of selected){
   const actions=[];
   for(const step of scenario.sequence){
    if(!operations.has(step))throw Error("unknown scenario operation: "+step);
    const result=await adapter.execute({scenario,step,contract});
    if(auditSteps.has(step)){
     if(!result||result.status!=="audited"||result.verified!==true||typeof result.proof!=="string"||!result.proof.trim()||!Number.isSafeInteger(result.slot)||result.slot<=0)throw Error(adapter.name+" did not provide verified audit evidence for "+scenario.name+"/"+step);
     actions.push({step,kind:"audit",slot:result.slot,proof:result.proof});
    }else{
     if(!result||result.status!=="executed"||typeof result.signature!=="string"||!/^[1-9A-HJ-NP-Za-km-z]{64,88}$/.test(result.signature)||!Number.isSafeInteger(result.slot)||result.slot<=0)throw Error(adapter.name+" did not provide real executed evidence for "+scenario.name+"/"+step);
     if(seenSignatures.has(adapter.name+":"+result.signature))throw Error("reused transaction signature within protocol benchmark: "+adapter.name);
     seenSignatures.add(adapter.name+":"+result.signature);
     actions.push({step,kind:"transaction",signature:result.signature,slot:result.slot});
    }
   }
   outputs.push({implementation:adapter.name,commit:identity.commit,programId:identity.programId,scenario:scenario.name,actions});
  }
 }
 return {kind:"adapter-reported-workload-evidence-unverified",network:contract.network,outputs,warning:"Adapter-supplied signatures are not independently authenticated until the finalized RPC receipt collector and state auditor verify them."};
}
export async function loadAdapter(path){const mod=await import(pathToFileURL(resolve(path)).href);if(typeof mod.identity!=="function"||typeof mod.execute!=="function")throw Error("adapter must export identity() and execute()");return {name:mod.name,identity:mod.identity,execute:mod.execute};}
if(process.argv[1]&&pathToFileURL(resolve(process.argv[1])).href===import.meta.url){
 const [contractFile,hybridFile,manifestFile,outFile,scenarioName]=process.argv.slice(2);
 if(!contractFile||!hybridFile||!manifestFile){console.error("Usage: node run-workload.mjs <contract.json> <hybrid-adapter.mjs> <manifest-adapter.mjs> [report.json] [scenario-name]");process.exit(2)}
 try{const contract=JSON.parse(readFileSync(contractFile,"utf8"));const adapters=[await loadAdapter(hybridFile),await loadAdapter(manifestFile)];const output=await executeWorkload(contract,adapters,{scenarioName});const result=JSON.stringify(output,null,2)+"\n";if(outFile)writeFileSync(outFile,result);else process.stdout.write(result)}
 catch(error){console.error(error.message);process.exitCode=1}
}
