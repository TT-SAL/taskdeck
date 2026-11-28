#!/usr/bin/env bash
# Stage device's end-to-end checks. `bash scripts/ci/stage.sh device` runs this
# from the repository root with CI_STAGE, CI_OS and CI_WIDE set (the contract is
# at the top of that script). The suite runs on Linux only for now (decision
# L3): the standby needs Node 22, and the devices run in the runner's container.
set -uo pipefail

if [[ "${CI_OS:-linux}" != "linux" ]]; then
    echo "device e2e: the end-to-end suite runs on Linux only for now (decision L3); nothing to do on ${CI_OS}"
    exit 0
fi

# node:sqlite needs Node 22. A runner whose default is older has it in its tool cache
# (the same lookup as relay/ci.sh; the harness repeats it, and honours TD_NODE).
major="$(node --version 2>/dev/null | sed -n 's/^v\([0-9][0-9]*\)\..*/\1/p')"
if [[ -z "$major" || "$major" -lt 22 ]]; then
    for dir in "${RUNNER_TOOL_CACHE:-/opt/hostedtoolcache}"/node/2[2-9].*/x64/bin; do
        [[ -x "$dir/node" ]] && PATH="$dir:$PATH"
    done
    export PATH
fi
echo "node $(node --version 2>&1)"

echo "::group::device e2e: the suite"
cargo test --locked -p taskdeck-device --features test-trust --test e2e
status=$?
echo "::endgroup::"
if [[ "$status" -ne 0 ]]; then
    echo "device e2e: FAILED"
    exit 1
fi
echo "device e2e: all checks passed"
