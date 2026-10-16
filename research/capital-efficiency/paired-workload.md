# Paired Hybrid–Manifest benchmark workload

This contract fixes the five scenarios, token-atom funding, maker distribution and step sequence **before running either protocol**. It prevents selection of favorable orders or omitted adverse cases after measurements exist.

```sh
node --test research/capital-efficiency/validate-workload.test.mjs
node research/capital-efficiency/validate-workload.mjs
```

## Execute, don't estimate

The run operator must separately map each semantic operation to valid protocol-specific instructions using pinned Hybrid and Manifest builds. There is no claim here of a common universal executable ABI. Initialize equivalent market states and use the same mint pair, quantities, maker identities, taker instructions and price boundaries where semantics permit; document unavoidable differences. Collect genuine finalized receipt signatures, account snapshots before/after, and compute-unit logs.

The fixed contract alone does not deploy SBF programs, fund accounts, verify receipts or execute the benchmark. A required next implementation is a protocol adapter for each runner backed by actual on-chain instructions.

## Fairness and failure reporting

Record unsuccessful attempts as well as landed fills, network/confirmation delays, skipped or unsupported operation types, failed collateral checks and account-lock conflicts. **Do not silently exclude failed scenarios.** Distinguish passive liquidity execution from active book execution, and Manifest global orders from local orders; compare equivalent behavior rather than names.

Report the exact Solana toolchain, Hybrid and Manifest commits, transaction environment, market state and verified mints. Then run the receipt collector and completed experiment validator on measured results.
