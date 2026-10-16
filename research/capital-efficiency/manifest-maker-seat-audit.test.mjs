import test from "node:test";import assert from "node:assert/strict";import {auditManifestMakerSeat} from "./manifest-maker-seat-audit.mjs";
const args=()=>({makerOwner:"maker",filledBaseAtoms:"40",quoteSpentAtoms:"20",before:{seats:[{trader:"maker",baseFreeAtoms:"0",quoteFreeAtoms:"0"}]},after:{seats:[{trader:"maker",baseFreeAtoms:"0",quoteFreeAtoms:"20"}]}});
test("checks exact maker quote proceeds for isolated ask fill",()=>{const x=auditManifestMakerSeat(args());assert.equal(x.makerSeatChecked,true);assert.equal(x.verified,false)});
test("rejects unmatched quote proceeds",()=>{const x=args();x.after.seats[0].quoteFreeAtoms="19";assert.throws(()=>auditManifestMakerSeat(x),/quote proceeds/)});
test("rejects missing seat or changed base free balance",()=>{const x=args();x.after.seats=[];assert.throws(()=>auditManifestMakerSeat(x),/one maker seat/);const y=args();y.after.seats[0].baseFreeAtoms="1";assert.throws(()=>auditManifestMakerSeat(y),/base withdrawable/)});
