import {readFileSync,writeFileSync} from "node:fs";
import {simulateCapital} from "./model.mjs";
const args=process.argv.slice(2);if(args.length<1||args.length>2){console.error("Usage: node run.mjs <scenario.json> [report.json]");process.exit(2)}
try{
const input=JSON.parse(readFileSync(args[0],"utf8"));
const report={schemaVersion:1,kind:"illustrative-capital-model-not-chain-benchmark",scenario:input,output:simulateCapital(input)};
const text=JSON.stringify(report,null,2)+"\n";if(args[1])writeFileSync(args[1],text);else process.stdout.write(text);
}catch(e){console.error(e.message);process.exit(1)}
