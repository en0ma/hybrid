# Rust program lifecycle verification matrix

PR #59 provides a wallet frontend and receipt verifier. These complement,
rather than replace, the existing funded SBF integration tests in
`program/tests/passive_swap_settlement.rs` on `main`.

## Executable Rust settlement coverage

The following **actual existing test functions** cover genuine SPL Token
movement through the deployed processor in Solana ProgramTest:

| Lifecycle / adversary | SBF test |
| --- | --- |
| Funded LP swaps followed by fee-aware redemption | `funded_buy_sell_then_fee_aware_lp_close_conserves_tokens` |
| Multi-LP funded buy/sell and exact-out settlement | `funded_multi_lp_buy_sell_and_exact_out_preserve_all_claims` |
| Redeem remaining post-trade inventory for all LPs | `multi_lp_positions_redeem_all_post_trade_inventory` |
| Cross into next LP price range inside one batch | `atomic_batch_crosses_into_next_lp_range_at_exact_price_boundary` |
| Roll back earlier transfers on later batch error | `multi_segment_batch_reverts_all_earlier_spl_movements_on_later_error` |
| Reject counterfeit vault | `counterfeit_base_vault_is_rejected_atomically` |
| Reject malicious single/multi LP fee | `attacker_cannot_lower_single_lp_fee_or_change_vaults`, `attacker_cannot_lower_multi_lp_fee_or_change_vaults` |

Run the existing suite using the repo's pinned Solana SBF tools and CI
environment (see `.github/workflows`). The full repository PR Gate
already runs funded SBF tests; it was green for PR #58 and previously
for PR #59. No edits to the Rust processor are needed for this matrix.

## Missing end-to-end devnet evidence

These SBF tests are **not proof of an external devnet deployment**.
For Issue #60, separately exercise maker collateral initialization and
funding (opcodes 11–14), linked book placement (30), active execution
(23–26), cancellation (31), and collateral withdrawal (14) against
one initialized funded market. Capture actual transaction signatures,
confirmed order sequence and page changes, maker-balance changes,
custody and token vault changes, and final withdrawals.

The browser in `devnet-demo/` currently builds maker placement/cancel
and active swaps; it does **not** perform the complete initialization,
collateral deposit, or withdrawal lifecycle. Until these workflows and
verified devnet receipts exist, the full market lifecycle remains unproven
on an external Solana cluster.

**Never put a signing keypair into CI or this repository.** Use funded
devnet-only operator wallets and check the program ID and RPC genesis.
