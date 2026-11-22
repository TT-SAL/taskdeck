#!/usr/bin/env bash
# Tests for scripts/spec-coverage.sh, on a made-up tree (spec-fixture.sh).
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
source "$TESTS_DIR/spec-fixture.sh"

COV="$SCRIPTS_DIR/spec-coverage.sh"
cov() { bash "$COV" --root "$R" "$@"; }

R="$TMP/tree"
make_tree "$R"
# core owns CORE-AB-001, CORE-AB-002 (with engine) and CORE-ABC-001; CORE-AB-009 is withdrawn.
mkdir -p "$R/crates/taskdeck-core/tests" "$R/crates/taskdeck-core/src/apply" "$R/crates/taskdeck-sync/tests"
cat >"$R/crates/taskdeck-core/tests/apply.rs" <<'EOF'
// spec: CORE-AB-001
#[test]
fn refuses_a_bad_command() {}
EOF
cat >"$R/crates/taskdeck-core/src/apply/apply_tests.rs" <<'EOF'
// spec: CORE-ABC-001
#[test]
fn trims() {}
EOF
# A marker in a source file that is not a test file does not count.
cat >"$R/crates/taskdeck-core/src/lib.rs" <<'EOF'
// spec: CORE-AB-002
EOF
# Nor does one in a data file or a note under tests/: they are no source files.
mkdir -p "$R/crates/taskdeck-core/tests/data"
echo '{"note": "spec: CORE-AB-002"}' >"$R/crates/taskdeck-core/tests/data/notes.json"
echo 'spec: CORE-AB-002' >"$R/crates/taskdeck-core/tests/README.md"

expect "an uncovered clause fails" 1 cov core
output_has "it is named with file and line" "CORE-AB-002  docs/sync/spec/core.md:"
output_lacks "a covered clause is not listed" "CORE-AB-001  docs"
output_lacks "the *_tests.rs file counts" "CORE-ABC-001  docs"
output_has "the summary counts" "spec-coverage core: 2 of 3 owned clauses cited, 1 uncovered, 0 bad citation(s), in 2 test file(s)"
output_lacks "the withdrawn clause is not owned" "CORE-AB-009"

expect "--list fails too while one is uncovered" 1 cov --list core
output_has "--list names the citing file" "crates/taskdeck-core/tests/apply.rs"
output_has "--list marks the uncovered clause" "(no test cites it)"
output_lacks "a .json file under tests/ cites nothing" "notes.json"
output_lacks "a .md file under tests/ cites nothing" "README.md"

# The other owner must cite it in its own tests: core's citation does not count for engine.
cat >"$R/crates/taskdeck-core/tests/apply.rs" <<'EOF'
// spec: CORE-AB-001
// spec: CORE-AB-002
EOF
expect "every core clause cited: core passes" 0 cov core
expect "engine still lacks its own citations" 1 cov engine
output_has "engine's uncovered clauses" "CORE-AB-002  docs/sync/spec/core.md:"
output_has "and its own clause" "ENG-SEND-001  docs/sync/spec/engine.md:"

# A token that extends an id does not cite it, and is itself an error.
cat >"$R/crates/taskdeck-sync/tests/send.rs" <<'EOF'
// spec: ENG-SEND-001
// spec: CORE-AB-0021
EOF
expect "an id that extends another does not cite it" 1 cov engine
output_has "the prefix id stays uncovered" "CORE-AB-002  docs/sync/spec/core.md:"
output_has "the longer token is no clause" "crates/taskdeck-sync/tests/send.rs:2: cites CORE-AB-0021, which is no clause"

cat >"$R/crates/taskdeck-sync/tests/send.rs" <<'EOF'
// spec: ENG-SEND-001
// spec: CORE-AB-002
// spec: CORE-AB-009
EOF
expect "a citation of a withdrawn clause fails" 1 cov engine
output_has "it is named" "send.rs:3: cites CORE-AB-009, which is withdrawn"
sed -i '/CORE-AB-009/d' "$R/crates/taskdeck-sync/tests/send.rs"
expect "engine passes once both are cited" 0 cov engine

# Names of tests: JavaScript and Kotlin.
mkdir -p "$R/relay/test"
cat >"$R/relay/test/auth.test.js" <<'EOF'
// Mentions RELAY-AUTH-001 in a comment, which is no citation.
EOF
expect "an id in a plain comment is no citation" 1 cov relay
output_has "the clause stays uncovered" "RELAY-AUTH-001  docs/sync/spec/relay.md:"
cat >"$R/relay/test/auth.test.js" <<'EOF'
test('RELAY-AUTH-001 refuses a request without a token', () => {});
EOF
expect "a test name with the id cites it" 1 cov relay
output_lacks "RELAY-AUTH-001 is covered" "RELAY-AUTH-001  docs"
output_has "the other relay clause is still uncovered" "SEAL-FRAME-001  docs/sync/spec/seal.md:"
echo "it('SEAL-FRAME-001: a frame carries its length', () => {});" >>"$R/relay/test/auth.test.js"
expect "relay passes" 0 cov relay

