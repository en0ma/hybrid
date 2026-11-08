import {readFileSync} from "node:fs";
import {resolve} from "node:path";
import {fileURLToPath} from "node:url";
const requiredMetrics=["computeUnits","transactionCount","accountMetas","rentBytes","inventoryUtilizationBps","fillPriceAtoms","vaultConservationVerified","signingLatencyMs","settlementFailureRateBps"];
const sha=/^[a-f0-9]{40}$/i;
const isFiniteNumber=n=>typeof n==="number"&&Number.isFinite(n)&&n>=0;
export function validateMeasuredReport(data){
 const errors=[];const fail=s=>errors.push(s);
 if(data?.schemaVersion!==2)fail("schemaVersion must be 2");
 if(!sha.test(data?.hybridCommit??""))fail("pin a full Hybrid commit SHA");
 if(!sha.test(data?.manifestBaselineCommit??""))fail("pin a full Manifest commit SHA");
 const env=data?.environment??{};
 for(const key of ["solanaVersion","rustVersion","nodeVersion","hostCPU","operatingSystem","runnerId","rpcUrlOrLocalMode","measurementMethod"])
  if(typeof env[key]!=="string"||!env[key].trim())fail("missing environment "+key);
 if(!isFiniteNumber(env.hostRAMGiB)||env.hostRAMGiB===0)fail("hostRAMGiB must be positive");
 for(const mint of ["base","quote"])if(!Number.isInteger(env.mintDecimals?.[mint])||env.mintDecimals[mint]<0)fail("invalid "+mint+" mint decimals");
 const cases=(data?.cases??[]).map(x=>x.name);
 const results=Array.isArray(data?.results)?data.results:[];
 for(const name of ["one-maker-one-level","eight-maker-fanout","two-market-inventory","partial-fill-cancel-replace-retry","adverse-price-collateral-stress"]){
  if(!cases.includes(name))fail("missing case "+name);
  for(const implementation of ["hybrid","manifest"]){
   const matching=results.filter(x=>x.caseName===name&&x.implementation===implementation);
   if(matching.length!==1){fail(name+": expected one "+implementation+" measurement");continue;}
   const item=matching[0],expected=implementation==="hybrid"?data.hybridCommit:data.manifestBaselineCommit;
   if(item.implementationCommit!==expected)fail(name+": commit mismatch for "+implementation);
   if(item.environmentId!==env.runnerId)fail(name+": unmatched benchmark environment");
   if(item.accountingVerified!==true)fail(name+": custody/accounting audit must pass");
   if(!Array.isArray(item.receiptSignatures)||item.receiptSignatures.length<1||item.receiptSignatures.some(x=>typeof x!=="string"||x.length<40))fail(name+": missing executed receipt evidence");
   for(const metric of requiredMetrics){
    const v=item.metrics?.[metric];
    if(metric==="vaultConservationVerified"){if(v!==true)fail(name+": vault conservation not verified");}
    else if(!isFiniteNumber(v))fail(name+": missing nonnegative metric "+metric);
   }
   if((item.metrics?.inventoryUtilizationBps??0)>10000||(item.metrics?.settlementFailureRateBps??0)>10000)fail(name+": basis points out of range");
  }
 }
 return errors;
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
 const file=process.argv[2];if(!file){console.error("Usage: node validate-results.mjs <completed-experiment.json>");process.exit(2)}
 try{const errors=validateMeasuredReport(JSON.parse(readFileSync(file,"utf8")));if(errors.length){console.error(errors.join("\n"));process.exitCode=1}else console.log("Complete paired empirical evidence validated for five scenarios");}
 catch(error){console.error(error.message);process.exitCode=1}
}
