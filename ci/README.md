# CI contract

Hybrid treats Solana compute units (CU), account count, state bytes, transaction bytes and quote latency as protocol-level budgets.

## Fork lane

`ci/fork.sh` runs Surfpool 1.5.0 in CI. Surfpool forks mainnet by default and lazily fetches accounts. If the repository secret `SOLANA_RPC_URL` exists it is used as the upstream source; otherwise Surfpool's default datasource is used.

Add executable `ci/fork-test.sh` for end-to-end protocol scenarios. The bootstrap smoke test verifies that the fork can load the deployed Manifest core program.

## Manifest baseline

Manifest is pinned to:

`d04b7aa90b098ba64ebcdf398e9a24d89a9114d8`

The public Manifest repository is compiled and tested on each gate. Its production replay benchmark uses a private repository, so Hybrid does not pretend that metric is reproducible. Instead, `ci/compare-cu.sh` is the extension point for identical Hybrid-vs-Manifest transactions under our own public fixture set.

## CU budget contract

Once Hybrid has an executable program, add executable `ci/cu-check.sh` and `ci/compare-cu.sh`. They must fail on regressions rather than merely report numbers. Keep thresholds versioned in-repo.

## Exact-head merge rule

The PR workflow checks out `pull_request.head.sha`, rejects unresolved review threads, and re-fetches the PR head at the final gate. If a commit lands while CI is running, the validated run is rejected and the new head must pass again.

The post-merge workflow reruns core, fork, and Manifest comparison suites on the fresh commit on `main`.
