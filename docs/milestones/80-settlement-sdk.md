# Milestone 80: SDK and settlement automation

Status: implementation in progress; exact-head CI and independent security review required. No production activation.

## Implemented interfaces

- `@hybrid-dex/instructions/settlement`: bounded opcode 40–42 and 44–45 instruction descriptors, strict frozen epoch and reserve decoders, commitment/root verification and Merkle proof reconstruction.
- `@hybrid-dex/instructions/settlement-rpc`: canonical PDA discovery, context-consistent book and collateral reads, SPL token/mint checks, epoch indexing.
- `@hybrid-dex/instructions/settlement-plan`: integer per-tick quotes, cumulative allocations, fractional BID collateral rounding, bounded quote-to-build preparation.
- `@hybrid-dex/instructions/settlement-lifecycle`: ALT read/activation validation and advisory extension batching, serialized-message matching and exact-byte simulation, create-new durable claim intent and finalized bitmap reconciliation.

## Operating contract

The trusted host supplies a Solana adapter implementing canonical PDA derivation, public-key conversion, lookup table deserialization and actual signed-wire instruction decoding (including resolved v0 keys). No signer or submission service is shipped. Lookup-table create/extend/deactivate transactions remain operator-managed; extension plans are advisory. Persist the complete pre-execution roster and allocation amounts independently before execution; a frozen epoch stores a root, not the original maker roster. Claims must match both commitment and root. Unknown signatures never authorize retry, even beyond blockhash expiry. Retain claim intent files and manually reconcile interrupted locks.

The SDK is not a replacement for on-chain validation. A snapshot can become stale before execution. Use fresh authenticated RPC context, exact signed-byte simulation, a trusted deployment domain and independent review. No automatic production submission is authorized.

## Required exit gates

- [ ] SDK positive and negative tests, including full RPC and recovery lifecycle fixtures.
- [ ] Verify concrete Solana adapter, actual v0/ALT signed transaction integration and quote/build/simulation parity.
- [ ] Exact-head CI green including SDK, SBF, CU, ELF and provenance; no weakened limits.
- [ ] Independent security review and operator authorization before any production activation.
