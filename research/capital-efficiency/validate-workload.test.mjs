import test from "node:test";import assert from "node:assert/strict";import {readFileSync} from "node:fs";import {validateWorkload} from "./validate-workload.mjs";
const fixture=JSON.parse(readFileSync(new URL("./paired-workload.json",import.meta.url),"utf8"));
test("required paired scenarios and funding match contract",()=>assert.deepEqual(validateWorkload(fixture),[]));
test("rejects absent stress scenario",()=>{const x=structuredClone(fixture);x.operations.pop();assert.match(validateWorkload(x).join(","),/adverse-price/)});
test("rejects non-integer negative funding",()=>{const x=structuredClone(fixture);x.funding.makerBaseAtoms="-1";assert.match(validateWorkload(x).join(","),/makerBaseAtoms/)});
test("rejects unequal-conditions declaration",()=>{const x=structuredClone(fixture);x.fairness.identicalFillQuantities=false;assert.match(validateWorkload(x).join(","),/identicalFillQuantities/)});
