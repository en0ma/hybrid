import {readFileSync, existsSync} from "node:fs";
import {dirname, join, resolve} from "node:path";
const root=resolve(import.meta.dirname,"..");
const config=JSON.parse(readFileSync(join(root,"docs.json"),"utf8"));
const allowed=new Set();
const groups=[];
function visit(x){
 if(typeof x==="string"){allowed.add(x);return;}
 if(Array.isArray(x)){for(const v of x)visit(v);return;}
 if(x&&typeof x==="object"){
  if(x.group)groups.push(x.group);
  if(x.pages)visit(x.pages);
  if(x.groups)visit(x.groups);
  if(x.tabs)visit(x.tabs);
  if(x.anchors)visit(x.anchors);
 }
}
visit(config.navigation);
if(!allowed.size)throw Error("No pages declared in Mintlify navigation");
const missing=[...allowed].filter(p=>!existsSync(join(root,p+".mdx")));
if(missing.length)throw Error("Missing Mintlify pages: "+missing.join(", "));
const links=[];
for(const p of allowed){
 const text=readFileSync(join(root,p+".mdx"),"utf8");
 if(!text.startsWith("---\\n".replace("\\n","\n")))throw Error(p+" missing YAML frontmatter");
 if(!/^title: /m.test(text.split("---")[1]))throw Error(p+" missing page title");
 for(const m of text.matchAll(/(?:href=|\]\()\"?(\/[a-z][a-z0-9/-]*)/g))links.push({page:p,target:m[1]});
}
const broken=links.filter(({target})=>!target.startsWith("/assets/")&&!allowed.has(target.slice(1)));
const assetLinks=links.filter(({target})=>target.startsWith("/assets/"));
for(const {target} of assetLinks){
 if(!existsSync(join(root,target.slice(1)+".svg"))&& !existsSync(join(root,target.slice(1))))
  throw Error("Missing diagram asset: "+target);
}
if(broken.length)throw Error("Undeclared internal links: "+JSON.stringify(broken));
console.log("Mintlify documentation verified:",allowed.size,"pages;",links.length,"internal links;",groups.length,"navigation groups.");
