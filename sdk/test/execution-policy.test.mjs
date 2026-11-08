import test from "node:test";
import assert from "node:assert/strict";
import {admitMakerPhase} from "../src/execution-policy.mjs";
const safe={
  enabled:true,killSwitch:false,pendingMutation:false,
  operatorApproved:true,simulationPassed:true,
  snapshotSlot:100,currentSlot:101,maxLagSlots:4,
  cancellations:[],placements:[],maxChanges:8,
};
test("default, kill switch, simulation and approval all fail closed",()=>{
  assert.throws(()=>admitMakerPhase(),/disabled/);
  assert.throws(()=>admitMakerPhase({...safe,killSwitch:true}),/disabled/);
  assert.throws(()=>admitMakerPhase({...safe,pendingMutation:true}),/Pending/);
  assert.throws(()=>admitMakerPhase({...safe,operatorApproved:false}),/approval/);
  assert.throws(()=>admitMakerPhase({...safe,simulationPassed:false}),/simulation/);
});
test("cannot mix cancellations and placements or use stale state",()=>{
  assert.throws(()=>admitMakerPhase({...safe,cancellations:[1],placements:[2]}),/separate/);
  assert.throws(()=>admitMakerPhase({...safe,currentSlot:110}),/Stale/);
  assert.throws(()=>admitMakerPhase({...safe,currentSlot:99}),/Stale/);
  assert.throws(()=>admitMakerPhase({...safe,snapshotSlot:0}),/Stale/);
  assert.throws(()=>admitMakerPhase({...safe,maxChanges:0,placements:[1]}),/cap/);
});
test("successful preflight returns advisory single-phase intent only",()=>{
  const first=admitMakerPhase({...safe,cancellations:[{sequence:1n}]});
  assert.equal(first.phase,"cancel");
  assert.equal(first.requiresDurableJournalAndFreshState,true);
  assert.equal(first.actions.length,1);
  const second=admitMakerPhase({...safe,placements:[{side:"bid"}]});
  assert.equal(second.phase,"place");
  assert.deepEqual(admitMakerPhase(safe),{phase:"noop",actions:[],slot:100});
});
