# Milestone 84: Empirical competitive benchmarks

Status: implementation started on merged M83 baseline `ecbe68a0c0e2c98378766fcf5f8f0ca3d9266e4c`. No performance ranking or complete five-scenario result claimed.

## Acceptance

Five equivalent executed Hybrid/Manifest workloads with pinned revisions, independently audited receipts, total execution plus claim CU, rent, packet/metas and inventory utilization. Existing paired workload contract: [research/capital-efficiency/paired-workload.json](../../research/capital-efficiency/paired-workload.json). Preserve all existing security, CU and ELF gates.

## Initial executable comparison

The first increment executes **two preliminary full-fill ASK cases**, one maker and eight makers, on the same offline Surfpool instance with the same mint pair, wallet identities, integer funding, order sizes and gross price. These are narrow subcases of the one-maker and fanout workloads, not certification of the full five-workload contract. Full fill at one price eliminates different partial FIFO/pro-rata allocation as a confounder. Hybrid lazy epoch execution and every maker claim are included; Manifest local core Swap and every maker withdrawal to the same external-wallet endpoint are included. Report fee differences separately; gross fill is matched, not net fee-adjusted economics. No Manifest global/reverse or Hybrid passive liquidity claims.

Both programs are built from the pinned full commits (Hybrid exact checked-out PR SHA, Manifest `d04b7aa90b098ba64ebcdf398e9a24d89a9114d8`), deployed using local Surfpool only, and ELF hashes recorded. Mint/token funding and initial Hybrid empty market/custody/book accounts are seeded using local fixture RPC; actual maker seat/balance creation, deposits, placements, trade and payout run as signed SBF transactions. Manifest market initialization is executed; Hybrid seeded initialization is explicitly not an equivalent creation-cost comparison. Account-state rent is actual allocated bytes and lamports, not claimed fee expenditure. Shared mint/external wallet token accounts are excluded from protocol-owned/vault footprint. Each finalized receipt independently matches signed wire, target program and market; actual CU, packet bytes, account metas and fee lamports are reported by stage. Post-funding, pre-trade, post-trade and final snapshots verify maker positions, vaults and external SPL deltas. Inventory utilization is resting base committed divided by initial maker base funding; not a claim about global collateral reuse or realized yield.

The CI gate must fail if either measured case fails. The remaining two-market inventory, partial-fill cancel/replace/retry and adverse collateral stress scenarios remain explicitly unmeasured. Next increments implement their economic parity and protocol-specific failure/receipt audit. No fabricated comparison rows.

## Reproduce

See `.github/workflows/empirical-benchmarks.yml`; the runner requires exact pinned build artifacts and a disposable offline loopback RPC. The workflow uploads `m84-empirical.json` even on a failure; only a successful validated run constitutes evidence. No public endpoint or secret keys.

## Exit gates

- [ ] All five equivalent workloads implemented and positive/negative tests committed.
- [ ] Exact-head CI green; preserve all existing SBF, CU and ELF limits.
- [ ] Evidence and limitations documented; independent review of security-sensitive code.
- [ ] Sequential merge with expected head SHA. No production activation without explicit approval.

## CI setup correction

