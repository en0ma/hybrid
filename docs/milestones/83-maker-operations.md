# M83 isolated maker operations and incident playbook

Status: experimental, loopback sandbox only; independent security and operations review pending. No unattended or public-cluster submission.

## Before operating
- Use a fresh offline Surfpool instance, disposable funded test signer, trusted RPC, pinned genesis and program, private mode-0700 journal directory. A loopback proxy is not proof of isolation. Verify actual offline runtime and signer custody.
- Read observe/plan output and reconcile all maker free/locked inventory, custody and vault liabilities. Set explicit positive caps and a reconciled quote-atom equity baseline; never infer a baseline after losses. Confirm no prior journals, locks, pending files, or live signature before initial creation.
- Default is read-only; enable sandbox mutations per process only. The environment switch is not a live interprocess emergency signal. Never run concurrent operators or store keys in the config.

## Normal operation
Each CLI invocation performs one stage. Reserve, externally sign the exact message, prepare, submit once, inspect, finalize and reconcile. Use read-only status with the execution switch off to check the original signature on the pinned genesis; neither status nor inspect grants release or retry. Do not issue another reservation until the journal is retired and a new authenticated plan is available. A signed wire is sensitive until blockhash expiry; do not paste it into public logs. Keep all archive evidence and capture the signature, finalized slot, phase, maker inventory and custody/vault proof. Rate renewal is explicit, idle-only after a full finalized-slot quiet interval; it never resets the loss baseline or retry count.

## Incident response
| Signal | Required action |
|---|---|
| Kill switch, loss, exposure, stale snapshot, or unexpected rate cap | Stop new commands, set execution switch off, retain all records. Review inventory and risk limits independently. No blind emergency cancellation or reset. |
| HTTP 429, timeout, disconnect, wrong signature, unknown history | Treat submitted intent as unresolved. Do not resubmit or replace. Check pinned genesis, history and exact finalized receipt. Null history is not proof of absence. |
| Finalized transaction error | Stop. Preserve wire, receipt and journal. After independent review, explicitly use sandbox-recover-failed only for the matching finalized failed receipt and unchanged authenticated maker/custody/vault state. The fee is archived and counters retained; do not retry the old wire. Otherwise retain the blocked intent. |
| Unexpected post-state, concurrent fill, pending lazy claim | Keep session blocked. Compare exact on-chain receipts, claim bitmaps, book, inventory and custody/vault changes with archived before state. Escalate to an independent operator; no guessed deltas. |
| Crash, stale lock, interrupted pending file, missing journal, archive mismatch | Preserve directory read-only backup and checksum evidence. Do not remove lock, edit journal or reinitialize to bypass unresolved intent. Independent reviewer must reconstruct exact signature, receipt and state before any manual recovery. |
| Unexpected cluster or endpoint | Stop immediately, isolate signer, verify RPC and genesis out of band. Treat any possible broadcast as unresolved. |

## Evidence and release gates
Record exact commit, CI run, RPC class/genesis, signer public key only, submitted signatures, finalized receipts and CU, all inventory/vault deltas, risk policy, archived counter renewals and incident evidence. Existing funded offline Surfpool coverage is not public devnet proof. Concurrent in-flight changes still require independent manual reconciliation; crash-interrupted records have no automated repair. Multi-page mutation unsupported. Independent security and operations review, exact-head CI, and explicit operator release decision required. No production activation.
