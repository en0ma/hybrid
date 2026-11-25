# Milestone 79: Multi-epoch settlement hardening

Status: planned / draft. Depends on milestone 78. Rebase onto main after predecessor merges; do not treat this plan as completed implementation.

## Scope and acceptance

Execute at least three actual interleaved ASK/BID epochs; claim in varied order; reject cross-market and cross-epoch proof replay, stale IDs and vault deficits; reconcile all token liabilities and fees.

## Required exit gates

- [ ] Implementation and positive/negative tests committed.
- [ ] Exact-head CI green including SBF, CU, ELF, provenance and funded lifecycle gates (do not weaken existing limits).
- [ ] Document evidence and limitations; independent review of security-sensitive code.
- [ ] Review and merge sequentially with expected head SHA. No production activation without explicit approval.
