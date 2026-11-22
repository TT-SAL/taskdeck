#!/usr/bin/env bash
# Runs every scripts/tests/test-*.sh and reports a total. Plain bash, no
# framework: each test file prints "ok" or "FAIL" lines and exits non-zero if
# any of its cases failed.
#
#   bash scripts/tests/run.sh
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
status=0
for t in "$here"/test-*.sh; do
    echo "== $(basename "$t")"
    if ! bash "$t"; then
        status=1
    fi
done
if [[ $status -eq 0 ]]; then
    echo "scripts/tests: all passed"
else
    echo "scripts/tests: FAILED"
fi
exit $status
