# Auditing and planning a two-page hybrid market

This guide is for **host-side Rust clients and keepers** using the merged linked-book maker lifecycle and `hybrid_program::client_audit`. It does not change the deployed program and it is not an RPC transport.

## Inputs and authentication

Fetch the market account, page-0 account, page-0 owner sidecar, page-1 account and page-1 owner sidecar as a **consistent snapshot**. Retain each account's actual public key, Solana owner and raw data; do not substitute locally derived addresses for keys returned by the RPC provider.

Pass all five as `AccountView` instances into `TwoPageSnapshot { market, head, head_owner, tail, tail_owner, side }`. Invoke `audit()`. The audit verifies:

- all five accounts are owned by the deployed hybrid program and not aliased;
- head and tail page addresses are the canonical market/index PDAs and the page-1 sidecar is canonical;
- legacy head sidecar address matches the market header commitment (full key for asks, 16-byte tag for bids);
- owner-sidecar lengths and links agree with each page;
- page ordering, cross-page price ordering and unique order sequences;
- the correct ask or bid order total agrees with the market header;
- links are either a fully detached page-1 pair or a fully linked head-to-tail pair.

A decoded account is not proof that the market authority can sign. Preserve the market-header authorization checks, and use a real market signer only where the on-chain ABI requires it.

## Safe decisions

Use the returned `AuditReport` before creating instructions from the `TwoPageBook` builder. `NextAction::AttachEmpty` means both pages are empty and detached, so a properly signed attach can be attempted. `DetachEmpty` means the tail is linked and empty; a properly signed detach can be attempted. `CancelOrFill` means tail orders remain and must be dealt with before detachment. `HeadOrders` means an empty detached tail must **not** be attached around live head orders.

**Important:** The report is advisory, not permission to transact. State may change between RPC read, transaction simulation and execution. Refresh the snapshot after swaps, cancellations, page compaction or any failed transaction. Explicitly check signatures, collateral balance and the resulting on-chain state; the program is the ultimate validator.

## Complete lifecycle

1. Inspect the two page PDAs and sidecars; use PR #50 typed builders for canonical addresses.
2. If page-1 accounts are closed, create them with the payer-funded `recreate_detached` instruction and then refresh the snapshot.
3. Audit; if the report says `AttachEmpty`, submit an authorized signed attach (opcode 29).
4. Deposit maker collateral using the existing custody instructions; then place maker orders on the appropriate page through opcode 30. Check market depth, ordering and free/locked maker balances.
5. Build linked exact-in/out SPL Token buy or sell swaps (opcodes 23–26) using account order and maker balance PDAs established by the typed client; audit and re-read before execution.
6. Cancel maker orders with the actual maker signer (opcode 31). Only after the tail is empty should a market signer detach it (opcode 32).
7. Audit detached page-1; if both the page and sidecar are empty and truly detached, optionally close (opcode 27). Rent is credited to the **market account**, not to the original funder.
8. Recreate if appropriate; repeat after a fresh audit.

## Boundaries and limitations

This auditor inspects the current two-page **order-book slice**, not a full chain of up to eight pages. The market can only perform authorized attach/detach if its key can actually sign; a market PDA is not automatically an externally signable authority. No keeper may infer funder refunds or mutate accounts from an offline report. Do not interpret the general Surfpool smoke test as evidence that these particular ABI sequences were exercised; the existing SBF tests provide transaction-level coverage.

The host-side audit is unsuitable as the sole security boundary: keep the on-chain PDA, sidecar, collateral and signature validation intact.
