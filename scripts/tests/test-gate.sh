#!/usr/bin/env bash
# Tests for scripts/gate.sh: made-up gate files in a made-up root, whose
# scripts/spec-coverage.sh and scripts/gate-lib.sh are stand-ins, and every real
# gate file read with --list.
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

GATE="$SCRIPTS_DIR/gate.sh"
R="$TMP/repo"
mkdir -p "$R/scripts/gates"
# Stand-ins for the common items: they log that they ran and pass.
for f in spec-coverage.sh gate-lib.sh; do
    printf '#!/usr/bin/env bash\necho "stand-in %s $*" >>"%s/ran.log"\n' "$f" "$R" >"$R/scripts/$f"
done
gate() { bash "$GATE" --root "$R" "$@"; }
COMMON_CORE='run | spec-coverage    | every clause cited (§3.3) | bash scripts/spec-coverage.sh core
run | no-unimplemented | none left (§3.3) | bash scripts/gate-lib.sh no-unimplemented core
run | no-skipped-tests | none skipped (§3.3) | bash scripts/gate-lib.sh no-skipped-tests core'
write_gate() { printf '%s\n' "$2" >"$R/scripts/gates/$1.gate"; }

# --- a gate with a passing, a failing and a CI item ---------------------------
write_gate core "# a comment, then a blank line

run | passes   | a command that passes (§1) | echo made-up-output && true
run | fails    | a command that fails (§2) | echo about to fail; exit 3
run | pipes    | a pipeline in the command (§3) | printf 'a\nb\n' | grep -q b
ci  | ci-thing | something only CI checks (§4) | the made-up job (ubuntu-latest)
$COMMON_CORE"
rm -f "$R/ran.log"
expect "a failing run item fails the gate" 1 gate core
output_has "the passing item passes" "PASS     passes"
output_has "the failing item fails" "FAIL     fails"
output_has "its exit status is shown" "==== FAIL fails (exit 3"
output_has "a command may hold a pipe" "PASS     pipes"
output_has "the CI item is CI-ONLY" "CI-ONLY  ci-thing"
output_has "the CI job is printed" "CI: the made-up job (ubuntu-latest)"
output_has "command output is shown" "made-up-output"
output_has "items after a failure still run" "PASS     no-skipped-tests"
output_has "the failure is summarised" "gate core: FAILED: fails"
expect "the common items ran from the root" 0 grep -q "stand-in gate-lib.sh no-skipped-tests core" "$R/ran.log"

expect "--only runs one passing item" 0 gate --only passes core
output_lacks "--only runs nothing else" "fails"
expect "--only on the failing item fails" 1 gate --only fails core
expect "--only on a CI item runs nothing and passes" 0 gate --only ci-thing core
output_has "it prints the CI item" "CI-ONLY  ci-thing"
expect "--only with an unknown id is an error" 2 gate --only nope core

write_gate core "run | marker | writes a file (§1) | touch made-up-marker
ci  | ci-thing | only CI (§4) | job
$COMMON_CORE"
expect "--list passes" 0 gate --list core
output_has "--list prints run items with their command" "$ touch made-up-marker"
output_has "--list prints CI items with their job" "CI: job"
output_has "--list counts" "gate core: 5 items (4 run here, 1 CI-only)"
expect "--list runs nothing" 0 test ! -e "$R/made-up-marker"
expect "the gate passes when every run item passes" 0 gate core
output_has "it says so" "gate core: passed: 4 run items; 1 CI-only items"
expect "the command ran in the root" 0 test -e "$R/made-up-marker"

# --- errors -------------------------------------------------------------------
expect "an unknown stage is an error" 2 gate hub
output_has "the stages are listed" "unknown stage 'hub'"
expect "a stage without a gate file is an error" 2 gate seal
output_has "the missing file is named" "no gate file"
write_gate core "# only comments"
expect "an empty gate file is an error" 2 gate --list core
output_has "it is named" "holds no item"
malformed() {  # malformed <case> <line> <message>
    write_gate core "$2
$COMMON_CORE"
    expect "malformed: $1" 2 gate --list core
    output_has "malformed: $1: reported" "$3"
}
malformed "unknown kind" "check | x | something (§1) | true" "the first field must be 'run' or 'ci'"
malformed "bad id" "run | Bad_Id | something (§1) | true" "the id 'Bad_Id' must be"
malformed "no § reference" "run | x | something | true" "with its § reference"
malformed "no command" "run | x | something (§1) |" "the fourth field (the command, or the CI job) is empty"
malformed "too few fields" "run | x | something (§1)" "the fourth field"
malformed "a repeated id" "run | spec-coverage | again (§1) | true" "the id 'spec-coverage' is used twice"
output_has "malformed lines are named by file and line" "core.gate:2: malformed line"

write_gate core "run | x | something (§1) | true"
expect "a gate without the common items is an error" 2 gate --list core
output_has "the rule is named" "the last items must be"
write_gate core "run | x | something (§1) | true
run | no-unimplemented | none left (§3.3) | bash scripts/gate-lib.sh no-unimplemented core
run | spec-coverage    | every clause cited (§3.3) | bash scripts/spec-coverage.sh core
run | no-skipped-tests | none skipped (§3.3) | bash scripts/gate-lib.sh no-skipped-tests core"
expect "the common items out of order are an error" 2 gate --list core
write_gate core "$COMMON_CORE
run | x | something (§1) | true"
expect "an item after the common items is an error" 2 gate --list core
write_gate core "${COMMON_CORE/no-skipped-tests core/no-skipped-tests seal}"
expect "a common item for another stage is an error" 2 gate --list core
write_gate core "${COMMON_CORE//run | no-/ci  | no-}"
expect "a common item that is a CI item is an error" 2 gate --list core
write_gate foundation "run | x | something (§1) | true
run | spec-coverage    | every clause cited (§3.3) | bash scripts/spec-coverage.sh foundation
run | no-skipped-tests | none skipped (§3.3) | bash scripts/gate-lib.sh no-skipped-tests foundation"
expect "foundation's gate has no no-unimplemented item" 0 gate --list foundation

# --- the real gates -----------------------------------------------------------
for s in foundation core seal relay engine android-shell device recovery desktop android release rc; do
    expect "the real $s gate reads" 0 bash "$GATE" --list "$s"
done
expect "the real foundation gate lists the Android-target check" 0 bash "$GATE" --list foundation
output_has "with its script" "$ bash scripts/check-android-targets.sh"
output_has "and the CI-only desktop matrix" "CI-ONLY  ci-green"

finish
