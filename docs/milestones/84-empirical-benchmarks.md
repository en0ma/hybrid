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
