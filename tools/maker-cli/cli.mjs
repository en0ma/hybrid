#!/usr/bin/env node
import {spawnSync} from "node:child_process";
import {fileURLToPath} from "node:url";
import {resolve} from "node:path";
const here=resolve(fileURLToPath(new URL(".",import.meta.url)));
const scripts=resolve(here,"../../devnet-demo/src");
const usage="Usage: node tools/maker-cli/cli.mjs manifest <MARKET> [MAKER] | readiness <MARKET> <ASK_OWNER> <BID_OWNER> <BASE_VAULT> <QUOTE_VAULT>";
const [cmd,...args]=process.argv.slice(2);
if(!["manifest","readiness"].includes(cmd)){console.error(usage);process.exit(2)}
if((cmd==="manifest"&&(args.length<1||args.length>2))||(cmd==="readiness"&&args.length!==5)){console.error(usage);process.exit(2)}
if(args.some(a=>!/^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(a))){console.error("Invalid base58 public key argument");process.exit(2)}
const script=cmd==="manifest"?"print-manifest.js":"check-readiness.js";
const result=spawnSync(process.execPath,[resolve(scripts,script),...args],{stdio:"inherit",env:process.env});
if(result.error){console.error(result.error.message);process.exit(1)}
process.exit(result.status??1);
