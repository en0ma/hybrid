# Milestone 77: Scale multipage BID settlement

Status: implementation under review; runtime verification pending. Baseline: merged milestone 76.

## Scope and acceptance

Measure legacy/v0/ALT packet, CU and ELF bounds; extend roster or tick envelope only after funded SBF execution and all claims; preserve opcode 44 ABI; reject malformed accounts, insufficient collateral, and late CPI failures. Start with packet and account fan-out characterization.

## Required exit gates

- [x] Implementation and positive/negative tests committed for the bounded 16-row slice.
- [ ] Exact-head CI green including SBF, CU, ELF, provenance and funded lifecycle gates (do not weaken existing limits).
- [ ] Document evidence and limitations; independent review of security-sensitive code.
- [ ] Review and merge sequentially with expected head SHA. No production activation without explicit approval.

## Implemented execution slice

Opcode 44 preserves the 17-byte instruction and 20 fixed accounts, while raising the total canonical maker-row cap from 8 to 16. The executable envelope remains exactly two linked pages and two executed ticks. Each tick retains its own immutable commitment, root and fees. The production SDK builder remains disabled. No changes to opcodes 41–43, custody accounting, claim ABI or existing CU/ELF limits.

The funded 16-maker fixture uses two high-price rows (300 base total) and fourteen half-price rows (1,400 base total), with 1,000 quote collateral, a bank-loaded ALT and a signed v0 transaction. Expected complete settlement: 1,700 base deposited, 1,000 quote paid, 1,683 base claimed and 17 base fee atoms remaining. Both epoch bitmaps and pending liabilities must exhaust. Negative cases cover reordered, readonly, oversized and omitted maker accounts and slippage, with full observed writable-account snapshots. Existing opcode 44 small-roster CPI rollback tests remain enabled. An expanded-roster late-CPI test now freezes the taker quote destination and checks the exact propagated SPL AccountFrozen error (17), ordered runtime token CPI success/failure and unchanged observed writable accounts. The test is committed but not yet verified by CI. Independent review remains outstanding.

The 8/9/16/32-maker serialization characterization uses synthetic ALT contents and is not proof of execution at 32 makers. Prior-head SBF logs at `43351ae` (GitHub Actions run 45721781500, job 139581727766) record funded 16-maker execution at 428 serialized bytes and 307,178 CU. The same run recorded synthetic legacy/v0 sizes of 1182/412 (8), 1215/414 (9), 1446/428 (16), and 1974/460 (32) bytes. This run failed a later rollback log assertion; these are observed prior-head measurements, not a green full-CI result. Reverify on the final head. Further tick/page expansion is intentionally deferred pending these gates.
