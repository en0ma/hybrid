// Read-only post-finalization maker audit. All account bytes are decoded
// from ONE getMultipleAccountsInfoAndContext RPC response, avoiding mixed
// slots between book sides and maker collateral. Never authorizes trading.
// Caller MUST independently derive canonical PDAs and trust the RPC source.
import {loadActivePageSnapshot} from "./rpc.mjs";
import {loadMakerBalanceSnapshot} from "./maker-balance.mjs";
import {readAtomicMakerJournal} from "./maker-atomic-journal.mjs";

function bytes(key,name){
  const value=typeof key?.toBytes==="function"?key.toBytes():key;
  if(!(value instanceof Uint8Array)||value.length!==32)
    throw new TypeError("Expected 32-byte "+name);
  return Buffer.from(value);
}
function distinctKeys(keys){
  const seen=new Set();
  for(const [i,key] of keys.entries()){
    const identity=bytes(key,"account key "+i).toString("hex");
    if(seen.has(identity))throw new Error("Duplicate post-finalization account key");
    seen.add(identity);
  }
}
function validSlot(n){return Number.isSafeInteger(n)&&n>0;}
/**
 * Read a fully coherent 6-account post-finalization snapshot at finalized
 * commitment, then run the SDK's existing market/owner/collateral decoders.
 * The finalized journal transition is NOT sufficient: the book and balance
 * must be independently refreshed and checked before a future quote plan.
 *
 * Returned result is observation-only, not an admission token. In particular,
 * these readers do NOT derive PDAs, prove page capacity, or enforce trading
 * budgets, and a future owner must revalidate state immediately before send.
 */
export async function auditFinalizedMakerState(connection,{
  journalPath,programId,market,maker,
  askPage,askOwnerPage,bidPage,bidOwnerPage,makerBalance,
  expectedGenesisHash,maxLagSlots=10,
}){
  if(!connection||typeof connection.getGenesisHash!=="function"||
      typeof connection.getMultipleAccountsInfoAndContext!=="function"||
      typeof connection.getSlot!=="function")
    throw new TypeError("RPC genesis, batched accounts and finalized slot required");
  if(!Number.isSafeInteger(maxLagSlots)||maxLagSlots<0)
    throw new RangeError("Invalid maximum slot lag");
  const journal=await readAtomicMakerJournal(journalPath);
  if(journal.phase!=="finalized")
    throw new Error("Maker journal not finalized; post-trade audit blocked");
  if(journal.genesisHash!==expectedGenesisHash)
    throw new Error("Maker journal genesis mismatch");
  if(await connection.getGenesisHash()!==expectedGenesisHash)
    throw new Error("RPC cluster genesis mismatch");
  const keys=[market,askPage,askOwnerPage,bidPage,bidOwnerPage,makerBalance];
  distinctKeys(keys);
  const response=await connection.getMultipleAccountsInfoAndContext(keys,{
    commitment:"finalized",minContextSlot:journal.lastConfirmedSlot,
  });
  const slot=response?.context?.slot;
  if(!validSlot(slot)||slot<journal.lastConfirmedSlot||
      !Array.isArray(response.value)||response.value.length!==6)
    throw new Error("Missing, invalid or stale post-finalization account batch");
  // Both book decoders and the collateral decoder use precisely this
  // immutable account response; none are allowed another RPC fetch.
  const [marketAccount,askAccount,askOwners,bidAccount,bidOwners,balanceAccount]=response.value;
  const mock={
    async getMultipleAccountsInfoAndContext(request,opts){
      if(opts.commitment!=="finalized"||opts.minContextSlot!==journal.lastConfirmedSlot)
        throw new Error("Post-finalization decoder used inconsistent options");
      const [requestedMarket,page,owner]=request;
      if(!bytes(requestedMarket,"market").equals(bytes(market,"market")))
        throw new Error("Unexpected market account request");
      if(bytes(page,"page").equals(bytes(askPage,"ask page"))&&
          bytes(owner,"owner").equals(bytes(askOwnerPage,"ask owner")))
        return {context:{slot},value:[marketAccount,askAccount,askOwners]};
      if(bytes(page,"page").equals(bytes(bidPage,"bid page"))&&
          bytes(owner,"owner").equals(bytes(bidOwnerPage,"bid owner")))
        return {context:{slot},value:[marketAccount,bidAccount,bidOwners]};
      throw new Error("Unexpected book accounts");
    },
    async getAccountInfoAndContext(address,opts){
      if(opts.commitment!=="finalized"||opts.minContextSlot!==journal.lastConfirmedSlot||
          !bytes(address,"balance").equals(bytes(makerBalance,"maker balance")))
        throw new Error("Unexpected maker collateral request");
      return {context:{slot},value:balanceAccount};
    },
  };
  const common={programId,market,minContextSlot:journal.lastConfirmedSlot,commitment:"finalized"};
  const ask=await loadActivePageSnapshot(mock,{
    ...common,page:askPage,ownerPage:askOwnerPage,side:"ask",
  });
  const bid=await loadActivePageSnapshot(mock,{
    ...common,page:bidPage,ownerPage:bidOwnerPage,side:"bid",
  });
  const balance=await loadMakerBalanceSnapshot(mock,{
    ...common,maker,makerBalance,
  });
  const chainSlot=await connection.getSlot("finalized");
  if(!validSlot(chainSlot)||chainSlot<slot||chainSlot-slot>maxLagSlots)
    throw new Error("Post-finalization maker snapshot is stale");
  return Object.freeze({
    mode:"post-finality-audit-only",
    slot,finalizedTransactionSlot:journal.lastConfirmedSlot,
    observedSlot:chainSlot,
    askOrderCount:ask.orders.length,bidOrderCount:bid.orders.length,
    freeBase:balance.freeBase,lockedBase:balance.lockedBase,
    freeQuote:balance.freeQuote,lockedQuote:balance.lockedQuote,
    requiresFreshSimulationBeforeSubmission:true,
    canSubmit:false,
  });
}