mkdir -p "$R/android/app/src/test/kotlin"
cat >"$R/android/app/src/test/kotlin/BridgeTest.kt" <<'EOF'
class BridgeTest {
    @Test fun `BRIDGE-API-001 answers every verb`() {}
}
EOF
expect "a Kotlin test name cites" 0 cov android-shell

# desktop: tests/e2e is device's, not desktop's.
mkdir -p "$R/tests/e2e" "$R/src"
echo "// spec: DEV-STORE-001" >"$R/tests/e2e/store.rs"
expect "a citation under tests/e2e does not count for desktop" 1 cov desktop
output_has "desktop's clause stays uncovered" "DEV-STORE-001  docs/sync/spec/device.md:"
expect "but counts for device" 0 cov device
echo "// spec: DEV-STORE-001" >"$R/src/store.rs"
expect "desktop passes with a citation under src/" 0 cov desktop

# Directories that are never searched.
mkdir -p "$R/crates/taskdeck-seal/target/tests" "$R/crates/taskdeck-seal/tests"
echo "// spec: SEAL-FRAME-001" >"$R/crates/taskdeck-seal/target/tests/x.rs"
expect "files under target/ do not count" 1 cov seal

# A field or binding named spec is no marker; a malformed id after the marker is an error.
cat >"$R/crates/taskdeck-seal/tests/kat.rs" <<'EOF'
struct Vector { spec: String, cases: Vec<Case> }
fn f(v: Vector) { let spec: &str = &v.spec; let respec: u8 = 0; }
// spec: SEAL-FRAME-001
#[test]
fn frame_carries_length() {}
EOF
expect "a field named spec next to a marker passes" 0 cov seal
echo "// spec: SEAL-FRAME" >>"$R/crates/taskdeck-seal/tests/kat.rs"
expect "a malformed id after the marker fails" 1 cov seal
output_has "it is named" "kat.rs:6: cites SEAL-FRAME, which is no clause"
rm "$R/crates/taskdeck-seal/tests/kat.rs"
# A source directory named build is searched; Gradle's build output is not.
mkdir -p "$R/crates/taskdeck-seal/tests/build"
echo "// spec: SEAL-FRAME-001" >"$R/crates/taskdeck-seal/tests/build/mod.rs"
expect "a test module directory named build counts" 0 cov seal
rm -r "$R/crates/taskdeck-seal/tests/build"
mkdir -p "$R/android/app/build/src/test/kotlin"
echo "plugins {}" >"$R/android/app/build.gradle.kts"
echo "// spec: CORE-AB-0099" >"$R/android/app/build/src/test/kotlin/Gen.kt"
expect "Gradle's build directory is not searched" 0 cov android-shell
rm -r "$R/android/app/build" "$R/android/app/build.gradle.kts"

# release reads scripts/tests/**, but not foundation's tests of the spec
# tools, whose made-up test files cite made-up ids.
mkdir -p "$R/scripts/tests"
echo "echo '// spec: CORE-AB-0099'" >"$R/scripts/tests/test-spec-made-up.sh"
echo "echo '// spec: CORE-AB-0099'" >"$R/scripts/tests/spec-fixture.sh"
echo "# a release test" >"$R/scripts/tests/test-release-dry-run.sh"
expect "release does not read the spec tools' own tests" 0 cov release
output_has "only the release test is read" "spec-coverage release: the stage owns no clause; 0 bad citation(s) in 1 test file(s)"
expect "nor does rc" 0 cov rc
output_lacks "rc reports no made-up id" "CORE-AB-0099"
echo "# spec: CORE-AB-0099" >>"$R/scripts/tests/test-release-dry-run.sh"
expect "a release test that cites a made-up id fails" 1 cov release
output_has "it is named" "scripts/tests/test-release-dry-run.sh:2: cites CORE-AB-0099, which is no clause"

# Stages without clauses, unknown stages, the table.
expect "a stage that owns no clause passes" 0 cov foundation
output_has "it says so" "spec-coverage foundation: the stage owns no clause"
expect "an unknown stage is a usage error" 2 cov hub
expect "no stage is a usage error" 2 cov
expect "--paths prints the table" 0 cov --paths
output_has "core's test files" "core           crates/taskdeck-core/tests/** crates/taskdeck-core/fuzz/fuzz_targets/** crates/taskdeck-core/src/**/tests.rs crates/taskdeck-core/src/**/*_tests.rs"
output_has "desktop leaves out tests/e2e" "desktop        src/** tests/** !tests/e2e/**"
output_has "rc takes every path" "relay/test/**"
expect "--paths <stage> prints one row" 0 cov --paths relay
output_lacks "only that row" "core "

# The real repository: the script reads its specs (it fails for stages whose
# clauses have no tests yet, which is correct during foundation).
expect "the real tree: foundation owns no clause" 0 bash "$COV" foundation
# release and rc read scripts/tests/**: no file there may cite a made-up id
# (their exit code depends on their own tests, so it is not checked here).
for s in release rc; do
    OUT="$(bash "$COV" "$s" 2>&1)"
    output_lacks "the real tree: $s finds no bad citation in scripts/tests" "scripts/tests/"
done

finish
