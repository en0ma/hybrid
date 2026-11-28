// Durable single-intent session, explicitly enabled loopback sandbox only.
// No unattended signing, public-cluster sending, retry or automatic retirement.
// Paths and RPC adapter must belong to a trusted private test environment.
import {open,readFile,rename,lstat,unlink} from "node:fs/promises";
import {loadLiveMakerSnapshot} from "./maker-live-snapshot.mjs";
import {dirname} from "node:path";
import {createHash} from "node:crypto";
import {request as httpRequest} from "node:http";
import {keyBytes} from "./settlement.mjs";
import {planLiveMakerQuotes} from "./maker-live-plan.mjs";
import {withMakerJournalLock,assertFirstWireSignature} from "./maker-journal-guard.mjs";
import {initializeAtomicMakerJournal,readAtomicMakerJournal,advanceAtomicMakerJournal} from "./maker-atomic-journal.mjs";
import {simulateMakerTransaction,assertSimulatedBytes} from "./simulation-preflight.mjs";
import {observeSignatureStatus} from "./signature-status.mjs";

const sha=b=>createHash("sha256").update(b).digest("hex");
const id=k=>keyBytes(k).toString("hex");
const integer=n=>Number.isSafeInteger(n)&&n>=0;
const positive=n=>integer(n)&&n>0;
const LIMITS=["maxLagSlots","maxOrders","maxChanges","maxBaseExposure","maxQuoteNotional",
  "maxLossQuote","baselineEquityQuote","maxOrdersPerWindow","maxTransactionsPerWindow","maxRetries"];
const ATOMS=new Set(["maxBaseExposure","maxQuoteNotional","maxLossQuote","baselineEquityQuote"]);
const MAX64=(1n<<64n)-1n;
// A complete snapshot can contain up to 512 linked-book orders. Archives
// include both the before and after order sets plus the finalized journal.
const SESSION_BYTES=256*1024,ARCHIVE_BYTES=512*1024;
function recordBody(state,maxBytes=SESSION_BYTES){
  const body=JSON.stringify({state,digest:sha(JSON.stringify(state))})+"\n";
  if(Buffer.byteLength(body)>maxBytes)throw new Error("Oversized maker record");
  return body;
}
const ALPHABET="123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
function encode58(bytes){
  let n=0n;for(const b of bytes)n=n*256n+BigInt(b);
  let out="";while(n){out=ALPHABET[Number(n%58n)]+out;n/=58n;}
  for(const b of bytes){if(b!==0)break;out="1"+out;}
  return out;
}
function domain(options){
  const {programId,market,maker,askHeadOwner,bidHeadOwner,expectedGenesisHash}=options;
  if(typeof expectedGenesisHash!=="string"||
    !/^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(expectedGenesisHash))
    throw new Error("Pinned genesis required");
  const result={program:id(programId),market:id(market),maker:id(maker),
    askHeadOwner:id(askHeadOwner),bidHeadOwner:id(bidHeadOwner),genesisHash:expectedGenesisHash};
  if(new Set([result.program,result.market,result.maker,result.askHeadOwner,result.bidHeadOwner]).size!==5)
    throw new Error("Aliased maker session identity");
  return result;
}
function limits(policy){
  if(!policy||typeof policy!=="object")throw new Error("Explicit risk limits required");
  return Object.fromEntries(LIMITS.map(name=>{
    const v=policy[name];
    if(ATOMS.has(name)?typeof v!=="string"||!/^\d+$/.test(v)||BigInt(v)>MAX64:!integer(v))
      throw new Error("Invalid "+name);
    return [name,v];
  }));
}
function operator(control){
  if(control?.enabled!==true||control.killSwitch!==false)
    throw new Error("Maker trading disabled by operator");
}
async function exists(path){
  try{await lstat(path);return true;}catch(e){if(e.code==="ENOENT")return false;throw e;}
}
async function checkPending(path){
  if(await exists(path+".pending"))throw new Error("Interrupted session update; manual reconciliation required");
}
async function read(path){
  await checkPending(path);
  const raw=await readFile(path,"utf8");
  if(Buffer.byteLength(raw)>SESSION_BYTES)throw new Error("Oversized maker session");
  let parsed;try{parsed=JSON.parse(raw);}catch{throw new Error("Corrupt maker session");}
  const state=parsed?.state;
  if(!state||parsed.digest!==sha(JSON.stringify(state))||state.version!==1||
    !["idle","reserved","prepared","submitted","finalized"].includes(state.phase)||
    !integer(state.ordersReserved)||!integer(state.transactionsReserved)||
    !integer(state.retries)||!integer(state.lastSlot)||!state.domain||
    !(state.rateWindowSlots===null||positive(state.rateWindowSlots))||
    (state.phase==="idle"?state.intent!==null:!state.intent))
    throw new Error("Corrupt maker session integrity");
  limits(state.limits);
  if(state.phase!=="idle"&&(!positive(state.intent.snapshotSlot)||
    !positive(state.intent.lastValidBlockHeight)||
    typeof state.intent.messageBase64!=="string"||
    !["cancel","place"].includes(state.intent.phase)))throw new Error("Corrupt reserved intent");
  return state;
}
async function persist(path,state,initialize=false,maxBytes=SESSION_BYTES){
  const body=recordBody(state,maxBytes);
  const pending=path+".pending";
  const handle=await open(pending,"wx",0o600);
  try{
    await handle.writeFile(body);
    await handle.sync();
  }finally{await handle.close();}
  if(initialize){const final=await open(path,"wx",0o600);await final.close();}
  await rename(pending,path);
  const directory=await open(dirname(path),"r");
  try{await directory.sync();}finally{await directory.close();}
  return state;
}
function sameDomain(state,options){
  if(JSON.stringify(state.domain)!==JSON.stringify(domain(options)))
    throw new Error("Maker session domain mismatch");
}
function journal(path){return path+".transaction";}
async function checkNoJournal(path){
  if(await exists(journal(path))||await exists(journal(path)+".pending"))
    throw new Error("Existing transaction journal requires reconciliation");
}
/** Explicit fresh-session initialization. Never overwrite an existing record.
 * Operator must separately establish that no old journal or live intent exists.
 */
