# Collect actual finalized Solana receipt metrics

The experiment template and schema tests are not transaction measurements. This collector queries **real finalized** Solana transaction signatures using `getTransaction`, rejecting missing, errored or unmetered receipts. It reports compute units, account-meta count, transaction slot and fees from the returned receipts.

```bash
node --test research/capital-efficiency/collect-receipts.test.mjs
node research/capital-efficiency/collect-receipts.mjs receipts-input.json receipts-output.json
```

Input example (replace the signature with a **real finalized** transaction):

```json
{
  "rpc": "https://api.devnet.solana.com",
  "signatures": ["PASTE_VALID_FINALIZED_SIGNATURE"]
}
```

Use identical workloads and carefully recorded Hybrid / Manifest commit identities. The collector deliberately **does not** infer signing latency, rent, maker inventory utilization, execution-price consistency, or token conservation from generic transaction receipts. Collect those independently from reproducible before/after account snapshots and event timing, then populate the paired `experiment-template.json` and validate the completed report.

**Trust restriction:** Only run this tool against trusted Solana RPC endpoints. The output is evidence about RPC-returned transactions, not cryptographic proof that a particular protocol instruction or market was executed. The checker currently accepts legacy and v0 transaction account key arrays.
