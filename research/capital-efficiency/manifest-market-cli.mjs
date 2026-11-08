import {readFileSync,writeFileSync} from "node:fs";import {readManifestMarket} from "./manifest-market.mjs";
const [configFile,resultFile]=process.argv.slice(2);
if(!configFile){console.error("Usage: node manifest-market-cli.mjs <market-config.json> [snapshot.json]");process.exit(2)}
try{const config=JSON.parse(readFileSync(configFile,"utf8"));const report=await readManifestMarket(config);const payload=JSON.stringify(report,null,2)+"\n";if(resultFile)writeFileSync(resultFile,payload);else process.stdout.write(payload);}
catch(e){console.error(e.message);process.exitCode=1}