export async function initializeLiveMakerSession({sessionPath,snapshotOptions,policy,
  acknowledgeEmptySession=false}){
  if(acknowledgeEmptySession!==true)throw new Error("Explicit empty-session acknowledgement required");
  const rateWindowSlots=policy?.rateWindowSlots??null;
  if(rateWindowSlots!==null&&!positive(rateWindowSlots))
    throw new Error("Invalid rateWindowSlots");
  const state={version:1,phase:"idle",domain:domain(snapshotOptions),limits:limits(policy),
    rateWindowSlots,ordersReserved:0,transactionsReserved:0,retries:0,lastSlot:0,intent:null};
  return withMakerJournalLock(sessionPath+".lock",async()=>{
    await checkPending(sessionPath);await checkNoJournal(sessionPath);
    if(await exists(sessionPath))throw new Error("Existing maker session");
    await persist(sessionPath,state,true);
    return {phase:"idle",canSubmit:false};
  });
}
export async function inspectLiveMakerSession(sessionPath,snapshotOptions){
  const state=await read(sessionPath);
  if(snapshotOptions!==undefined)sameDomain(state,snapshotOptions);
  return {...state,canSubmit:false,requiresManualRecovery:state.phase!=="idle"};
}
/** Reserve a SINGLE canonical SDK action and its exact unsigned message.
 * No release path exists until independently verified post-finality recovery.
 */
