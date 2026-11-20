# PR #74: authenticated 9–16 maker BID epoch foundation

**Status: draft — not yet ready for merge.**

This change begins the unfinished roster component of the PR #72 follow-up roadmap.
PR #73 merged authenticated single-tick, single-page BID execution with at most
eight distinct maker seats. This draft extends that **same single-tick BID ABI**
to a bounded 16-seat roster. It does **not** claim multipage or multilevel
execution.

## Changes and compatibility

- Opcode 42 still accepts the same 17-byte payload (base fill and minimum quote
  out), the same first 17 fixed accounts, and one **writable, ordered,
  canonical maker-balance PDA per maker**. The max full account count is now
  33 (17 fixed + 16 makers).
- The program continues to derive the roster from the authenticated BID page
  and its owner sidecar. The caller may not provide makers, weights, Merkle
  leaves or the epoch root as untrusted instruction data.
- Each maker must be unique. All orders must still be on page 0 at the same
  canonical tick. The page must have no linked neighbors.
- The commitment domain, side=1 encoding, 44-byte maker rows, claim leaf domain
  and opcode 40 verifier are unchanged. The fixed commitment preimage buffer
  grows to 928 bytes (the 16-seat preimage itself is 905 bytes).
- The in-place Merkle workspace grows to 16 leaves; odd levels still duplicate
  their final leaf. Frozen-epoch 912-byte storage and its bitmap are unchanged.
- Existing opcode 41 ASK execution remains capped at eight seats and is
  intentionally unchanged by this draft.
- SDK now exposes `hashPubkeyFrozenBidEpochV1` with the same on-chain domain
  for one to sixteen BID makers. The existing ASK helper retains its cap.

## Verified with checked-in vectors

- Nine-maker BID digest:
  `81eaae5151672a4b1fb6a0cb1f41dcfc5173003cc6759024fcf91686550733c6`
- Nine-maker BID Merkle root (odd terminal leaf duplicated):
  `06e7d26453d0aaa9b13b40f2afb5c2136be470326d0e1a2a44e48f443d62a025`
- Sixteen-maker BID digest:
  `6c753eed7dd4ab15711ec2d48b77c4d3315b7d2839186e9a98a50738cbe14959`
- Sixteen-maker BID Merkle root:
  `7d6f22035f78d5898824a267180132f69fa6bb7c1539b480f88bf41d0551c9e2`

These deterministic Rust/SDK checks are **not evidence** that a 16-seat
settlement fits runtime compute, loaded account, stack, heap or SBF limits.

## Required before marking this PR ready for review

- [ ] Add funded SBF execution and **all** maker claims at nine and sixteen
      seats; verify residual locked quote, fee conservation, vault floors and
      exact claims against every Merkle proof.
- [ ] Measure execution compute at nine and sixteen seats without bypassing
      existing protocol CU budgets. Establish a defensible upper bound or
      lower the max if required.
- [ ] Confirm stack/frame diagnostics and ELF budget on the exact head.
- [ ] Add odd-seat proof verification and rollback for duplicate, missing,
      swapped or readonly maker-balance PDAs and incorrect proof siblings.
- [ ] Ensure all Rust, SDK, SBF, fuzz, fork and Manifest CI lanes pass.
- [ ] Obtain independent security review for the expanded account ABI.

## Out of scope: next execution ABI

Linked multipage BID execution and multilevel price crossing require an
explicit ordered page/sidecar account ABI and per-tick epoch commitments.
They must not be implemented by pretending all price ticks are a single
uniform-price epoch. Retain the current single-tick, one-page rejection
policy until that versioned design and funded tests are complete.
