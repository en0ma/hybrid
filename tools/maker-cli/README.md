# Hybrid maker CLI — P1 starting point

Initial commands are **read-only** wrappers around the existing reference manifest and readiness scripts. This avoids inventing a trading ABI or unattended key custody.

```bash
cd devnet-demo && npm install
cd ../tools/maker-cli
npm test
node cli.mjs manifest <MARKET> [MAKER]
node cli.mjs readiness <MARKET> <ASK_OWNER> <BID_OWNER> <BASE_VAULT> <QUOTE_VAULT>
```

The readiness command requires a correctly provisioned market and valid RPC configuration. It does not sign transactions.

## P1 completion criteria

Introduce a typed, versioned SDK with correct Q64 conversion, authenticated decoded book depth, maker free/locked balances, place/cancel/replace support, safe batch sequencing, simulation/confirmation handling, transaction retry journal, and executable end-to-end tests with the real program. Add examples and devnet evidence. Do not call this production-ready before those tests pass.
