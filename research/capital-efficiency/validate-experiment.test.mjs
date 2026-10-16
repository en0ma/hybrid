import test from "node:test";import assert from "node:assert/strict";import {readFileSync} from "node:fs";import {validateTemplate} from "./validate-experiment.mjs";
const input=JSON.parse(readFileSync(new URL("./experiment-template.json",import.meta.url),"utf8"));
test("benchmark template covers five scenarios and provenance",()=>assert.deepEqual(validateTemplate(input),[]));
test("detect missing signing latency",()=>{const t=structuredClone(input);t.metrics=t.metrics.filter(x=>x!=="signingLatencyMs");assert.match(validateTemplate(t).join(","),/signingLatencyMs/)});
test("detect missing adversarial case",()=>{const t=structuredClone(input);t.cases=t.cases.filter(x=>x.name!=="adverse-price-collateral-stress");assert.match(validateTemplate(t).join(","),/adverse-price/)});
test("detect missing Hybrid provenance",()=>{const t=structuredClone(input);delete t.hybridCommit;assert.match(validateTemplate(t).join(","),/both Git revisions/)});
