#!/usr/bin/env node
import {spawnSync} from "node:child_process";
import {fileURLToPath} from "node:url";
import {resolve} from "node:path";
import bs58 from "bs58";
const here=resolve(fileURLToPath(new URL(".",import.meta.url)));
const scripts=resolve(here,"../../devnet-demo/src");
const usage="Usage: node tools/maker-cli/cli.mjs manifest <MARKET> [MAKER] | readiness <MARKET> <ASK_OWNER> <BID_OWNER> <BASE_VAULT> <QUOTE_VAULT>";
const [cmd,...args]=process.argv.slice(2);
if(!["manifest","readiness"].includes(cmd)){console.error(usage);process.exit(2)}
if((cmd==="manifest"&&(args.length<1||args.length>2))||(cmd==="readiness"&&args.length!==5)){console.error(usage);process.exit(2)}
function validSolanaPublicKey(value) {
  if (typeof value !== "string") return false;
  try { return bs58.decode(value).length === 32; } catch { return false; }
}
if(args.some(a=>!validSolanaPublicKey(a))){console.error("Invalid Solana public key argument (expected 32 decoded bytes)");process.exit(2)}
const script=cmd==="manifest"?"print-manifest.js":"check-readiness.js";
const result=spawnSync(process.execPath,[resolve(scripts,script),...args],{stdio:"inherit",env:process.env});
if(result.error){console.error(result.error.message);process.exit(1)}
process.exit(result.status??1);
