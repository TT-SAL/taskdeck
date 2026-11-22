# relay/

The relay: `relay-core.js` behind a storage interface, the Cloudflare Worker and Durable Object (`worker.js`), the Node 22 standby (`standby.js`, `node:sqlite`) and the `node:test` conformance suite. No runtime dependencies (SYNC_PLAN.md §3.4).

Nothing lives here yet: the directory is created by stage `foundation`.

- Owner: stage `relay` (SYNC_PLAN.md §4).
- Spec: `docs/sync/spec/relay.md`.
- CI: `bash scripts/ci/stage.sh relay` runs `bash relay/ci.sh` from the
  repository root if that file exists, with `CI_STAGE`, `CI_OS` and `CI_WIDE`
  set (the contract is at the top of `scripts/ci/stage.sh`). The stage's own
  checks go in that `ci.sh`, not in `ci.yml`.
