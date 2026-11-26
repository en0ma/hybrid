# Milestone 82: Complete passive liquidity settlement

Status: implementation in progress. Milestone 81 merged. No released account version exists; extend the existing position layout directly, without migration.

## Scope and acceptance

Issue #68: explicit LP inventory and fee accounting, authenticated SPL exact-in/out both directions, active/passive crossing, overlapping positions, redemption, hostile rollback and migration review.

## Required exit gates

- [ ] Implementation and positive/negative tests committed.
- [ ] Exact-head CI green including SBF, CU, ELF, provenance and funded lifecycle gates (do not weaken existing limits).
- [ ] Document evidence and limitations; independent review of security-sensitive code.
- [ ] Review and merge sequentially with expected head SHA. No production activation without explicit approval.
