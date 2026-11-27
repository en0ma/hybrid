# Milestone 82: Complete passive liquidity settlement

Status: implementation and funded regression suite verified at `ea3fb31`; final exact-head and independent security review gates pending. Milestone 81 merged. No released account version exists; extend the existing position layout directly, without migration.

## Scope and acceptance

Issue #68: explicit LP inventory and fee accounting, authenticated SPL exact-in/out both directions, active/passive crossing, overlapping positions, redemption, hostile rollback and migration review.

## Required exit gates

- [x] Implementation and positive/negative tests committed.
- [ ] Exact-head CI green including SBF, CU, ELF, provenance and funded lifecycle gates (use the operator-approved ELF ceiling below; all other existing limits remain unchanged).
- [ ] Document evidence and limitations; independent review of security-sensitive code.
- [ ] Review and merge sequentially with expected head SHA. No production activation without explicit approval.

## Operator-approved bytecode budget revision

On 2026-11-27 the operator explicitly authorized extending the ELF budget. The ceiling for `hybrid_program.so` is raised from 409,600 bytes (400 KiB) to 425,984 bytes (416 KiB), a 16 KiB / 4% increase. The preceding exact-head build at `3d5b180` measured 408,232 bytes and already passed the old ceiling; the revised ceiling provides bounded headroom for remaining M82 fixes, not a waiver of the ELF gate. Evidence: [deterministic core CI](https://github.com/en0ma/hybrid/actions/runs/46001700424/job/140514696663).

The same run subsequently failed the funded opcode-46 regression with an SBF heap allocation error. This is a distinct blocker; the ELF budget change does not alter runtime heap, compute, persistent state, review, or deployment requirements. Full exact-head CI and independent security review remain mandatory.

## Verified implementation evidence (2026-11-27)

Reviewed code head: `ea3fb31998ba619faa0211c3ac090e6dd3f956ff`. [Deterministic CI](https://github.com/en0ma/hybrid/actions/runs/46072836098/job/140753596159) passed formatting, strict Clippy, native tests, persistent state budgets, SBF compilation and funded tests, fuzz smoke runs, and all CU budgets. ELF: 410,432 / 425,984 bytes (15,552 bytes remaining). The [Manifest baseline, CU comparison and mainnet-fork jobs](https://github.com/en0ma/hybrid/actions/runs/46072836098) also passed. All six supplementary workflows passed, including [funded loopback Surfpool lifecycle](https://github.com/en0ma/hybrid/actions/runs/46072836109) and [client SDK verification](https://github.com/en0ma/hybrid/actions/runs/46072836160).

The funded SBF suite covers exact-out atom rounding in both directions, LP inventory and fee redemption, authenticated multi-position settlement, hybrid ASK/BID page compaction, non-square ask price eligibility, and five consecutive active legs followed by passive settlement in each direction. Opcode 46 checks precise late SPL insufficient-funds and aggregate-limit errors and equality of rollback snapshots. The original two-leg tests retain the 200,000 CU test meter; the five-active-leg stress cases use a test-only 1,000,000 CU meter with the default heap to isolate allocation behavior. All production CU and persistent-state budgets remain unchanged. These tests do not establish that every maximum-size route fits a 200,000 CU transaction.

[Codex review](https://github.com/en0ma/hybrid/pull/82#issuecomment-7176259358) reported no new findings at the reviewed code head. All six historical review threads were resolved with regression evidence. The original PR Gate run failed only its review-thread and dependent merge-gate jobs, which ran before thread resolution. The documentation-only update requires fresh exact-head CI; the separate security review has been requested but no completed security-review receipt is recorded. No public devnet or mainnet activation is authorized by these results.
