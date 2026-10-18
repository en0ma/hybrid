# Collect actual finalized Solana receipt metrics

The experiment template and schema tests are not transaction measurements. This collector queries **real finalized** Solana transaction signatures using `getTransaction`, rejecting missing, errored or unmetered receipts. It reports compute units, account-meta count, transaction slot and fees from the returned receipts.

```bash
node --test research/capital-efficiency/collect-receipts.test.mjs
node research/capital-efficiency/collect-receipts.mjs receipts-input.json receipts-output.json
```

Input example (replace the signature with a **real finalized** transaction):

```json
{
  "rpc": "https://api.devnet.solana.com",\n  "expectedProgramId": "PASTE_BASE58_DEPLOYED_PROGRAM_ID",
  "expectedMarket": "PASTE_BASE58_TARGET_MARKET",
  "signatures": ["PASTE_VALID_FINALIZED_SIGNATURE"]
}
```

Use identical workloads and carefully recorded Hybrid / Manifest commit identities. The collector deliberately **does not** infer signing latency, rent, maker inventory utilization, execution-price consistency, or token conservation from generic transaction receipts. Collect those independently from reproducible before/after account snapshots and event timing, then populate the paired `experiment-template.json` and validate the completed report.

**Trust restriction:** Only run this tool against trusted Solana RPC endpoints. The output is evidence about RPC-returned transactions, not cryptographic proof that a particular protocol instruction or market was executed. The checker currently accepts legacy and v0 transaction account key arrays.

## Program identity check

The receipt collector now requires `expectedProgramId` and verifies that a finalized transaction's **top-level instruction list** directly invokes that program. This rejects unrelated successful transfers masquerading as Hybrid/Manifest executions. For versioned transactions, loaded writable/read-only address tables contribute to instruction index resolution. This is a minimum authentication check: transaction semantics, market identity, actual matching behavior, and token conservation still require independent program-specific decoders and before/after-state audits. CPI-only invocations are not accepted by this direct-instruction gate.

## Optional market binding

Provide `expectedMarket` to require that at least one **top-level instruction targeting the expected program** also directly references that market as an instruction account. Mere inclusion of the market in unrelated transaction instructions does not pass this check. This is still not a validation of program-specific account positions, trading opcode, custody mints, pre/post token conservation, or successful order matching; perform those checks in a separate protocol-specific auditor.

## Requested-signature binding

A valid receipt must also return the **same first transaction signature** that was requested. This prevents a dishonest or misconfigured RPC response from associating a different transaction's compute/fee data with the supplied signature. It is a consistency check against RPC data—not independent consensus verification, and it does not establish fee attribution or correct market-level settlement.