export async function reserveLiveMakerIntent(connection,{sessionPath,snapshotOptions,orders,control}){
  operator(control);
  return withMakerJournalLock(sessionPath+".lock",async()=>{
    const state=await read(sessionPath);sameDomain(state,snapshotOptions);
    if(state.phase!=="idle")throw new Error("Pending maker intent; reconcile before new reservation");
    await checkNoJournal(sessionPath);
    const plan=await planLiveMakerQuotes(connection,{
      snapshotOptions:{...snapshotOptions,minContextSlot:Math.max(snapshotOptions.minContextSlot??0,state.lastSlot)},
      orders,policy:{...state.limits,enabled:true,killSwitch:false,pendingMutation:false,
        ordersInWindow:state.ordersReserved,transactionsInWindow:state.transactionsReserved,retries:state.retries},
    });
    if(plan.phase==="noop")return {phase:"noop",canSubmit:false};
    if(!plan.firstInstruction||typeof snapshotOptions.adapter?.compileUnsignedV0!=="function"||
      typeof connection.getLatestBlockhashAndContext!=="function")
      throw new Error("Canonical compiler and context blockhash required");
    const block=await connection.getLatestBlockhashAndContext({
      commitment:"confirmed",minContextSlot:plan.observedSlot,
    });
    const slot=block?.context?.slot,blockhash=block?.value?.blockhash;
    const lastValidBlockHeight=block?.value?.lastValidBlockHeight;
    if(!positive(slot)||slot<plan.observedSlot||slot-plan.snapshotSlot>state.limits.maxLagSlots||
      !positive(lastValidBlockHeight)||typeof blockhash!=="string"||
      !/^[1-9A-HJ-NP-Za-km-z]{32,44}$/.test(blockhash))
      throw new Error("Stale or invalid blockhash context");
    operator(control);
    const tx=snapshotOptions.adapter.compileUnsignedV0({payer:snapshotOptions.maker,
      recentBlockhash:blockhash,instruction:plan.firstInstruction});
    const bytes=Buffer.from(tx.serialize());
    if(bytes.length<=65||bytes.length>1232||bytes[0]!==1||bytes.subarray(1,65).some(Boolean))
      throw new Error("Expected one unsigned maker signature");
    const intent={phase:plan.phase,snapshotSlot:plan.snapshotSlot,blockhash,lastValidBlockHeight,
      targets:orders.map(o=>({side:o.side,priceAtoms:o.priceAtoms,sizeAtoms:o.sizeAtoms})),
      messageBase64:bytes.subarray(65).toString("base64"),unsignedSha256:sha(bytes),
      action:JSON.parse(JSON.stringify(plan.actions[0],(_,v)=>typeof v==="bigint"?String(v):v))};
    const next={...state,phase:"reserved",lastSlot:slot,ordersReserved:state.ordersReserved+1,
      transactionsReserved:state.transactionsReserved+1,intent};
    await persist(sessionPath,next);
    return {phase:"reserved",canSubmit:false,unsignedWireBase64:bytes.toString("base64"),
      snapshotSlot:plan.snapshotSlot,action:intent.action,lastValidBlockHeight};
  });
}
/** Verify an externally signed exact reservation, simulate, persist v2
 * identity and exact wire hash. This is NOT permission to submit.
 */
