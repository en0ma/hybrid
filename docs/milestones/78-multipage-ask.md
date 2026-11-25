# Milestone 78: Multipage ASK lazy settlement

Status: planned / draft. Depends on milestone 77. Rebase onto main after predecessor merges; do not treat this plan as completed implementation.

## Scope and acceptance

Specify separate ASK ABI; authenticate linked pages and makers; per-tick epoch commitments, checked fees and SPL custody; funded crossing, claims and hostile rollback; preserve opcode 41.

## Required exit gates

- [ ] Implementation and positive/negative tests committed.
- [ ] Exact-head CI green including SBF, CU, ELF, provenance and funded lifecycle gates (do not weaken existing limits).
- [ ] Document evidence and limitations; independent review of security-sensitive code.
- [ ] Review and merge sequentially with expected head SHA. No production activation without explicit approval.
