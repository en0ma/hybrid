# Milestone 82: Complete passive liquidity settlement

Status: planned / draft. Depends on milestone 81. Rebase onto main after predecessor merges; do not treat this plan as completed implementation.

## Scope and acceptance

Issue #68: versioned LP inventory and fee accounting, authenticated SPL exact-in/out both directions, active/passive crossing, overlapping positions, redemption, hostile rollback and migration review.

## Required exit gates

- [ ] Implementation and positive/negative tests committed.
- [ ] Exact-head CI green including SBF, CU, ELF, provenance and funded lifecycle gates (do not weaken existing limits).
- [ ] Document evidence and limitations; independent review of security-sensitive code.
- [ ] Review and merge sequentially with expected head SHA. No production activation without explicit approval.
