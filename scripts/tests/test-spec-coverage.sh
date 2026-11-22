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

# Ruling 9: an id that stands as a whole token anywhere in a test file cites
# its clause, whether or not a live test stands next to it.
cat >"$R/crates/taskdeck-core/tests/apply.rs" <<'EOF2'
// spec: CORE-AB-001
fn helper() {}

// #[test]
// spec: CORE-AB-002
// fn old() {}
/* CORE-ABC-001 in a block comment */
EOF2
rm "$R/crates/taskdeck-core/src/apply/apply_tests.rs"
expect "ids beside a helper, a commented-out test or in a comment cite" 0 cov core
output_has "all three are cited" "spec-coverage core: 3 of 3 owned clauses cited, 0 uncovered, 0 bad citation(s), in 1 test file(s)"
echo "// see CORE-AB-0099" >>"$R/crates/taskdeck-core/tests/apply.rs"
expect "an id-shaped token that is no clause fails, marker or not" 1 cov core
output_has "it is named" "apply.rs:8: cites CORE-AB-0099, which is no clause"
printf '// spec: CORE-AB-001\n// spec: CORE-AB-002\n' >"$R/crates/taskdeck-core/tests/apply.rs"

# The other owner must cite it in its own tests: core's citation does not count for engine.
expect "engine still lacks its own citations" 1 cov engine
output_has "engine's uncovered clauses" "CORE-AB-002  docs/sync/spec/core.md:"
output_has "and its own clause" "ENG-SEND-001  docs/sync/spec/engine.md:"

# A token that extends an id does not cite it, and is itself an error.
cat >"$R/crates/taskdeck-sync/tests/send.rs" <<'EOF'
// spec: ENG-SEND-001
// spec: CORE-AB-0021
#[test]
fn sends() {}
EOF
expect "an id that extends another does not cite it" 1 cov engine
output_has "the prefix id stays uncovered" "CORE-AB-002  docs/sync/spec/core.md:"
output_has "the longer token is no clause" "crates/taskdeck-sync/tests/send.rs:2: cites CORE-AB-0021, which is no clause"

cat >"$R/crates/taskdeck-sync/tests/send.rs" <<'EOF'
// spec: ENG-SEND-001
// spec: CORE-AB-002
// spec: CORE-AB-009
#[test]
fn sends() {}
EOF
expect "a citation of a withdrawn clause fails" 1 cov engine
output_has "it is named" "send.rs:3: cites CORE-AB-009, which is withdrawn"
sed -i '/CORE-AB-009/d' "$R/crates/taskdeck-sync/tests/send.rs"
expect "engine passes once both are cited" 0 cov engine

# Names of tests: JavaScript and Kotlin.
mkdir -p "$R/relay/test"
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
# Markers above a node:test and a JUnit test cite them.
mkdir -p "$R/android/page/test"
printf '// spec: BRIDGE-API-001\nit("answers", () => {});\n' >"$R/android/page/test/verbs.test.js"
printf 'class T {\n    // spec: BRIDGE-API-001\n    @Test\n    fun answers() {}\n}\n' >"$R/android/app/src/test/kotlin/Marker.kt"
expect "markers above node:test and JUnit tests cite them" 0 bash "$SCRIPTS_DIR/spec-coverage.sh" --root "$R" --list android-shell
output_has "all three files and the Kotlin name" "android/app/src/test/kotlin/BridgeTest.kt android/app/src/test/kotlin/Marker.kt android/page/test/verbs.test.js"
rm -r "$R/android/page" "$R/android/app/src/test/kotlin/Marker.kt"

# desktop: tests/e2e is device's, not desktop's.
mkdir -p "$R/tests/e2e" "$R/src"
printf '// spec: DEV-STORE-001\n#[test]\nfn stores() {}\n' >"$R/tests/e2e/store.rs"
expect "a citation under tests/e2e does not count for desktop" 1 cov desktop
output_has "desktop's clause stays uncovered" "DEV-STORE-001  docs/sync/spec/device.md:"
expect "but counts for device" 0 cov device
printf '# spec: DEV-STORE-001\nexpect "stores" 0 true\n' >"$R/tests/store.sh"
expect "a marker in a shell test cites it" 0 cov desktop
rm "$R/tests/store.sh"
cp "$R/tests/e2e/store.rs" "$R/src/store.rs"
expect "desktop passes with a citation under src/" 0 cov desktop

# Directories that are never searched.
mkdir -p "$R/crates/taskdeck-seal/target/tests" "$R/crates/taskdeck-seal/tests"
printf '// spec: SEAL-FRAME-001\n#[test]\nfn frames() {}\n' >"$R/crates/taskdeck-seal/target/tests/x.rs"
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
cp "$R/crates/taskdeck-seal/target/tests/x.rs" "$R/crates/taskdeck-seal/tests/build/mod.rs"
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
# rc owns no clause but runs every stage's check, each on that stage's files.
expect "rc fails while a clause of another stage is cited nowhere" 1 cov rc
output_has "core's uncovered clause" "CORE-ABC-001  docs/sync/spec/core.md:"
output_has "seal's uncovered clause" "SEAL-FRAME-001  docs/sync/spec/seal.md:"
output_has "nor does rc read the spec tools' own tests" "spec-coverage rc: the stage owns no clause; 0 bad citation(s) in 7 test file(s)"
output_lacks "rc reports no made-up id" "CORE-AB-0099"
printf "import { test } from 'node:test';\n// spec: CORE-ABC-001\n// spec: SEAL-FRAME-001\ntest('x', () => {});\n" >"$R/relay/test/rc.test.js"
expect "a citation in another stage's tests does not cover the clause for its owner" 1 cov rc
output_has "core's clause stays uncovered" "CORE-ABC-001  docs/sync/spec/core.md:"
output_has "seal's clause stays uncovered" "SEAL-FRAME-001  docs/sync/spec/seal.md:"
mkdir -p "$R/crates/taskdeck-seal/tests"
printf '// spec: CORE-ABC-001\n#[test]\nfn x() {}\n' >"$R/crates/taskdeck-core/tests/rc.rs"
printf '// spec: SEAL-FRAME-001\n#[test]\nfn x() {}\n' >"$R/crates/taskdeck-seal/tests/rc.rs"
expect "rc passes once every stage's clauses are cited by that stage's tests" 0 cov rc
rm "$R/relay/test/rc.test.js" "$R/crates/taskdeck-core/tests/rc.rs" "$R/crates/taskdeck-seal/tests/rc.rs"
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
