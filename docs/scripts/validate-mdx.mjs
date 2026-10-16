import {readFileSync} from "node:fs";
import {resolve, join} from "node:path";
import {compile} from "@mdx-js/mdx";
const root=resolve(import.meta.dirname,"..");
const config=JSON.parse(readFileSync(join(root,"docs.json"),"utf8"));
const pages=new Set();
function walk(node){
 if(Array.isArray(node)){node.forEach(walk);return;}
 if(typeof node==="string"){pages.add(node);return;}
 if(node&&typeof node==="object"){
  for(const key of ["pages","groups","tabs"])if(node[key])walk(node[key]);
 }
}
walk(config.navigation);
let failed=0;
for(const page of pages){
 const path=join(root,page+".mdx");
 try { await compile(readFileSync(path,"utf8"),{development:false}); }
 catch(err){failed++;console.error("MDX compilation failed:",page,"\\n",err);}
}
if(failed)process.exitCode=1;
else console.log("Compiled",pages.size,"Mintlify MDX pages successfully");
