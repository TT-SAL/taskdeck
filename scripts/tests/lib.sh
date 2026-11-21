# Shared helpers for scripts/tests/test-*.sh. Source it; call `finish` last.
TESTS_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPTS_DIR="$(dirname "$TESTS_DIR")"
REPO_DIR="$(dirname "$SCRIPTS_DIR")"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
FAILS=0
PASSES=0

# expect <case name> <expected exit code> <command...>
# Runs the command, keeps its combined output in $OUT, and checks the exit code.
expect() {
    local name="$1" want="$2"
    shift 2
    OUT="$("$@" 2>&1)"
    local got=$?
    if [[ "$got" -eq "$want" ]]; then
        PASSES=$((PASSES + 1))
        echo "ok    $name"
    else
        FAILS=$((FAILS + 1))
        echo "FAIL  $name: exit $got, wanted $want"
        sed 's/^/        /' <<<"$OUT"
    fi
}

# output_has <case name> <fixed string>: the last command's output contains it.
output_has() {
    if grep -qF -- "$2" <<<"$OUT"; then
        PASSES=$((PASSES + 1))
        echo "ok    $1"
    else
        FAILS=$((FAILS + 1))
        echo "FAIL  $1: output lacks: $2"
        sed 's/^/        /' <<<"$OUT"
    fi
}

# output_lacks <case name> <fixed string>
output_lacks() {
    if grep -qF -- "$2" <<<"$OUT"; then
        FAILS=$((FAILS + 1))
        echo "FAIL  $1: output has: $2"
        sed 's/^/        /' <<<"$OUT"
    else
        PASSES=$((PASSES + 1))
        echo "ok    $1"
    fi
}

finish() {
    echo "   $PASSES passed, $FAILS failed"
    [[ "$FAILS" -eq 0 ]]
}
