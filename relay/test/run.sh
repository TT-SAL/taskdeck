#!/usr/bin/env bash
# The conformance suite of docs/sync/spec/relay.md §18 against one target:
#   bash relay/test/run.sh core      the core in process
#   bash relay/test/run.sh standby   the standby over HTTP on loopback
#   bash relay/test/run.sh worker    the Worker under `wrangler dev` (the pinned
#                                    wrangler of scripts/dev-setup.sh relay)
# Extra arguments go to `node --test` (for example --test-name-pattern=...).
set -uo pipefail
target="${1:-}"
case "$target" in core|standby|worker) shift ;; *) echo "usage: bash relay/test/run.sh core|standby|worker" >&2; exit 2 ;; esac
cd "$(dirname "${BASH_SOURCE[0]}")/.." || exit 2
export TD_TARGET="$target"

if [[ "$target" != worker ]]; then
    exec node --test "$@" 'test/*.test.mjs'
fi

if [[ -z "${TASKDECK_WRANGLER:-}" ]]; then
    tools="${TASKDECK_TOOLS_HOME:-$HOME/.taskdeck-tools}"
    # shellcheck disable=SC1091
    [[ -f "$tools/env.sh" ]] && . "$tools/env.sh"
fi
if [[ -z "${TASKDECK_WRANGLER:-}" || ! -x "${TASKDECK_WRANGLER:-}" ]]; then
    echo "run.sh: wrangler is not installed: bash scripts/dev-setup.sh relay" >&2
    exit 2
fi

work="$(mktemp -d)"
port="${TD_WORKER_PORT:-$(node -e 'const s=require("net").createServer().listen(0,"127.0.0.1",()=>{console.log(s.address().port);s.close()})')}"
# No account, no network: a local runtime only (SYNC_PLAN.md §3.4: agents never deploy).
WRANGLER_SEND_METRICS=false CI=true HOME="$work/home" XDG_CONFIG_HOME="$work/config" \
    "$TASKDECK_WRANGLER" dev --config test/worker/wrangler.toml --ip 127.0.0.1 --port "$port" \
    --inspector-port 0 --persist-to "$work/state" --show-interactive-dev-session=false \
    >"$work/wrangler.log" 2>&1 &
wrangler_pid=$!
stop() {
    kill "$wrangler_pid" 2>/dev/null
    wait "$wrangler_pid" 2>/dev/null
    rm -rf "$work"
}
trap stop EXIT

ready=0
for _ in $(seq 1 120); do
    if node -e 'fetch(process.argv[1]).then((r)=>process.exit(r.ok?0:1),()=>process.exit(1))' "http://127.0.0.1:$port/__test/ping" 2>/dev/null; then ready=1; break; fi
    kill -0 "$wrangler_pid" 2>/dev/null || break
    sleep 0.5
done
if [[ "$ready" != 1 ]]; then
    echo "run.sh: wrangler dev did not start:" >&2
    tail -n 40 "$work/wrangler.log" >&2
    exit 1
fi

TD_WORKER_URL="http://127.0.0.1:$port" node --test --test-concurrency=1 "$@" 'test/*.test.mjs'
status=$?
if [[ "$status" != 0 ]]; then
    echo "run.sh: the last lines of wrangler's log:" >&2
    tail -n 30 "$work/wrangler.log" >&2
fi
exit "$status"