export async function prepareLiveMakerIntent(connection,{sessionPath,snapshotOptions,transaction,control}){
  operator(control);
  return withMakerJournalLock(sessionPath+".lock",async()=>{
    const state=await read(sessionPath);sameDomain(state,snapshotOptions);
    if(state.phase!=="reserved")throw new Error("No unsigned reserved maker intent");
    await checkNoJournal(sessionPath);
    const intent=state.intent,wire=Buffer.from(transaction.serialize());
    if(wire.length<=65||wire.length>1232||wire[0]!==1||
      !wire.subarray(65).equals(Buffer.from(intent.messageBase64,"base64")))
      throw new Error("Signed message differs from durable reservation");
    const unsigned=Buffer.from(wire);unsigned.fill(0,1,65);
    if(sha(unsigned)!==intent.unsignedSha256)throw new Error("Reserved transaction fingerprint mismatch");
    const signature=encode58(wire.subarray(1,65));
    assertFirstWireSignature(transaction,signature);
    if(await exists(archivePath(sessionPath,signature))||
      await exists(archivePath(sessionPath,signature)+".pending"))
      throw new Error("Previously reconciled transaction signature cannot be reused");
    if(await connection.getGenesisHash()!==state.domain.genesisHash)throw new Error("RPC genesis mismatch");
    const currentSlot=await connection.getSlot("confirmed");
    const height=await connection.getBlockHeight("confirmed");
    if(!positive(currentSlot)||currentSlot<state.lastSlot||!integer(height)||
      height>intent.lastValidBlockHeight)throw new Error("Stale or expired reserved transaction");
    const evidence=await simulateMakerTransaction(connection,{transaction,
      minContextSlot:intent.snapshotSlot,currentSlot,maxLagSlots:state.limits.maxLagSlots});
    if(evidence.simulationSlot-intent.snapshotSlot>state.limits.maxLagSlots)
      throw new Error("Simulation exceeds reserved freshness window");
    assertSimulatedBytes(transaction,evidence);
    if(sha(wire)!==evidence.transactionSha256)throw new Error("Signed wire changed before preparation");
    operator(control);
    await initializeAtomicMakerJournal(journal(sessionPath),{transaction,simulationEvidence:evidence,
      signature,genesisHash:state.domain.genesisHash,lastValidBlockHeight:intent.lastValidBlockHeight,
      snapshotSlot:intent.snapshotSlot});
    await persist(sessionPath,{...state,phase:"prepared",lastSlot:evidence.simulationSlot,
      intent:{...intent,signature,transactionSha256:evidence.transactionSha256,
        signedWireBase64:wire.toString("base64"),simulationEvidence:evidence}});
    return {phase:"prepared",signature,canSubmit:false,requiresDurableSubmissionIntegration:true};
  });
}
function sandbox(connection,control){
  operator(control);
  if(control.sandboxSubmit!==true)throw new Error("Explicit funded sandbox submission opt-in required");
  let url;try{url=new URL(connection.rpcEndpoint);}catch{throw new Error("Loopback RPC required");}
  if(url.protocol!=="http:"||!["127.0.0.1","[::1]"].includes(url.hostname)||
    !url.port||url.username||url.password||url.pathname!=="/"||url.search||url.hash)
    throw new Error("Loopback RPC required; public cluster submission disabled");
  return url;
}
// Bypass web3.js retry-on-429 transport. No redirects, agent, middleware
// or retry loop; a single native HTTP request is the entire send attempt.
function sendSandboxWireOnce(url,wire,minContextSlot){
  const body=JSON.stringify({jsonrpc:"2.0",id:1,method:"sendTransaction",params:[
    wire.toString("base64"),{encoding:"base64",skipPreflight:false,
      preflightCommitment:"confirmed",maxRetries:0,minContextSlot},
  ]});
  return new Promise((resolve,reject)=>{
    const req=httpRequest(url,{method:"POST",agent:false,headers:{
      "Content-Type":"application/json","Content-Length":Buffer.byteLength(body),
      "Connection":"close",
    }},res=>{
      if(res.statusCode!==200){res.destroy();reject(new Error("Uncertain HTTP submission response"));return;}
      const chunks=[];let size=0;
      res.on("data",chunk=>{
        size+=chunk.length;
        if(size>8192){res.destroy(new Error("Oversized RPC response"));return;}
        chunks.push(chunk);
      });
      res.on("error",reject);
      res.on("aborted",()=>reject(new Error("Interrupted RPC response")));
      res.on("end",()=>{
        try{
          const value=JSON.parse(Buffer.concat(chunks).toString("utf8"));
          if(value?.jsonrpc!=="2.0"||value.id!==1||value.error!==undefined||
            typeof value.result!=="string")throw new Error("Uncertain RPC submission result");
          resolve(value.result);
        }catch(e){reject(e);}
      });
    });
    req.on("error",reject);
    const timer=setTimeout(()=>req.destroy(new Error("Sandbox submission timeout")),5000);
    req.on("close",()=>clearTimeout(timer));
    req.end(body);
  });
}
async function verifiedJournal(path,state){
  if(await exists(journal(path)+".pending"))throw new Error("Interrupted transaction journal update");
  const record=await readAtomicMakerJournal(journal(path));
  const intent=state.intent,wire=Buffer.from(intent.signedWireBase64,"base64");
  if(record.phase!==state.phase||record.signature!==intent.signature||
    record.genesisHash!==state.domain.genesisHash||record.snapshotSlot!==intent.snapshotSlot||
    record.lastValidBlockHeight!==intent.lastValidBlockHeight||
    record.transactionSha256!==intent.transactionSha256||sha(wire)!==record.transactionSha256||
    !wire.subarray(65).equals(Buffer.from(intent.messageBase64,"base64")))
    throw new Error("Session and transaction journal identity mismatch");
  assertFirstWireSignature({serialize:()=>wire},record.signature);
  assertSimulatedBytes({serialize:()=>wire},intent.simulationEvidence);
  return {record,wire};
}
/** Broadcast at most once from a durable prepared intent. The session and
 * journal enter submitted BEFORE the network call. RPC uncertainty never
 * changes that state or permits a replacement. Only isolated funded local
 * test wallets are allowed; this is not a general-purpose send service.
 */