At initial head `5f42d41`, six existing workflows passed, code review was clean, but [new empirical workflow](https://github.com/en0ma/hybrid/actions/runs/46116124764) failed with zero jobs. Move runner-dependent paths into a step using `GITHUB_ENV`, use the same pinned Agave v4.3.0 and Surfpool 1.5.0 as PR Gate (record container image ID), bind the RPC port to loopback, and run for every main-target PR head so docs-only updates do not silently omit evidence. Persist per-protocol partial receipts and pending attempt status in the failure report, without signed wire or keys. No verified comparison numbers yet.

## HTTP-only finality correction

At `f79dfce`, the empirical job successfully installed the pinned tools, built both SBF programs, passed the receipt unit tests and started offline Surfpool. Codex P1 identified the unpublished WebSocket port: web3.js confirmation waits for signature subscription while only HTTP is published. Replace *all* subscription-based confirmations (including funding) with bounded HTTP signature history polling, explicit finalized bank slot and exact finalized receipt verification; funding additionally checks finalized lamports. No send retry on missing status. HTTP fetch timeout 10 seconds, no redirects or 429 retry; execution step bounded to 20 minutes. Add positive and negative polling regressions. The original run was still hung at execution when inspected; no comparative numbers verified. Fresh CI and review required.

## Runtime compatibility and snapshot diagnostics

At `d9003ea`, [empirical CI](https://github.com/en0ma/hybrid/actions/runs/46120439251) passed all six receipt/finality tests, pinned builds and offline startup, then failed with `Maker snapshot is stale` rather than hanging. Codex exact-head review found no new issues; the old WebSocket issue is addressed. Existing funded lifecycle succeeded with Surfpool 1.8.0 ([run](https://github.com/en0ma/hybrid/actions/runs/46120439236)), while this lane used older 1.5.0. Pin the known working 1.8.0 release and record its binary hash; this is a compatibility hypothesis, not a proven root cause. Keep the Agave v4.3.0 pin and all existing SDK freshness/ownership checks. Add actual RPC snapshot context-slot and observed-slot diagnostics and minimum slot anchored to the last finalized receipt; print bounded public partial receipt evidence on failure. No CU comparison verified yet.

## Independent paired-case CI shards and incremental evidence

At `0c05986`, the existing funded lifecycle, core PR Gate ([green rerun](https://github.com/en0ma/hybrid/actions/runs/46122082839)) and code review passed, with zero unresolved findings; the empirical [run](https://github.com/en0ma/hybrid/actions/runs/46122072320) was still executing when inspected. Each case now runs in its own disposable CI job with both programs sharing that case's identical mint/funding/actor fixture. Parallelize only distinct disposable actor funding, not protocol trade or settlement. Checkpoint the report by atomic rename before submission and after every finalized receipt, logging only public receipt metadata. Each shard must validate the exact head, Manifest pin, program IDs, ELF and runtime hashes, selected case and complete receipts; an aggregate job downloads both and requires both successful shards. No cross-host latency ranking is implied; each comparison is within its own runner. These are still only the first two preliminary cases, not the entire five-workload contract. No verified numbers claimed until the actual runs finish.

## Verified baseline and two-market local inventory increment

Exact head `6ce9545` passed all seven workflows, clean code review and no unresolved threads. [Empirical run 46122963231](https://github.com/en0ma/hybrid/actions/runs/46122963231) aggregate job 140914208222 independently validated both artifacts; [PR Gate](https://github.com/en0ma/hybrid/actions/runs/46122963216). Surfpool 1.8.0, Agave v4.3.0, Manifest pin above. Observed values (not estimates):

| Full-fill workload | Protocol | Execution CU | Payout CU | Total settlement CU | Total settlement bytes | Max packet bytes | Max account keys | Settlement fees (lamports) |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| One maker | Hybrid | 65,924 | 225,950 | 291,874 | 1,361 | 725 | 18 | 10,000 |
| One maker | Manifest | 8,595 | 3,253 | 11,848 | 849 | 462 | 10 | 10,000 |
| Eight makers | Hybrid | 97,179 | 1,910,052 | 2,007,231 | 6,589 | 733 | 18 | 45,000 |
| Eight makers | Manifest | 14,962 | 26,205 | 41,167 | 3,558 | 462 | 10 | 45,000 |

Hybrid lazy claims dominate CU here. This unfavorable result is retained rather than hidden or changed retroactively. Both protocols paid out to external wallets, but these narrow full-fill ASK cases do not measure passive liquidity, global orders or optimized batch claims. Each pair used a separate runner; no wall-clock ranking. Setup costs not economically equivalent; Hybrid empty market seed excludes signed creation. See uploaded artifacts for footprint and raw receipts.

Third shard implements the two-market local-inventory case: same maker and taker, same mint pair and one shared base/quote wallet per actor per protocol (no top-ups between markets). Two simultaneous one-level ASK books at prices 1 and 2 (two distinct price levels across the markets), 100 base atoms each. Verify both open books, collateral vaults and shared-wallet 200 atom debit before either execution, then signed full fills and all payouts, final shared wallet totals and protocol fee difference (3 vs 0 quote atoms). Independent validation requires distinct market and wallet account addresses and receipt signatures, both open-state proofs before execution, and per-market equivalent gross fills and measured receipt costs. No global collateral reuse or two price levels *within each market* claimed. Remaining partial-fill/cancel/replace/retry and adverse-price collateral stress cases unmeasured. New exact-head run and review required.

## Verified two-market baseline; partial active-fill/replacement increment

Exact head `630a197` passed all seven CI workflows and clean code review. The [three-workload aggregate](https://github.com/en0ma/hybrid/actions/runs/46137796971) (job 140959886892) independently validated both-market shared wallet evidence. At prices 1 and 2, respectively, Hybrid settlement consumed 309,874 and 294,857 CU (604,731 total), Manifest 11,848 and 11,848 CU (23,696 total). Hybrid payout CU 239,450 and 227,450; Manifest payout CU 3,253 each. Settlement packet totals 1,361 bytes per Hybrid market, 849 per Manifest market; 10,000 lamports per market on both. Actual receipt metrics can vary by PDA bump. This measures local inventory, not global collateral reuse.

Fourth shard implements two makers with 100 base atoms each at ASK prices 1 and 2, a real 50-atom active partial fill at the best price (no lazy fee), finalized cancellation of its remaining 50, fresh state read, replacement of those 50 at price 3, and quote payout of 50 atoms. The replacement is sent exactly once; inject a lost acknowledgement locally after real send, then reconcile the original precomputed signature to its exact finalized wire/receipt without resubmitting. This is controlled fault injection, not proof of recovery after an actual RPC network outage. Compare state of both makers, sequence identities, balances, all three external wallets, both vaults and account footprint after every phase; unrelated maker must remain unchanged. Final 150 base atoms remain locked at prices 2 and 3. Report total receipt CU (including cancel/replace), and execution/payout subset separately; do not compare active partial fill against lazy full-fill as if identical paths. Adverse-price stress remains pending. Fresh exact-head CI required.

## Verified partial-fill baseline and adverse-price stress implementation

Exact head `262ed0d` passed all seven workflows, clean code review and zero unresolved findings. [Four-workload aggregate](https://github.com/en0ma/hybrid/actions/runs/46140590544), job 140968258361: Hybrid 198,690 CU total across 11 transactions, 84,631 CU execution plus payout (70,048 + 14,583); Manifest 61,819 CU total, 11,736 CU execution plus payout (8,483 + 3,253). Settlement bytes 1,078 vs 849, max packet 592 vs 462, both 10,000 lamports settlement fees. Controlled acknowledgement loss recovered original signature without second send. Actual transport outages remain fail-closed.

Fifth shard: two local markets with the same two makers and taker and shared wallets, each book initially at prices 2 and 3, 100 base atoms each. Both markets fully funded before any mutation. Adverse reprice maker 0 from 2 to 1 via finalized cancel and replace; real 50-atom active fill at 1. Attempt to withdraw one base atom despite zero free base and remaining locked inventory: require exact insufficient-funds simulation rejection, then deliberately submit only that signed negative-test transaction on the isolated local runtime (skip preflight), require finalized same error and exact wire, and verify no mutation of protocol accounts, collateral, or SPL wallets. Include failed transaction CU/fees separately from successful settlement totals. Payout remaining quote credit; prove the other market's complete protocol-owned state hashes unchanged through each market's lifecycle and exact shared wallet conservation. This models adverse ASK repricing and locked-collateral withdrawal, not oracle-driven liquidation, cross-market credit or global order collateral. All five cases implemented subject to fresh funded CI and review; no result for the fifth claimed yet.
