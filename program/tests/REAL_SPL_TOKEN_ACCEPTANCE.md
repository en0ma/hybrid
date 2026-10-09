# Real SPL Token integration acceptance criteria

**Status: not implemented.** The existing `program/tests/swaps.rs`,
`program/tests/sell_swaps.rs` and `program/tests/passive_custody.rs`
register `mock_token_process` under the SPL Token program ID. They test
Hybrid settlement logic but do **not** prove that real SPL Token
`Transfer` CPIs, vault signing, token authority and account validation
work end to end.

## Required integration harness

- Register the **actual SPL Token processor** in `solana-program-test`.
  Do not override `TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA`
  with a mock in the real-token integration suite.
- Initialize actual SPL Token mint and account states via real token
  instructions. Use correct rent-exempt account allocation and mint /
  authority setup, including vault authority PDA control.
- Execute Hybrid instructions through `solana-program-test`, not pure
  Rust transition functions alone.
- Test transactions with distinct maker, taker, and fee-payer identities.
- Assert *both* serialized Hybrid state and unpacked SPL Token account
  balances/owners after each transaction.
- Compare complete pre/post account data and balances on expected failure;
  assert atomic rollback and absence of unauthorized token movements.
- Keep the existing mock-based tests as fast regression coverage, but run
  the real-token tests in the mandatory SBF CI lane.

## Acceptance matrix

| Test | Required on-chain result |
| --- | --- |
| Create custody + maker balances | Vault mints and PDA authority validated |
| Deposit base/quote | Real SPL transfer + free balance + custody totals reconcile |
| Place/cancel asks and bids | Required collateral locked/released; vault tokens unchanged |
| Buy exact-in / exact-out | Taker token deltas match ask fills, maker quote credits and vault balances |
| Sell exact-in / exact-out | Taker token deltas match bid fills, maker base credits and vault balances |
| Partial fills / multiple makers | Remaining page quantities and owner sidecars stay aligned |
| Slippage, underfunding, insufficient depth | All relevant accounts identical before and after failed transaction |
| Wrong mint / forged vault authority | Reject without moving funds |
| Wrong maker balance / sidecar | Reject without moving funds |
| Taker account alias / wrong owner | Reject without moving funds |
| LP deposit and principal redemption | Real SPL transfers, segregated reserves and vault coverage preserved |
| LP duplicate close / short vault | Reject and roll back the full transaction |

## Invariants

For every successful transaction, real token vault balances must cover all
recorded active collateral, passive reserves and fees. No instruction may
credit or debit a maker's collateral without the corresponding custody
movement or documented internal balance transfer. Rejected transactions
must not mutate token accounts, maker balances, order pages, ownership
sidecars, pool state or custody totals.

The tests must enforce exact amounts including rounding, rather than only
asserting that a transaction succeeds. All four native swap opcodes
(15, 16, 17, 18) must be exercised against the real token processor.

## Implementation / merge criteria

This document is **not** a substitute for the real-token tests.
Do not treat this milestone as complete until the real test harness is
implemented, formatted with `cargo fmt --all`, and passes all standard
native, SBF, fuzz, CU, fork and Manifest checks. Production trading still
requires an independent security review and operational controls.
