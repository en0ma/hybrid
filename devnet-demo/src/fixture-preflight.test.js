import test from "node:test";
import assert from "node:assert/strict";
import {fixturePreflightReport} from "./fixture-preflight.js";
test("preflight fails closed when required tools or program deployment are missing",()=>{
 const report=fixturePreflightReport({tools:{surfpool:true,solana:false,cargo:true},
  programExecutable:false,rpcReachable:true,fullBookConfigured:false});
 assert.equal(report.readyForProvisioning,false);
 assert.match(report.missing.join(";"),/solana CLI/);
 assert.match(report.missing.join(";"),/Deploy Hybrid/);
 assert.match(report.missing.join(";"),/HYBRID_BID_OWNER0/);
});
test("preflight ready only means provisioning prerequisites, not lifecycle execution",()=>{
 const report=fixturePreflightReport({tools:{surfpool:true,solana:true,cargo:true},
  programExecutable:true,rpcReachable:true,fullBookConfigured:true});
 assert.equal(report.readyForProvisioning,true);
 assert.match(report.evidence,/No signed transactions/);
});
