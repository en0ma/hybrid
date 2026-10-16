import {readFileSync,writeFileSync} from "node:fs";
import {pathToFileURL} from "node:url";
import {resolve} from "node:path";
import {runLocalManifestFill} from "./manifest-local-fill.mjs";
import {requireLocalRpc} from "./manifest-local-send.mjs";
// Operator module exports buildFixture({config, connection, web3}).
// Private keys must only be loaded by that local module, never stored in config.
export async function runConfiguredManifestFill({config,fixtureLoader}){
 if(!config||typeof config!=="object"||typeof fixtureLoader!=="function")throw Error("configuration and operator fixture loader required");
 requireLocalRpc(config.rpcUrl);
 for(const name of ["programId","market","baseMint","quoteMint","taker","makerOwner"])
  if(typeof config[name]!=="string"||!config[name])throw Error("missing Manifest benchmark "+name);
 if(config.secretKey||config.privateKey||config.keypair)throw Error("do not include signing secrets in benchmark config");
 const {Connection,Transaction}=await import("@solana/web3.js");
 const connection=new Connection(config.rpcUrl,"confirmed");
 const fixture=await fixtureLoader({config,connection,Transaction});
 if(!fixture?.payer||!fixture?.instruction||typeof fixture.snapshotMarket!=="function"||!fixture.vaultAccounts)throw Error("operator fixture incomplete");
 // The operator's snapshot callback must use the exact shared connection.
 const snapshotMarket=()=>fixture.snapshotMarket({connection,config});
 const result=await runLocalManifestFill({...config,connection,Transaction,payer:fixture.payer,instruction:fixture.instruction,snapshotMarket,vaultAccounts:fixture.vaultAccounts});
 return {...result,operatorFixtureProvided:true};
}
if(process.argv[1]&&pathToFileURL(resolve(process.argv[1])).href===import.meta.url){
 const [configPath,fixturePath,outputPath]=process.argv.slice(2);
 if(!configPath||!fixturePath||!outputPath){console.error("Usage: node manifest-local-fill-cli.mjs <config.json> <local-operator-fixture.mjs> <evidence.json>");process.exitCode=2}
 else{
  try{
   const config=JSON.parse(readFileSync(configPath,"utf8"));
   const fixture=await import(pathToFileURL(resolve(fixturePath)).href);
   const result=await runConfiguredManifestFill({config,fixtureLoader:fixture.buildFixture});
   writeFileSync(outputPath,JSON.stringify(result,null,2)+"\n",{flag:"wx",mode:0o600});
   console.log("Wrote unverified Manifest local execution evidence:",outputPath);
  }catch(error){console.error(error.message);process.exitCode=1}
 }
}
