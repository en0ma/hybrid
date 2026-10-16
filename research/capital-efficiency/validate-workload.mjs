import {readFileSync} from "node:fs";
import {fileURLToPath} from "node:url";
import {resolve} from "node:path";
const names=["one-maker-one-level","eight-maker-fanout","two-market-inventory","partial-fill-cancel-replace-retry","adverse-price-collateral-stress"];
export function validateWorkload(x){
const errors=[];if(x?.schemaVersion!==1||x?.kind!=="paired-hybrid-manifest-workload-contract")errors.push("bad workload schema");
if(x?.network!=="surfpool"&&x?.network!=="devnet")errors.push("unsupported network");
for(const key of ["makerBaseAtoms","makerQuoteAtoms","takerBaseAtoms","takerQuoteAtoms"]){const v=x?.funding?.[key];if(typeof v!=="string"||!/^[0-9]+$/.test(v)||BigInt(v)<=0n)errors.push("invalid funding "+key)}
const cases=Array.isArray(x?.operations)?x.operations:[];
for(const name of names){const matching=cases.filter(c=>c.name===name);if(matching.length!==1)errors.push("missing or duplicate "+name);else if(!matching[0].sequence?.includes("audit"))errors.push("missing audit "+name)}
for(const field of ["identicalMints","identicalFunding","identicalOrderNotional","identicalMakerDistribution","identicalFillQuantities","recordProtocolSpecificFees","benchmarkProgramVersionsByCommit"])if(x?.fairness?.[field]!==true)errors.push("fairness contract missing "+field);
return errors;
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){const file=process.argv[2]??new URL("./paired-workload.json",import.meta.url).pathname;try{const e=validateWorkload(JSON.parse(readFileSync(file,"utf8")));if(e.length){console.error(e.join("\n"));process.exitCode=1}else console.log("Five comparable P2 workloads and funding invariants validated")}catch(err){console.error(err.message);process.exitCode=1}}