export async function submitSandboxMakerIntent(connection,{sessionPath,snapshotOptions,transaction,control}){
  sandbox(connection,control);
  return withMakerJournalLock(sessionPath+".lock",async()=>{
    const state=await read(sessionPath);sameDomain(state,snapshotOptions);
    if(state.phase!=="prepared")throw new Error("No prepared intent; duplicate submission blocked");
    const {record,wire}=await verifiedJournal(sessionPath,state);
    const intent=state.intent;
    if(!wire.equals(Buffer.from(transaction.serialize())))throw new Error("Signed wire differs from journal");
    if(snapshotOptions.commitment&&snapshotOptions.commitment!=="confirmed")
      throw new Error("Sandbox submission requires confirmed snapshot");
    // Re-authenticate and replan from chain, using already-reserved counters.
    const plan=await planLiveMakerQuotes(connection,{
      snapshotOptions:{...snapshotOptions,commitment:"confirmed",
        minContextSlot:Math.max(snapshotOptions.minContextSlot??0,state.lastSlot)},
      orders:intent.targets,policy:{...state.limits,enabled:true,killSwitch:false,pendingMutation:false,
        ordersInWindow:state.ordersReserved-1,transactionsInWindow:state.transactionsReserved-1,
        retries:state.retries},
    });
    if(plan.phase!==intent.phase||!plan.firstInstruction)
      throw new Error("Reserved action no longer matches fresh plan");
    const candidate=snapshotOptions.adapter.compileUnsignedV0({payer:snapshotOptions.maker,
      recentBlockhash:intent.blockhash,instruction:plan.firstInstruction});
    if(sha(Buffer.from(candidate.serialize()))!==intent.unsignedSha256)
      throw new Error("Reserved instruction no longer matches fresh plan");
    const currentSlot=await connection.getSlot("confirmed");
    const height=await connection.getBlockHeight("confirmed");
    if(!positive(currentSlot)||currentSlot<plan.observedSlot||!integer(height)||
      height>intent.lastValidBlockHeight||currentSlot-intent.snapshotSlot>state.limits.maxLagSlots)
      throw new Error("Stale or expired prepared transaction");
    const evidence=await simulateMakerTransaction(connection,{transaction,
      minContextSlot:plan.snapshotSlot,currentSlot,maxLagSlots:state.limits.maxLagSlots});
    if(evidence.simulationSlot-intent.snapshotSlot>state.limits.maxLagSlots||
      !wire.equals(Buffer.from(transaction.serialize())))
      throw new Error("Signed wire changed or simulation outside reserved freshness");
    // Preserve the original preparation evidence for the v2 journal's
    // original snapshot binding. The second simulation is separately stored.
    const submitted={...state,phase:"submitted",lastSlot:evidence.simulationSlot,
      intent:{...intent,submissionSimulation:evidence,before:plan.before}};
    // Bound every prospective persisted record BEFORE mutating the journal
    // or sending. Use the largest possible slot encoding for later finality.
    const maxSlot=Number.MAX_SAFE_INTEGER;
    const projected={...submitted,phase:"finalized",lastSlot:maxSlot,
      intent:{...submitted.intent,finalizedSlot:maxSlot}};
    recordBody(projected);
    recordBody({session:projected,journal:{...record,phase:"finalized",lastConfirmedSlot:maxSlot},
      audit:{slot:maxSlot,actual:expectedAfter(projected.intent),
        custody:plan.before.custody,vaults:plan.before.vaults}},ARCHIVE_BYTES);
    sandbox(connection,control);
    await advanceAtomicMakerJournal(journal(sessionPath),{
      from:"prepared",to:"submitted",signature:record.signature,genesisHash:record.genesisHash,
      transaction:{serialize:()=>wire},simulationEvidence:intent.simulationEvidence,
    });
    await persist(sessionPath,submitted);
    // A kill switch changed during disk persistence must prevent broadcast.
    // The conservative submitted state still requires reconciliation.
    sandbox(connection,control);
    const sendSlot=await connection.getSlot("confirmed");
    const sendHeight=await connection.getBlockHeight("confirmed");
    if(!positive(sendSlot)||sendSlot<evidence.simulationSlot||
      sendSlot-intent.snapshotSlot>state.limits.maxLagSlots||
      !integer(sendHeight)||sendHeight>intent.lastValidBlockHeight||
      await connection.getGenesisHash()!==record.genesisHash)
      throw new Error("Prepared wire no longer fresh at broadcast");
    const endpoint=sandbox(connection,control);
    try{
      const returned=await sendSandboxWireOnce(endpoint,wire,evidence.simulationSlot);
      return {state:returned===record.signature?"submitted":"unknown",
        signature:record.signature,canRetry:false,requiresIndependentReconciliation:true};
    }catch{
      return {state:"unknown",signature:record.signature,canRetry:false,
        requiresIndependentReconciliation:true};
    }
  });
}
/** Observation only: no expiry, release or retry is inferred from RPC. */
export async function observeLiveMakerIntent(connection,{sessionPath,snapshotOptions}){
  const state=await read(sessionPath);sameDomain(state,snapshotOptions);
  if(!["prepared","submitted","finalized"].includes(state.phase))
    throw new Error("No prepared maker intent");
  const {record}=await verifiedJournal(sessionPath,state);
  const observation=await observeSignatureStatus(connection,{signature:record.signature,
    expectedGenesisHash:record.genesisHash,lastValidBlockHeight:record.lastValidBlockHeight});
  return {state:observation.state,phase:state.phase,signature:record.signature,
    slot:observation.slot,contextSlot:observation.contextSlot,
    currentBlockHeight:observation.currentBlockHeight,expiredBlockhash:observation.expiredBlockhash,
    canSubmit:false,canRetry:false,canRelease:false,requiresIndependentReconciliation:true};
}
/** Match the finalized receipt to the exact reserved message and signature.
 * Keep the session blocked even on success until independent post-state
 * reconciliation proves the expected collateral and order changes.
 */
