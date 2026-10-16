import {readFileSync} from "node:fs";import {fileURLToPath} from "node:url";import {resolve} from "node:path";
export function validateTemplate(t){
 const errors=[];
 const requiredCases=["one-maker-one-level","eight-maker-fanout","two-market-inventory","partial-fill-cancel-replace-retry","adverse-price-collateral-stress"];
 const cases=Array.isArray(t.cases)?t.cases:[];
 for(const name of requiredCases)if(!cases.some(c=>c.name===name))errors.push("missing case "+name);
 const requiredMetrics=["computeUnits","transactionCount","accountMetas","rentBytes","inventoryUtilizationBps","fillPriceAtoms","vaultConservationVerified","signingLatencyMs"];
 for(const name of requiredMetrics)if(!t.metrics?.includes(name))errors.push("missing metric "+name);
 if(!Object.hasOwn(t,"hybridCommit")||!Object.hasOwn(t,"manifestBaselineCommit"))errors.push("both Git revisions required");
 for(const name of ["solanaVersion","rustVersion","nodeVersion","hostCPU","hostRAMGiB","operatingSystem","runnerId","rpcUrlOrLocalMode","mintDecimals","measurementMethod"])if(!Object.hasOwn(t.environment??{},name))errors.push("missing environment "+name);
 for(const name of ["hybrid","manifest"])if(!t.resultRequirements?.implementations?.includes(name))errors.push("missing implementation "+name);
 if(!Array.isArray(t.results))errors.push("results must be an array");
 return errors;
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const t=JSON.parse(readFileSync(resolve(process.argv[2]??new URL("./experiment-template.json",import.meta.url).pathname),"utf8"));
 const errors=validateTemplate(t);
 if(errors.length){console.error(errors.join("\n"));process.exitCode=1}else console.log("P2 experiment template valid: 5 benchmark cases, 2 revisions, environment and signing latency");
}
