// Build/deploy the pinned-ID Hybrid SBF program to a loopback Surfpool RPC.
// Requires the original fixed-ID program keypair; never silently generate a different ID.
import {spawnSync} from "node:child_process";
import {readFile} from "node:fs/promises";
import {resolve} from "node:path";
import {Connection,Keypair,PublicKey} from "@solana/web3.js";
import {PROGRAM_ID} from "./abi.js";
import {clusterConfig,assertCluster} from "./cluster.js";
export function checkedProgramKeypair(bytes){
 if(!Array.isArray(bytes)||bytes.length!==64||bytes.some(x=>!Number.isInteger(x)||x<0||x>255))
  throw Error("Program keypair must contain exactly 64 octets");
 const key=Keypair.fromSecretKey(Uint8Array.from(bytes));
 if(key.publicKey.toBase58()!==PROGRAM_ID)
  throw Error("Program keypair does not match pinned Hybrid program ID");
 return key;
}
function command(name,args,env){
 const result=spawnSync(name,args,{cwd:resolve(".."),env,stdio:"inherit",timeout:600000});
 if(result.error||result.status!==0)throw Error(name+" failed: "+(result.error?.message||result.status));
}
export function programChunks(binary,bytesPerChunk=256*1024){
 if(!Buffer.isBuffer(binary)||binary.length<4||!binary.subarray(0,4).equals(Buffer.from([0x7f,0x45,0x4c,0x46])))
  throw Error("Expected compiled ELF SBF program binary");
 if(!Number.isSafeInteger(bytesPerChunk)||bytesPerChunk<1||bytesPerChunk>1024*1024)
  throw Error("Invalid program chunk size");
 const chunks=[];
 for(let offset=0;offset<binary.length;offset+=bytesPerChunk)
  chunks.push({offset,data:binary.subarray(offset,offset+bytesPerChunk).toString("hex")});
 return chunks;
}
export async function uploadSurfpoolProgram(url,binary,call=fetch){
 const chunks=programChunks(binary);
 for(const {offset,data} of chunks){
  const response=await call(url,{method:"POST",headers:{"content-type":"application/json"},
   body:JSON.stringify({jsonrpc:"2.0",id:offset+1,method:"surfnet_writeProgram",
    params:[PROGRAM_ID,"0x"+data,offset]})});
  if(!response.ok)throw Error("surfnet_writeProgram HTTP "+response.status+" at "+offset);
  const result=await response.json();
  if(result.error)throw Error("surfnet_writeProgram failed at "+offset+": "+JSON.stringify(result.error));
 }
 return {chunks:chunks.length,bytes:binary.length};
}
async function main(){
 const cfg=clusterConfig("surfpool",process.env.SOLANA_RPC_URL||"http://127.0.0.1:8899");
 const rpc=new Connection(cfg.url,"confirmed");await assertCluster(rpc,"surfpool");
 const keypath=process.env.HYBRID_PROGRAM_KEYPAIR;
 const existing=await rpc.getAccountInfo(new PublicKey(PROGRAM_ID),"confirmed");
 if(existing?.executable){
  console.log(JSON.stringify({deployed:true,alreadyPresent:true,program:PROGRAM_ID,rpc:cfg.url}));
  return;
 }
 if(keypath)checkedProgramKeypair(JSON.parse(await readFile(resolve(keypath),"utf8")));
 const binary=resolve("..",process.env.HYBRID_SBF_PATH||"target/deploy/hybrid_program.so");
 if(process.env.HYBRID_SKIP_BUILD!=="1")
  command("cargo",["build-sbf","--manifest-path","program/Cargo.toml"],process.env);
 if(!keypath){
  const elf=await readFile(binary);
  const upload=await uploadSurfpoolProgram(cfg.url,elf);
  const deployed=await rpc.getAccountInfo(new PublicKey(PROGRAM_ID),"confirmed");
  if(!deployed?.executable)throw Error("Surfpool program write returned but fixed-ID program is not executable");
  console.log(JSON.stringify({deployed:true,method:"surfnet_writeProgram",program:PROGRAM_ID,rpc:cfg.url,...upload}));
  return;
 }
 const payer=process.env.HYBRID_DEPLOY_PAYER;
 if(!payer)throw Error("HYBRID_DEPLOY_PAYER is required for a funded local deployment");
 const args=["program","deploy","--url",cfg.url,"--keypair",resolve(payer),
  "--program-id",resolve(keypath),binary];
 command("solana",args,process.env);
 const deployed=await rpc.getAccountInfo(new PublicKey(PROGRAM_ID),"confirmed");
 if(!deployed?.executable)throw Error("Deployment CLI returned but pinned program is not executable on Surfpool");
 console.log(JSON.stringify({deployed:true,program:PROGRAM_ID,rpc:cfg.url,binary}));
}
if(process.argv[1]&&resolve(process.argv[1])===new URL(import.meta.url).pathname)
 main().catch(e=>{console.error(e.message);process.exitCode=1;});