export async function finalizeSandboxMakerIntent(connection,{sessionPath,snapshotOptions}){
  return withMakerJournalLock(sessionPath+".lock",async()=>{
    const state=await read(sessionPath);sameDomain(state,snapshotOptions);
    if(state.phase!=="submitted")throw new Error("No submitted maker intent");
    const {record,wire}=await verifiedJournal(sessionPath,state);
    const observation=await observeSignatureStatus(connection,{signature:record.signature,
      expectedGenesisHash:record.genesisHash,lastValidBlockHeight:record.lastValidBlockHeight});
    if(observation.state!=="finalized-success")
      return {state:observation.state,canRetry:false,canRelease:false};
    const receipt=await connection.getTransaction(record.signature,{
      commitment:"finalized",maxSupportedTransactionVersion:0,
    });
    if(!receipt||receipt.version!==0||receipt.slot!==observation.slot||
      receipt.slot<state.lastSlot||receipt.meta?.err!==null||
      !Array.isArray(receipt.transaction?.signatures)||receipt.transaction.signatures.length!==1||
      receipt.transaction.signatures[0]!==record.signature||
      typeof receipt.transaction.message?.serialize!=="function"||
      !Buffer.from(receipt.transaction.message.serialize()).equals(wire.subarray(65)))
      throw new Error("Finalized receipt does not match journaled signed message");
    await advanceAtomicMakerJournal(journal(sessionPath),{
      from:"submitted",to:"finalized",signature:record.signature,genesisHash:record.genesisHash,
      transaction:{serialize:()=>wire},simulationEvidence:state.intent.simulationEvidence,
      finalizedSlot:receipt.slot,
    });
    await persist(sessionPath,{...state,phase:"finalized",lastSlot:receipt.slot,
      intent:{...state.intent,finalizedSlot:receipt.slot}});
    return {state:"finalized-success",signature:record.signature,finalizedSlot:receipt.slot,
      canRetry:false,canRelease:false,requiresPostStateReconciliation:true};
  });
}

function archivePath(path,signature){
  if(typeof signature!=="string"||!/[1-9A-HJ-NP-Za-km-z]/.test(signature)||
    !/^[1-9A-HJ-NP-Za-km-z]{80,90}$/.test(signature))throw new Error("Invalid archive signature");
  return path+".settled."+signature;
}
const inventoryFields=["freeBase","lockedBase","freeQuote","lockedQuote"];
const orderView=o=>({side:o.side,sequence:String(o.sequence),priceX64:String(o.priceX64),
  sqrtPriceX64:String(o.sqrtPriceX64),baseQty:String(o.baseQty),pageIndex:o.pageIndex});
const sortedOrders=list=>list.map(orderView).sort((a,b)=>
  BigInt(a.sequence)<BigInt(b.sequence)?-1:BigInt(a.sequence)>BigInt(b.sequence)?1:0);
const equal=(a,b)=>JSON.stringify(a)===JSON.stringify(b);
function expectedAfter(intent){
  const before=intent.before,action=intent.action;
  if(!before||!Array.isArray(before.ownedOrders)||!["ask","bid"].includes(action?.side))
    throw new Error("Missing pre-submit inventory proof");
  const inventory=Object.fromEntries(inventoryFields.map(k=>[k,BigInt(before.inventory[k])]));
  let orders=before.ownedOrders.map(orderView),nextSequence=BigInt(before.nextSequence),order;
  const cancel=intent.phase==="cancel";
  if(cancel){
    order=orders.find(o=>o.side===action.side&&o.sequence===action.sequence&&o.pageIndex===0);
    if(!order)throw new Error("Missing canceled order in pre-submit proof");
    orders=orders.filter(o=>o.sequence!==order.sequence);
  }else{
    order=orderView({...action,sequence:nextSequence,pageIndex:0});
    orders.push(order);nextSequence++;
  }
  const q=1n<<64n;
  const amount=order.side==="ask"?BigInt(order.baseQty):
    (BigInt(order.baseQty)*BigInt(order.priceX64)+q-1n)/q;
  const [free,locked]=order.side==="ask"?["freeBase","lockedBase"]:["freeQuote","lockedQuote"];
  inventory[free]+=cancel?amount:-amount;
  inventory[locked]+=cancel?-amount:amount;
  if(Object.values(inventory).some(v=>v<0n||v>MAX64)||nextSequence>MAX64)
    throw new Error("Expected collateral change out of range");
  return {inventory:Object.fromEntries(inventoryFields.map(k=>[k,String(inventory[k])])),
    orders:sortedOrders(orders),nextSequence:String(nextSequence)};
}
/** Strict single-action, no-intervening-change reconciliation.
 * A concurrent fill, deposit, withdrawal, sequence advance or unexplained
 * vault change is NOT inferred to be harmless. Leave the session blocked.
 * The complete receipt-bound intent and audit are archived before retirement.
 */
