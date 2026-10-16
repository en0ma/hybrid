# Execution adapter interface — P2

The paired-workload runner is now able to invoke two explicit protocol adapters with the same ordered scenarios. This is **not** a completed Hybrid/Manifest adapter implementation. It deliberately refuses missing adapters, non-executed operations or unpinned commits.

Each adapter module exports:

```js
export const name = "hybrid"; // or "manifest"
export async function identity() {
  return { commit: "<exact 40-char audited commit>", programId: "<actual deployed Solana program ID>" };
}
export async function execute({ scenario, step, contract }) {
  // Build/sign/send/confirm the protocol-specific instruction against the declared test network.
  // Audit the resulting state. Return *actual* confirmed transaction evidence.
  return { status: "executed", signature: "<real base58 Solana signature>", slot: 123 };
}
```

The returned example is schematic, **not executable trading code**. Both adapters must map the operation to their actual on-chain ABI; where one protocol cannot support a semantic step, the run must **fail visibly**, not silently emit a fabricated signature.

```bash
node --test research/capital-efficiency/run-workload.test.mjs
node research/capital-efficiency/run-workload.mjs research/capital-efficiency/paired-workload.json ./hybrid-adapter.mjs ./manifest-adapter.mjs results.json
```

## Evidence trust boundary

The runner records adapter-returned signatures as **unverified claims**. Use `collect-receipts.mjs` to independently fetch finalized on-chain receipts bound to program and market. Independently audit token balances, state transitions, order-book effects and rent. Only then fill a paired benchmark report. The runner must not be used to imply a confirmed result based solely on test adapter outputs.

## Implementation follow-ups

- Pin and build both protocol implementations, with operator-controlled deployment authority.
- Build actual Hybrid/Manifest instruction adapters and run under equal conditions.
- Collect confirmations and independent account-state audit evidence for each stage.
- Produce a reproducible pair of run manifests with any unsupported semantics explicitly recorded.

## Separate transaction and audit steps

The runner now records `{kind:"transaction",signature,slot}` for actual submitted instructions and `{kind:"audit",proof,slot}` for state observations, confirmations, refreshes, simulated timeouts and checks. An audit must return `{status:"audited",verified:true,proof:"...",slot:...}`; a recycled transaction signature **cannot** substitute for independent proof. These are adapter claims and still require independent verification before publication. Operational steps may need richer outcome typing as protocol adapters evolve.
