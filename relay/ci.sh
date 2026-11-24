#!/usr/bin/env bash
# Stage relay's CI checks. `bash scripts/ci/stage.sh relay` runs this from the
# repository root (the contract is at the top of that script). Every step runs
# even after one fails; the script exits 1 if any did.
set -uo pipefail

# node:sqlite needs Node 22. A runner whose default is older has it in its tool cache.
major="$(node --version 2>/dev/null | sed -n 's/^v\([0-9][0-9]*\)\..*/\1/p')"
if [[ -z "$major" || "$major" -lt 22 ]]; then
    for dir in "${RUNNER_TOOL_CACHE:-/opt/hostedtoolcache}"/node/2[2-9].*/x64/bin; do
        [[ -x "$dir/node" ]] && PATH="$dir:$PATH"
    done
    export PATH
fi
echo "node $(node --version)"

failed=()
step() {
    local name="$1"
    shift
    echo "::group::relay: $name"
    if "$@"; then echo "relay: $name: ok"; else echo "relay: $name: FAILED"; failed+=("$name"); fi
    echo "::endgroup::"
}

step "wrangler (pinned)" bash scripts/dev-setup.sh relay
step "suite on the core" bash relay/test/run.sh core
step "suite on the standby" bash relay/test/run.sh standby
step "suite on the Worker under wrangler dev" bash relay/test/run.sh worker
step "no runtime dependencies" bash scripts/gate-lib.sh npm-no-deps relay/package.json
step "every owned clause is cited by a test" bash scripts/spec-coverage.sh relay
step "nothing unimplemented" bash scripts/gate-lib.sh no-unimplemented relay
step "no skipped test" bash scripts/gate-lib.sh no-skipped-tests relay
step "the shipped Worker bundles (dry run, nothing uploaded)" bash -c '. "${TASKDECK_TOOLS_HOME:-$HOME/.taskdeck-tools}/env.sh" && WRANGLER="$TASKDECK_WRANGLER" WRANGLER_SEND_METRICS=false bash relay/deploy.sh --dry-run'

if [[ "${#failed[@]}" -gt 0 ]]; then
    echo "relay: failed: ${failed[*]}"
    exit 1
fi
echo "relay: all checks passed"
