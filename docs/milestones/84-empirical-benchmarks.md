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
