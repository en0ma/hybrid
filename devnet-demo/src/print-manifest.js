import {deriveMarketAccounts} from "./manifest.js";
const [market,maker]=process.argv.slice(2);
if(!market) {
  console.error("Usage: node src/print-manifest.js <MARKET_PUBKEY> [MAKER_PUBKEY]");
  process.exit(2);
}
console.log(JSON.stringify({...deriveMarketAccounts(market,maker),
  requiredExternalKeys:["ask page-0 owner sidecar","bid page-0 owner sidecar",
    "base and quote SPL mints","base and quote SPL token vaults","market authority/payer"],
  note:"Deterministic addresses are not proof that accounts exist or are initialized."},null,2));
