# P2 — Capital efficiency and advanced orders

Research workstream, **not a deployed protocol feature**. Before changing custody or ABI semantics, measure equivalently funded maker scenarios versus Manifest's capital efficiency and reverse-order mechanics.

## Candidate designs

| Mechanism | Potential value | Principal concern |
| --- | --- | --- |
| Per-market locked collateral (baseline) | Explicit solvency by market | Fragmentation of inventory |
| Shared collateral across markets | Less idle inventory | Cross-market insolvency / race conditions |
| Automated reverse orders | Faster maker replenishment | Inventory drift, authorization, event ordering |
| Atomic cancel-and-replace | Lower market-maker quote downtime | Account locks, compute, ordering |
| Passive liquidity settlement expansion | Broader taker fill paths | Pricing, fee and redemption accounting |

## Required benchmark scenarios

1. A market with one maker and one price level.
2. Eight distinct makers per trade to test account fan-out.
3. Two markets with the same maker inventory to measure shared versus fragmented capital.
4. Partial fill, cancel, replace, and network retry sequence.
5. Adverse price moves to test collateral reserves and quote skew.

Measure *compute units, transaction count, account metas, rent bytes, signing latency, actual fill price, token conservation, inventory utilization*. Reproduce on identical hardware/toolchain and an equivalent workload using a pinned Manifest commit. Do not infer superiority from unrelated pinned unit benchmarks.

## Design gates

- Formalized authorization and mint/vault ownership invariants
- Conservation proofs and multi-market adversarial simulations
- Race/replay and stale-book tests
- Account layout and ABI migration plan
- Fuzz/SBF/CU regressions and devnet transactions

Outcome is a decision record with measured trade-offs and a follow-up implementation PR for whichever mechanism wins.
