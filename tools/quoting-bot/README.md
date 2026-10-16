# Hybrid reference quoting strategy (P1)

This module demonstrates **bounded, deterministic, integer-only target quote planning**. It does not connect to an exchange, sign transactions, manage keys, calculate Q64 order prices, or submit actual orders. Its input price is in quote atoms per base atom and is for strategy testing only.

```bash
node --test tools/quoting-bot/plan.test.mjs
```

## Safe integration design

1. Authenticate a sufficiently fresh market snapshot and maker inventory.
2. Use explicit risk budgets (base/quote), spread, levels, and size.
3. Convert target prices into protocol-compatible Q64 fields using audited helpers.
4. Feed authenticated orders and targets to `reconcile_maker`, cancel then confirm, refresh, simulate placements and sign only after maker approval.
5. Journal signatures; never retry an uncertain submission blindly.

**Completion criteria:** live snapshot reader, correct Q64 conversion, integrated maker reconciliation, kill switch, pending-signature journal, simulation and confirmations, independent fill/inventory reconciliation, and long-duration public-devnet tests. Never run this initial reference against funded wallets unattended.
