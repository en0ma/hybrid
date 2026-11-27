# Milestone 82: Complete passive liquidity settlement

Status: implementation in progress. Milestone 81 merged. No released account version exists; extend the existing position layout directly, without migration.

## Scope and acceptance

Issue #68: explicit LP inventory and fee accounting, authenticated SPL exact-in/out both directions, active/passive crossing, overlapping positions, redemption, hostile rollback and migration review.

## Required exit gates

- [ ] Implementation and positive/negative tests committed.
- [ ] Exact-head CI green including SBF, CU, ELF, provenance and funded lifecycle gates (use the operator-approved ELF ceiling below; all other existing limits remain unchanged).
- [ ] Document evidence and limitations; independent review of security-sensitive code.
- [ ] Review and merge sequentially with expected head SHA. No production activation without explicit approval.

## Operator-approved bytecode budget revision

On 2026-11-27 the operator explicitly authorized extending the ELF budget. The ceiling for `hybrid_program.so` is raised from 409,600 bytes (400 KiB) to 425,984 bytes (416 KiB), a 16 KiB / 4% increase. The preceding exact-head build at `3d5b180` measured 408,232 bytes and already passed the old ceiling; the revised ceiling provides bounded headroom for remaining M82 fixes, not a waiver of the ELF gate. Evidence: [deterministic core CI](https://github.com/en0ma/hybrid/actions/runs/46001700424/job/140514696663).

The same run subsequently failed the funded opcode-46 regression with an SBF heap allocation error. This is a distinct blocker; the ELF budget change does not alter runtime heap, compute, persistent state, review, or deployment requirements. Full exact-head CI and independent security review remain mandatory.
