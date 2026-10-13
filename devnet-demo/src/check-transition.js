import {readFileSync} from "node:fs";
import {compareMakerSnapshots,compareOrderLockSnapshots} from "./transitions.js";
const [beforePath,afterPath,action,asset,amount]=process.argv.slice(2);
if(!beforePath||!afterPath||!["deposit","withdraw","place","cancel"].includes(action)||!asset||(["deposit","withdraw"].includes(action)&&!amount)){
 console.error("Usage: npm run check:transition -- <before.json> <after.json> deposit|withdraw base|quote <amount-atoms> OR place|cancel ask|bid [expected-locked-atoms]");
 process.exit(2);
}
const before=JSON.parse(readFileSync(beforePath,"utf8"));
const after=JSON.parse(readFileSync(afterPath,"utf8"));
if(before.cluster!==after.cluster)throw Error("Snapshots have different clusters");
const report=["place","cancel"].includes(action)?
 compareOrderLockSnapshots(before,after,action,asset,amount):
 compareMakerSnapshots(before,after,action,asset,amount);
console.log(JSON.stringify({market:before.market,maker:before.maker,...report},null,2));