export async function reconcileSandboxMakerIntent(connection,{sessionPath,snapshotOptions}){
  return withMakerJournalLock(sessionPath+".lock",async()=>{
    const state=await read(sessionPath);sameDomain(state,snapshotOptions);
    if(state.phase!=="finalized")throw new Error("No finalized intent to reconcile");
    const {record}=await verifiedJournal(sessionPath,state);
    const intent=state.intent;
    if(record.lastConfirmedSlot!==intent.finalizedSlot||record.lastConfirmedSlot!==state.lastSlot||
      !positive(intent.before?.slot)||intent.before.slot>record.lastConfirmedSlot)
      throw new Error("Finalized journal and pre-submit context mismatch");
    const snapshot=await loadLiveMakerSnapshot(connection,{
      ...snapshotOptions,commitment:"finalized",maxLagSlots:state.limits.maxLagSlots,
      minContextSlot:Math.max(snapshotOptions.minContextSlot??0,state.lastSlot),
    });
    const expected=expectedAfter(intent);
    const actual={inventory:Object.fromEntries(inventoryFields.map(k=>[k,String(snapshot.inventory[k])])),
      orders:sortedOrders(snapshot.ownedOrders),nextSequence:String(snapshot.header.nextSequence)};
    const custody={base:String(snapshot.custodyState.totalBase),quote:String(snapshot.custodyState.totalQuote)};
    const vaults={base:String(snapshot.vaultAmounts.base),quote:String(snapshot.vaultAmounts.quote)};
    if(snapshot.pendingLazySettlement||!equal(actual,expected)||
      !equal(custody,intent.before.custody)||!equal(vaults,intent.before.vaults))
      throw new Error("Unexplained post-finality maker, custody or vault state; manual reconciliation required");
    const audit={slot:snapshot.slot,actual,custody,vaults};
    // Exclusive archive creation retains both complete records. If anything
    // fails after archival, the lock and/or non-idle state prevent reuse.
    await retireWithArchive(sessionPath,state,record,audit);
    return {state:"reconciled",signature:record.signature,slot:snapshot.slot,
      canSubmit:false,canRetry:false,requiresFreshPlan:true};
  });
}

/** Explicit idle-only renewal, never an automatic retry or reset of loss
 * baseline/limits. Require a FULL configured quiet interval after the last
 * reconciled slot, not a wall-clock boundary that permits adjacent bursts.
 * A missing window configuration keeps lifetime reservation caps.
 */
export async function renewSandboxMakerRateWindow(connection,{sessionPath,snapshotOptions,control}){
  sandbox(connection,control);
  if(control.renewRateWindow!==true)throw new Error("Explicit rate-window renewal required");
  return withMakerJournalLock(sessionPath+".lock",async()=>{
    const state=await read(sessionPath);sameDomain(state,snapshotOptions);
    if(state.phase!=="idle")return {state:"pending-intent",canSubmit:false};
    await checkNoJournal(sessionPath);
    if(state.rateWindowSlots===null)throw new Error("Rate-window renewal not configured");
    if(state.ordersReserved===0&&state.transactionsReserved===0)
      return {state:"unused-window",canSubmit:false};
    if(!positive(state.lastSlot)||typeof connection.getSlot!=="function"||
      await connection.getGenesisHash()!==state.domain.genesisHash)
      throw new Error("Invalid rate-window context");
    const slot=await connection.getSlot("finalized");
    if(!positive(slot)||slot<state.lastSlot)throw new Error("Stale finalized renewal slot");
    const elapsed=slot-state.lastSlot;
    if(elapsed<state.rateWindowSlots)
      return {state:"cooldown",remainingSlots:state.rateWindowSlots-elapsed,canSubmit:false};
    sandbox(connection,control);
    if(control.renewRateWindow!==true)throw new Error("Rate-window renewal no longer approved");
    const path=sessionPath+".rate-window."+state.lastSlot;
    // Durable audit before reset; a partial transition remains locked.
    await persist(path,{session:state,renewalSlot:slot},true,ARCHIVE_BYTES);
    await persist(sessionPath,{...state,ordersReserved:0,transactionsReserved:0,lastSlot:slot});
    return {state:"renewed",slot,canSubmit:false,requiresFreshPlan:true};
  });
}

