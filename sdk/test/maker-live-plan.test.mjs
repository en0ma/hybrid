import test from "node:test";
import assert from "node:assert/strict";
import {planLiveMakerQuotes} from "../src/maker-live-plan.mjs";
test("policy fails closed before RPC",async()=>{
  const connection={async getGenesisHash(){throw new Error("unexpected RPC");}};
  await assert.rejects(planLiveMakerQuotes(connection,{policy:{}}),/disabled/);
  await assert.rejects(planLiveMakerQuotes(connection,{policy:{enabled:true,killSwitch:false}}),/Pending mutation/);
  await assert.rejects(planLiveMakerQuotes(connection,{policy:{enabled:true,killSwitch:false,pendingMutation:false}}),/maxLagSlots/);
});
