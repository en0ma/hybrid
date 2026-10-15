import {PublicKey} from "@solana/web3.js";
import {PROGRAM_ID} from "./abi.js";

// Deterministic market account manifest. Does not touch the network or
// fabricate the legacy page-0 owner sidecars, which are market-bound keys.
export function deriveMarketAccounts(marketAddress,makerAddress) {
  const program=new PublicKey(PROGRAM_ID);
  const market=new PublicKey(marketAddress);
  const maker=makerAddress?new PublicKey(makerAddress):null;
  const enc=new TextEncoder();
  const derive=(...seeds)=>PublicKey.findProgramAddressSync(seeds,program)[0].toBase58();
  const index=(n)=>new Uint8Array([n,0,0,0]);
  return {
    programId:PROGRAM_ID,market:market.toBase58(),
    custody:derive(enc.encode("custody"),market.toBytes()),
    vaultAuthority:derive(enc.encode("vault-authority"),market.toBytes()),
    askPage0:derive(enc.encode("ask-page"),market.toBytes(),index(0)),
    bidPage0:derive(enc.encode("bid-page"),market.toBytes(),index(0)),
    askPage1:derive(enc.encode("ask-page"),market.toBytes(),index(1)),
    bidPage1:derive(enc.encode("bid-page"),market.toBytes(),index(1)),
    askOwnerPage1:derive(enc.encode("ask-owner-page"),market.toBytes(),index(1)),
    bidOwnerPage1:derive(enc.encode("bid-owner-page"),market.toBytes(),index(1)),
    ...(maker?{makerBalance:derive(enc.encode("maker-balance"),market.toBytes(),maker.toBytes())}:{})
  };
}
