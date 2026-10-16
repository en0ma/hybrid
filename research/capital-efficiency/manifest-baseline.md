# Reference baseline and comparison protocol

**Hybrid baseline:** `crates/settlement/src/lib.rs`, `program/src/client_maker.rs`, `devnet-demo/README.md`. These are observed repository facts, not roadmap promises.

**Manifest baseline (public documentation):** https://github.com/Bonasa-Tech/manifest/blob/main/README.md

Manifest describes **global orders**, which do not lock token inventory on the local market order. Assets are reused across markets and moved just in time at fill. Its **reverse orders** can recreate opposite-side liquidity using realized fill proceeds. The core/wrapper boundary deliberately keeps application conveniences out of the matching core.

## Reproducible cross-protocol protocol

1. Pin a specific audited Manifest Git SHA (required in final results; `null` in template is not acceptable).
2. Pin Hybrid Git SHA, Solana toolchain, runner hardware, RPC mode, mint decimals and maker distribution.
3. Use exact identical wallet funding, mint pairs, order counts, order sizes, slippage bounds and fill paths.
4. Execute **real instruction transactions** in the same controlled environment and collect logs, account metas, compute units, rent bytes and final SPL balances. Model-only metrics are not suitable for these fields.
5. Separate maker quote-update throughput from taker settlement throughput; compare with and without Manifest global orders, and with/without any future Hybrid global-reserve design.
6. Report liquidity failure rate under concurrent fills, and actual landed trades, not just nominal available depth.
7. Include signed public-devnet receipts as a later validation phase; no claims about mainnet performance.

See [Manifest's own description of global/reverse orders](https://github.com/Bonasa-Tech/manifest/blob/main/README.md). Do not equate this directory's offline model with actual Manifest measured performance.
