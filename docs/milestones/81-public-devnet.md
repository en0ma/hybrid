# Milestone 81: Funded Surfpool SDK integration and settlement verification

Status: implementation in progress. Depends on merged milestone 80.
Scope revised at operator direction: public devnet deployment is not an exit gate for this milestone. Public-network readiness remains a separate requirement.

## Acceptance

- Compile and deploy the fixed-ID SBF program to an isolated loopback Surfpool runtime.
- Preserve the existing funded six-stage active-order lifecycle gate.
- Exercise SDK-derived execution and claim transactions against actual SBF, including bounded ASK/BID single-page and multipage cases, actual v0/ALT packing, and multiple independent frozen epochs.
- Recover claims from persisted rosters and durable signed intents; independently inspect RPC receipts, epoch bitmaps, reserve liabilities, maker balances, vault amounts and fees.
- Reject replay and unsafe recovery; retain non-secret evidence only.
- Keep existing ABI, compute, ELF, fuzz and provenance gates unchanged.

Surfpool-only deployment and fixture seeding are permitted. These are not public deployment or permissionless provisioning evidence. No public RPC or real funds are required.

## Exit gates

- [ ] Actual signed SDK lifecycle receipts and independently verified state reconciliation.
- [ ] Positive and negative tests, exact-head CI green.
- [ ] Document measured evidence, fixture assumptions and outstanding independent security review.
- [ ] No production activation.
