# Milestone 81: Public devnet active execution

Status: planned / draft. Depends on milestone 80. Rebase onto main after predecessor merges; do not treat this plan as completed implementation.

## Scope and acceptance

Authorized deploy, real independent funded wallets and six-stage public devnet lifecycle; independent RPC auditor and non-secret evidence; no injected accounts or private keys in artifacts.

## Required exit gates

- [ ] Implementation and positive/negative tests committed.
- [ ] Exact-head CI green including SBF, CU, ELF, provenance and funded lifecycle gates (do not weaken existing limits).
- [ ] Document evidence and limitations; independent review of security-sensitive code.
- [ ] Review and merge sequentially with expected head SHA. No production activation without explicit approval.
