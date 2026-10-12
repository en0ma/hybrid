// Explicit cluster selection: never silently allow public clusters in local mode.
export const DEVNET_GENESIS="EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG";
export function clusterConfig(mode,url){
  const parsed=new URL(url);
  if(mode==="surfpool"){
    if(parsed.protocol!=="http:"||!["localhost","127.0.0.1","[::1]"].includes(parsed.hostname))
      throw Error("Surfpool RPC must be loopback HTTP; remote RPC cannot be trusted as local");
    if(parsed.username||parsed.password||parsed.search||parsed.hash)
      throw Error("Surfpool RPC URL must not contain credentials, query or fragment");
    return {cluster:"surfpool",url:parsed.href,explorer:null};
  }
  if(mode==="devnet"){
    if(parsed.protocol!=="https:"&&!["localhost","127.0.0.1"].includes(parsed.hostname))
      throw Error("Devnet RPC must use HTTPS unless explicitly loopback");
    return {cluster:"devnet",url:parsed.href,explorer:"devnet"};
  }
  throw Error("Select devnet or surfpool explicitly");
}
export async function assertCluster(connection,mode){
  const genesis=await connection.getGenesisHash();
  if(mode==="devnet"&&genesis!==DEVNET_GENESIS)throw Error("RPC is not devnet");
  // A Surfpool genesis is not an authentication mechanism. Only loopback URL is allowed.
  return genesis;
}