async function retireWithArchive(sessionPath,state,record,audit){
  const path=archivePath(sessionPath,record.signature);
  if(await exists(path))throw new Error("Existing reconciliation archive; manual recovery required");
  await persist(path,{session:state,journal:record,audit},true,ARCHIVE_BYTES);
  await unlink(journal(sessionPath));
  const directory=await open(dirname(sessionPath),"r");
  try{await directory.sync();}finally{await directory.close();}
  await persist(sessionPath,{...state,phase:"idle",lastSlot:audit.slot,intent:null});
}
/** Explicit recovery of a conclusively failed transaction, not a retry.
 * Only the exact signed message with a finalized error and independently
 * authenticated unchanged market collateral/order state can be retired.
 * Native transaction fees may be charged despite instruction rollback.
 * Unknown, expired-without-receipt or concurrent changes remain blocked.
 */
export async function reconcileFailedSandboxMakerIntent(connection,{sessionPath,snapshotOptions,control}){
  sandbox(connection,control);
  if(control.recoverFinalizedFailure!==true)throw new Error("Explicit finalized-failure recovery approval required");
  return withMakerJournalLock(sessionPath+".lock",async()=>{
    const state=await read(sessionPath);sameDomain(state,snapshotOptions);
    if(state.phase!=="submitted")throw new Error("No submitted maker intent to recover");
    const {record,wire}=await verifiedJournal(sessionPath,state);
    const observation=await observeSignatureStatus(connection,{signature:record.signature,
      expectedGenesisHash:record.genesisHash,lastValidBlockHeight:record.lastValidBlockHeight});
    if(observation.state!=="finalized-failure")
      return {state:observation.state,canRetry:false,canRelease:false};
    const receipt=await connection.getTransaction(record.signature,{
      commitment:"finalized",maxSupportedTransactionVersion:0,
    });
    const error=receipt?.meta?.err;
    if(!receipt||receipt.version!==0||receipt.slot!==observation.slot||
      receipt.slot<state.lastSlot||error===null||error===undefined||
      !(typeof error==="string"||(typeof error==="object"&&!Array.isArray(error)))||
      Buffer.byteLength(JSON.stringify(error))>4096||!integer(receipt.meta.fee)||
      !Array.isArray(receipt.transaction?.signatures)||receipt.transaction.signatures.length!==1||
      receipt.transaction.signatures[0]!==record.signature||
      typeof receipt.transaction.message?.serialize!=="function"||
      !Buffer.from(receipt.transaction.message.serialize()).equals(wire.subarray(65)))
      throw new Error("Finalized failure receipt does not match journaled signed message");
    const before=state.intent.before;
    if(!positive(before?.slot)||before.slot>receipt.slot)
      throw new Error("Missing pre-submit inventory proof");
    const snapshot=await loadLiveMakerSnapshot(connection,{
      ...snapshotOptions,commitment:"finalized",maxLagSlots:state.limits.maxLagSlots,
      minContextSlot:Math.max(snapshotOptions.minContextSlot??0,receipt.slot),
    });
    const actual={inventory:Object.fromEntries(inventoryFields.map(k=>[k,String(snapshot.inventory[k])])),
      orders:sortedOrders(snapshot.ownedOrders),nextSequence:String(snapshot.header.nextSequence)};
    const expected={inventory:before.inventory,orders:sortedOrders(before.ownedOrders),
      nextSequence:before.nextSequence};
    const custody={base:String(snapshot.custodyState.totalBase),quote:String(snapshot.custodyState.totalQuote)};
    const vaults={base:String(snapshot.vaultAmounts.base),quote:String(snapshot.vaultAmounts.quote)};
    if(snapshot.pendingLazySettlement||!equal(actual,expected)||
      !equal(custody,before.custody)||!equal(vaults,before.vaults))
      throw new Error("Unexplained failed-transaction post-state; manual reconciliation required");
    sandbox(connection,control);
    if(control.recoverFinalizedFailure!==true)throw new Error("Finalized-failure recovery no longer approved");
    const audit={outcome:"finalized-failure",finalizedSlot:receipt.slot,slot:snapshot.slot,
      error,feeLamports:receipt.meta.fee,actual,custody,vaults};
    await retireWithArchive(sessionPath,state,record,audit);
    return {state:"reconciled-failure",signature:record.signature,slot:snapshot.slot,
      feeLamports:receipt.meta.fee,canSubmit:false,canRetry:false,requiresFreshPlan:true};
  });
}
