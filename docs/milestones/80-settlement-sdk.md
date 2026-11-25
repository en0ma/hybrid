# Milestone 80: SDK and settlement automation

Status: planned / draft. Depends on milestone 79. Rebase onto main after predecessor merges; do not treat this plan as completed implementation.

## Scope and acceptance

Typed builders for bounded execution; authenticated account discovery, ALT lifecycle, epoch indexing, proof vectors and claim recovery; quote/build/simulate parity; opt-in submission only after evidence.

## Required exit gates

- [ ] Implementation and positive/negative tests committed.
- [ ] Exact-head CI green including SBF, CU, ELF, provenance and funded lifecycle gates (do not weaken existing limits).
- [ ] Document evidence and limitations; independent review of security-sensitive code.
- [ ] Review and merge sequentially with expected head SHA. No production activation without explicit approval.
