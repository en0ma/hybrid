import {readFileSync} from "node:fs";
import {compareMakerSnapshots} from "./transitions.js";
const [beforePath,afterPath,action,asset,amount]=process.argv.slice(2);
if(!beforePath||!afterPath||!["deposit","withdraw"].includes(action)||!asset||!amount){
 console.error("Usage: npm run check:transition -- <before.json> <after.json> deposit|withdraw base|quote <amount-atoms>");
 process.exit(2);
}
const before=JSON.parse(readFileSync(beforePath,"utf8"));
const after=JSON.parse(readFileSync(afterPath,"utf8"));
if(before.cluster!==after.cluster)throw Error("Snapshots have different clusters");
const report=compareMakerSnapshots(before,after,action,asset,amount);
console.log(JSON.stringify({market:before.market,maker:before.maker,...report},null,2));
