# Milestone 78: Multipage ASK lazy settlement

Status: bounded implementation committed; final verification pending. Depends on milestone 77. Rebase onto main after predecessor merges; do not treat this plan as completed implementation.

## Scope and acceptance

Specify separate ASK ABI; authenticate linked pages and makers; per-tick epoch commitments, checked fees and SPL custody; funded crossing, claims and hostile rollback; preserve opcode 41.

## Required exit gates

- [ ] Implementation and positive/negative tests committed.
- [ ] Exact-head CI green including SBF, CU, ELF, provenance and funded lifecycle gates (do not weaken existing limits).
- [ ] Document evidence and limitations; independent review of security-sensitive code.
- [ ] Review and merge sequentially with expected head SHA. No production activation without explicit approval.

## Opcode 45 implementation

The branch includes merged milestone 77 (4f419dc). Opcode 45 accepts a 17-byte instruction [45, base_fill:u64LE, max_quote_in:u64LE], exactly two linked ASK page/owner pairs, two fresh frozen epochs, and 2–8 canonical maker-balance PDAs after 20 fixed accounts. The maker accounts are read-only for execution; opcode 40 debits historical locked base at claim. Opcode 41 remains unchanged. Fees are calculated per tick on quote proceeds and included in the taker's maximum quote input. Head and tail owner sidecars, maker PDAs, vaults, mints, system and token programs, distinct account roles and historical reserve floors are authenticated before transfers. The existing linked ASK compactor preserves the two-page cancellation ABI.

Funded integration tests cover partial three-maker settlement (350 base, 505 quote including 5 fee atoms), all three claims, malformed accounts and a frozen taker base destination after the incoming quote transfer. A separate full eight-maker fixture expects 900 base delivered, 2,727 quote paid, 2,700 quote claimed, 27 quote fee atoms, zero pending debits/credits and all epoch seats claimed. These values are test expectations, not independently observed measurements. Exact-head CI must establish packet/CU/ELF evidence and test results before merging. Production SDK activation and independent security review remain outstanding.
