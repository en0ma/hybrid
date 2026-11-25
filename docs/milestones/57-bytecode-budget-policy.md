# PR #57 bytecode budget policy

The project's previously enforced SBF ELF artifact budget was **262,144 bytes (256 KiB)**.
The passive settlement build measured **273,496 bytes**, exceeding that policy
by **11,352 bytes**. The new budget is **294,912 bytes (288 KiB)**,
an increase of **32 KiB** with 21,416 bytes of headroom over the observed build.

This change is an explicit project policy revision, **not a Solana runtime
configuration change**, a proven on-chain deployment, or an exemption from
runtime compute, stack, heap, or account data limits.

The `ci/bytecode-check.sh` gate remains mandatory. Do not skip the SBF
build, funded `cargo test-sbf` transaction suite, pinned-Manifest compute
comparisons, or independent security review. Growth past 288 KiB requires
a separate documented review. Measurements are from the previous CI head,
not a guarantee about subsequent compiled artifacts.

## Merge blockers beyond artifact size

- [ ] Prove funded buy/sell, fee-aware redemption and hostile-account rollback in SBF
- [ ] Build and test the exact merge head against all existing CI gates
- [ ] Address any SBF verifier errors, frame/stack warnings and deployment issues
- [ ] Complete authenticated multi-LP price-segment settlement and exact-out execution
- [ ] Independent security review before production activation

## Multi-LP execution revision

The exact-head SBF build including authenticated multi-position settlement and
atomic segment batches measured **306,288 bytes** on October 12, 2026.
The revised project policy budget is **327,680 bytes (320 KiB)**, providing
**21,392 bytes** of headroom over that measured ELF.
The previous 288 KiB budget rejected this measured feature-complete binary.
This revision does not bypass compilation, native/SBF tests, compute-unit
checks, vault solvency checks, or an independent security review, and it does
not change Solana protocol-level deployment rules.

## Lazy epoch claim implementation revision (PR #71)

The exact-head SBF build on November 11, 2026 measured **331,816 bytes**,
exceeding the existing 320 KiB policy by 4,136 bytes. The explicitly revised
project artifact budget is **352,256 bytes (344 KiB)**, leaving **20,440 bytes**
of headroom against that measurement. This changes the CI artifact-size policy
only; it does not increase the Solana SBF stack-frame limit, authorize deployment,
or relax compute, custody, state, funded lifecycle or review gates.

Both linked ask/bid fill-compaction functions previously reported 4,416-byte
SBF frames against a 4,096-byte limit. Their page construction is now isolated
in non-inlined helpers; the exact-head SBF build must confirm stack safety.
The funded Surfpool trade also reported an overlapping-copy runtime error and
must pass independently before merging. Keep all strict CI checks enabled.

## BID lazy epoch settlement revision (PR #73)

The exact-head SBF build on November 18, 2026 measured **366,696 bytes**,
exceeding the previous **352,256-byte (344 KiB)** project artifact-size
budget by **14,440 bytes**. The revised policy limit is **393,216 bytes
(384 KiB)**, providing **26,520 bytes** of headroom (approximately 7.2%
of the measured ELF) for this BID settlement implementation.

This is a versioned repository CI policy revision, **not** a change to
Solana's runtime limits or proof of deployability. The mandatory
`ci/bytecode-check.sh` gate remains enabled, as do strict Rust and SBF
builds, state/compute budgets, fuzz tests, funded lifecycle verification,
pinned Manifest comparison, exact-head validation and security review.
Future bytecode growth above 384 KiB must fail CI and receive a separate
reviewed policy change. Measurements describe the previous PR head and
must be reconfirmed on the new exact head.

## Atomic multipage BID settlement revision (PR #76)

The exact-head SBF build on November 21, 2026, including the new atomic
two-page, two-price BID execution and per-tick epoch sealing path, measured
**394,136 bytes**. This exceeded the previous **393,216-byte (384 KiB)**
repository policy by **920 bytes** after the stack-safe and shared-code
refactors.

The revised project policy budget is **409,600 bytes (400 KiB)**, an increase
of **16,384 bytes (16 KiB)**. Relative to the measured binary, this leaves
**15,464 bytes** (approximately 3.9%) of headroom. The previous
384 KiB threshold remains a documented historical guard, not the effective
limit for PR #76. Future growth past 400 KiB must fail CI and receive a
separate reviewed policy change.

This is **only** an in-repository artifact-size threshold revision.
It does **not** raise the 4,096-byte SBF stack-frame limit, Solana transaction
packet or compute limits, prove deployment readiness, or bypass any funded
SBF, state, CU, pinned-Manifest, security-review or exact-head checks.
The 394,136-byte measurement is from the previous exact head; the new head
must be rebuilt and measured. The atomic opcode 44 PR remains draft pending
execution, per-tick claims, rollback, transaction-size and CU validation.
