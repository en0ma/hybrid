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
