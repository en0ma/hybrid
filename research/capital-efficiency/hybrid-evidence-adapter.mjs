// Hybrid adapter for previously executed, independently verified Surfpool lifecycle evidence.
// No fabricated signatures, signing keys, or implicit transactions.
// Scoped to the one-maker-one-level case; all other scenarios fail closed.
import {readFileSync} from "node:fs";
const load=path=>JSON.parse(readFileSync(path,"utf8"));
export const name="hybrid";
export function inspectEvidence(evidence,verified){
 if(evidence?.completed!==true||evidence?.finalAudit?.verified!==true||evidence?.tradeSettlement?.verified!==true)throw Error("Hybrid audited lifecycle incomplete");
 if(verified?.cluster!=="surfpool"||verified?.program!=="US517G5965aydkZ46HS38QLi7UQiSojurfbQfKCELFx"||verified.market!==evidence.market)throw Error("independent receipt verification mismatch");
 const stages=["init","deposit","place","trade","cancel","withdraw"];
 if(evidence.stages?.length!==6||verified.report?.length!==6)throw Error("six lifecycle stages required");
 for(let i=0;i<stages.length;i++){
  const stage=evidence.stages[i],receipt=verified.report[i];
  if(stage.stage!==stages[i]||stage.status!=="snapshotted"||receipt.signature!==stage.signature||receipt.slot!==stage.slot||!stage.signature||stage.slot<1)throw Error("Hybrid stage receipt mismatch "+stages[i]);
 }
 if(new Set(evidence.stages.map(x=>x.signature)).size!==6)throw Error("duplicate lifecycle signatures");
 return {stages,market:evidence.market};
}
export async function identity(){
 const commit=process.env.HYBRID_BENCHMARK_COMMIT;
 if(!/^[a-f0-9]{40}$/i.test(commit??""))throw Error("pin HYBRID_BENCHMARK_COMMIT");
 return {commit,programId:"US517G5965aydkZ46HS38QLi7UQiSojurfbQfKCELFx"};
}
export async function execute({scenario,step,contract}){
 if(contract.network!=="surfpool")throw Error("Hybrid evidence adapter supports only isolated Surfpool");
 if(scenario.name!=="one-maker-one-level")throw Error("unsupported Hybrid benchmark scenario: "+scenario.name);
 const evidencePath=process.env.HYBRID_LIFECYCLE_EVIDENCE;
 const verifiedPath=process.env.HYBRID_VERIFIED_RECEIPTS;
 if(!evidencePath||!verifiedPath)throw Error("require real lifecycle and independent receipt evidence files");
 const evidence=load(evidencePath),verified=load(verifiedPath);
 inspectEvidence(evidence,verified);
 const stageMap={initialize:"init",deposit:"deposit",place:"place",trade:"trade"};
 if(!stageMap[step])throw Error("unsupported benchmark step: "+step+"; audit is a state proof, not a transaction");
 const record=evidence.stages.find(x=>x.stage===stageMap[step]);
 return {status:"executed",signature:record.signature,slot:record.slot};
}
