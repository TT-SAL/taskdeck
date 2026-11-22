#!/usr/bin/env bash
# Tests for scripts/gate-lib.sh: the two scans and the data checks, on
# made-up trees. The patterns the scans look for are assembled at run time.
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

LIB="$SCRIPTS_DIR/gate-lib.sh"
lib() { bash "$LIB" --root "$R" "$@"; }
UNIMPL="unimplemented""!()"
TODO="todo""!(\"later\")"
UNREACH_TODO="unreachable""!(\"TODO: later\")"
IGNORE="#[ig""nore]"

# --- no-unimplemented ---------------------------------------------------------
R="$TMP/u"
mkdir -p "$R/crates/taskdeck-core/src" "$R/crates/taskdeck-core/tests" "$R/crates/taskdeck-core/target/debug"
echo "pub fn f() {}" >"$R/crates/taskdeck-core/src/lib.rs"
expect "a clean crate passes" 0 lib no-unimplemented core
output_has "it says where it looked" "no-unimplemented core: ok (none in crates/taskdeck-core)"
printf 'pub fn g() {\n    %s\n}\n' "$UNIMPL" >"$R/crates/taskdeck-core/src/g.rs"
expect "an unimplemented body fails" 1 lib no-unimplemented core
output_has "file and line are named" "crates/taskdeck-core/src/g.rs:2:"
rm "$R/crates/taskdeck-core/src/g.rs"
printf '#[test]\nfn t() { %s }\n' "$TODO" >"$R/crates/taskdeck-core/tests/t.rs"
expect "a todo in a test fails" 1 lib no-unimplemented core
output_has "tests are included" "crates/taskdeck-core/tests/t.rs:2:"
printf 'fn t() { %s }\n' "$UNREACH_TODO" >"$R/crates/taskdeck-core/tests/t.rs"
expect "unreachable with a TODO fails" 1 lib no-unimplemented core
for u in "unimplemented ""! ()" "todo""!{}" "todo""![]" "core::todo""!(\"x\")" "unreachable ""! ( \"TODO\")"; do
    printf 'fn t() { %s }\n' "$u" >"$R/crates/taskdeck-core/tests/t.rs"
    expect "the spelling '$u' fails" 1 lib no-unimplemented core
done
printf 'fn t() {\n    unimplemented''!\n    ()\n}\n' >"$R/crates/taskdeck-core/tests/t.rs"
expect "a macro whose brackets are on the next line fails" 1 lib no-unimplemented core
output_has "its line is named" "crates/taskdeck-core/tests/t.rs:2:"
for u in 'todo\n    !()' 'unimplemented\n!\n{}' 'unreachable''!(\n    "TODO: later")'; do
    printf "fn t() {\n    $u\n}\n" >"$R/crates/taskdeck-core/tests/t.rs"
    expect "a macro split over lines fails: '$u'" 1 lib no-unimplemented core
    output_has "the line it starts on is named" "crates/taskdeck-core/tests/t.rs:2:"
done
printf 'fn t(x: u8) { if x > 1 { unreachable!("x is a bit") } }\n// todo list: none\n' >"$R/crates/taskdeck-core/tests/t.rs"
expect "another unreachable passes" 0 lib no-unimplemented core
printf '%s\n' "$UNIMPL" >"$R/crates/taskdeck-core/target/debug/gen.rs"
printf 'Bodies are %s in foundation.\n' "$UNIMPL" >"$R/crates/taskdeck-core/README.md"
expect "target/ and Markdown are not scanned" 0 lib no-unimplemented core
expect "a stage whose paths do not exist yet passes" 0 lib no-unimplemented relay
output_has "and says so" "not present yet: relay"
expect "an unknown stage is a usage error" 2 lib no-unimplemented hub

# --- no-skipped-tests ---------------------------------------------------------
R="$TMP/s"
mkdir -p "$R/crates/taskdeck-seal/tests" "$R/relay/test" "$R/android/app/src/test/kotlin" "$R/src" "$R/tests/e2e" "$R/scripts/tests"
echo "fn ok() {}" >"$R/crates/taskdeck-seal/tests/a.rs"
expect "a clean seal passes" 0 lib no-skipped-tests seal
printf '%s\nfn slow() {}\n' "$IGNORE" >"$R/crates/taskdeck-seal/tests/a.rs"
expect "an ignored Rust test fails" 1 lib no-skipped-tests seal
output_has "it is named" "crates/taskdeck-seal/tests/a.rs:1:"
printf '#[ignore = "%s"]\n' "slow" >"$R/crates/taskdeck-seal/tests/a.rs"
expect "an ignore with a reason fails too" 1 lib no-skipped-tests seal
for rs in "# [ig""nore]" "#[ ig""nore ]" "#![ig""nore]" "# ! [ig""nore = \"slow\"]" "#[cfg_attr(not(feature = \"slow\"), ig""nore)]"; do
    printf '#[test]\n%s\nfn slow() {}\n' "$rs" >"$R/crates/taskdeck-seal/tests/a.rs"
    expect "seal: '$rs' fails" 1 lib no-skipped-tests seal
done
# Attributes split over lines, as rustfmt writes a long cfg_attr.
for rs in '#[cfg_attr(\n    not(feature = "slow"),\n    ig''nore\n)]' '#[\n    ig''nore\n]'; do
    printf "#[test]\n$rs\nfn slow() {}\n" >"$R/crates/taskdeck-seal/tests/a.rs"
    expect "seal: a split '$rs' fails" 1 lib no-skipped-tests seal
    output_has "the attribute's first line is named" "crates/taskdeck-seal/tests/a.rs:2:"
done
# A source directory named build is scanned; Gradle's build output is not.
mkdir -p "$R/crates/taskdeck-seal/src/build"
echo "fn ok() {}" >"$R/crates/taskdeck-seal/tests/a.rs"
printf '#[test]\n%s\nfn a() {}\n' "$IGNORE" >"$R/crates/taskdeck-seal/src/build/tests.rs"
expect "seal: a module directory named build is scanned" 1 lib no-skipped-tests seal
rm -r "$R/crates/taskdeck-seal/src/build"
mkdir -p "$R/android/app/build/generated"
echo "plugins {}" >"$R/android/app/build.gradle.kts"
echo "@Ig""nore class Gen" >"$R/android/app/build/generated/Gen.kt"
expect "android: Gradle's build directory is not scanned" 0 lib no-skipped-tests android
rm -r "$R/android/app/build" "$R/android/app/build.gradle.kts"
printf '#[test]\nfn inner() {\n    %s\n}\n' "#![ig""nore]" >"$R/crates/taskdeck-seal/tests/a.rs"
expect "seal: an inner ignore in a test body fails" 1 lib no-skipped-tests seal
output_has "its line is named" "crates/taskdeck-seal/tests/a.rs:3:"
printf '#[cfg_attr(test, derive(Debug))]\nstruct S;\nfn f(v: &[u8], n: usize) { let _ = v.iter().skip(1).skip(n); }\n' >"$R/crates/taskdeck-seal/tests/a.rs"
expect "seal: another cfg_attr and Iterator::skip are not skips" 0 lib no-skipped-tests seal
printf 'fn f(s: &str, ctx: &mut Cursor, t: &[u8]) {\n    let it = s.chars();\n    let _ = it.skip(1).next();\n    ctx.skip(4);\n    let _ = t.iter().count(); let _ = t.len(); test.skip(n);\n}\n' >"$R/crates/taskdeck-seal/tests/a.rs"
expect "seal: Iterator::skip on a binding named it, ctx or test is not a skip" 0 lib no-skipped-tests seal
# In JavaScript the title may be a variable, as a table-driven suite writes it.
for js in "for (const c of cases) {\n  it.sk""ip(c.name, () => {});\n}" "test.sk""ip(name, async () => {});" \
    "describe.sk""ip(suiteName, () => {});" "it.to""do(c.name);" "suite.sk""ip(\`\${n}\`, () => {});"; do
    printf "$js\n" >"$R/relay/test/a.test.js"
    expect "relay: '$js' fails" 1 lib no-skipped-tests relay
done
mkdir -p "$R/android/page/test"
printf '<script type="module">\nfor (const c of cases) it.sk''ip(c.name, () => {});\n</script>\n' >"$R/android/page/test/a.html"
expect "android: a skip in the page's test page fails" 1 lib no-skipped-tests android
output_has "its line is named" "android/page/test/a.html:2:"
rm -r "$R/android/page"
for js in "it.sk""ip('x', () => {});" "test.sk""ip('x', () => {});" "describe.sk""ip('x', () => {});" "x""it('x', () => {});" \
    "test('x', { sk""ip: true }, () => {});" "it('y', { sk""ip: 'later' }, () => {});" "it('y', { to""do: true }, () => {});" \
    "x""describe('x', () => {});" "x""test('x', () => {});" "it.to""do('v');" "test('x', (t) => { t.sk""ip(); });" \
    "test('x', (t) => { t.to""do(); });" "describe .sk""ip('x', () => {});" \
    "test('x', { sk""ip }, () => {});" "test('x', { timeout: 5, to""do }, () => {});" "test('x', { 'sk""ip': true }, () => {});" \
    "it('x', { \"to""do\": 'later' }, () => {});" "test('a => b', { sk""ip: true }, () => {});" \
    "test('x', (ctx) => { ctx.sk""ip('later'); });" "test('x', (context) => { context.to""do(); });" \
    "test('x', (c) => { c.sk""ip('later'); });" "test('x', (t) => { t?.sk""ip(); });" \
    "test['sk""ip']('x', () => {});" "describe[\"to""do\"]('x', () => {});"; do
    echo "$js" >"$R/relay/test/a.test.js"
    expect "relay: '$js' fails" 1 lib no-skipped-tests relay
done
# Options on lines of their own, as a formatter writes a long call.
for js in "test('x', {\n  sk""ip: true,\n}, () => {});" "test(\n  'apply (merge) keeps order',\n  { to""do: true },\n  () => {},\n);" \
    "it(name, {\n  timeout: 5,\n  sk""ip: 'later',\n}, () => {});"; do
    printf "$js\n" >"$R/relay/test/a.test.js"
    expect "relay: a split '$js' fails" 1 lib no-skipped-tests relay
    output_has "the call's first line is named" "relay/test/a.test.js:1:"
done
echo "process.exit(0); edit.skipped = 1; latest.skip = 2; test('x', { timeout: 5 }, () => {});" >"$R/relay/test/a.test.js"
expect "relay: exit( and other words are not skips" 0 lib no-skipped-tests relay
printf 'if (/x/.test(s)) {\n  opts = { sk''ip: 1 };\n}\n' >"$R/relay/test/a.test.js"
expect "relay: an object after a .test() call is not a test option" 0 lib no-skipped-tests relay
for js in "test('pagination', () => {\n  const q = { sk""ip: 10, limit: 5 };\n  assert.equal(q.sk""ip, 10);\n});" \
    "test('x', async function () {\n  const q = { to""do: [] };\n});" "it('x', { timeout: 5 }, (t) => {\n  run({ sk""ip: 1 });\n});"; do
    printf "$js\n" >"$R/relay/test/a.test.js"
    expect "relay: an object in a test body is not a test option: '$js'" 0 lib no-skipped-tests relay
done
for kt in "@Ig""nore fun x() {}" "@Dis""abled fun x() {}" "@Dis""abledOnOs(OS.MAC) fun x() {}" "assumeTrue(fa""lse)" \
    "@org.junit.Ig""nore fun x() {}" "@org.junit.jupiter.api.Dis""abled fun x() {}" "assumeTrue(\n    fa""lse)" \
    "Assume.assumeTrue(\"not yet\", fa""lse)" "Assume.assumeFalse(tr""ue)" "assumeFalse(tr""ue, \"later\")"; do
    printf "$kt\n" >"$R/android/app/src/test/kotlin/A.kt"
    expect "android: '$kt' fails" 1 lib no-skipped-tests android
done
printf 'assumeTrue("ok", true)\nassumeFalse(false)\nassumeTrue(isLinux)\n' >"$R/android/app/src/test/kotlin/A.kt"
expect "android: assumptions that hold or depend on the system pass" 0 lib no-skipped-tests android
rm "$R/android/app/src/test/kotlin/A.kt"
printf '%s\n' "$IGNORE" >"$R/tests/e2e/x.rs"
expect "desktop does not scan tests/e2e" 0 lib no-skipped-tests desktop
expect "device does" 1 lib no-skipped-tests device
printf '%s\n' "$IGNORE" >"$R/src/phone.rs"
expect "desktop scans src" 1 lib no-skipped-tests desktop
output_has "the file is named" "src/phone.rs:1:"
printf 'echo "%s"\n' "$IGNORE" >"$R/scripts/tests/test-x.sh"
expect "shell scripts are not scanned" 0 lib no-skipped-tests foundation
expect "rc scans every path" 1 lib no-skipped-tests rc
mkdir -p "$R/docs/sync/status/w"
printf "const task = 'what is still %s and %s here';\n" "$UNIMPL" "$IGNORE" >"$R/docs/sync/status/w/r.js"
rm "$R/src/phone.rs"
expect "rc does not scan the records under docs/" 0 lib no-unimplemented rc
printf 'fn f() { %s }\n' "$UNIMPL" >"$R/src/x.rs"
expect "rc still scans the rest" 1 lib no-unimplemented rc
output_has "the code is named" "src/x.rs:1:"
rm -r "$R/src/x.rs" "$R/docs"
# A scan leaves no bytecode beside the scripts (python3 -B), so a gate run
# does not change the tree.
mkdir -p "$TMP/copy/scripts"
cp "$LIB" "$SCRIPTS_DIR/gate_checks.py" "$SCRIPTS_DIR/speclib.py" "$TMP/copy/scripts/"
expect "a scan from a copy of the scripts runs" 1 bash "$TMP/copy/scripts/gate-lib.sh" --root "$R" no-skipped-tests rc
expect "and leaves no __pycache__ there" 1 test -e "$TMP/copy/scripts/__pycache__"
expect "owned-paths prints a stage's paths" 0 lib owned-paths device
output_has "the engine's crate passes to device" "crates/taskdeck-sync"

# --- data checks --------------------------------------------------------------
R="$TMP/d"
mkdir -p "$R/v" "$R/relay"
echo '{"spec": "seal.md §4", "cases": [{"name": "a", "key": "00ff", "out": "abcd"}]}' >"$R/v/frame.json"
expect "well-formed vectors pass" 0 lib vectors v
echo '{"spec": "seal.md §4", "cases": [{"name": "a", "key": "00FF"}]}' >"$R/v/frame.json"
expect "uppercase hex fails" 1 lib vectors v
output_has "it is named" "is not lowercase"
echo '{"spec": "seal.md §4", "cases": [{"name": "a", "k": "00"}, {"name": "a", "k": "01"}]}' >"$R/v/frame.json"
expect "a case name twice fails" 1 lib vectors v
echo '{"spec": "seal", "cases": [{"name": "a", "k": "00"}]}' >"$R/v/frame.json"
expect "a spec field without a section fails" 1 lib vectors v
echo '{"spec": "seal.md §4", "spec": "seal.md §5", "cases": [{"name": "a", "k": "00"}]}' >"$R/v/frame.json"
expect "a duplicate key fails" 1 lib vectors v
echo '{"spec": "seal.md §4", "cases": []}' >"$R/v/frame.json"
expect "no cases fails" 1 lib vectors v
expect "a missing vector directory fails" 1 lib vectors nope

echo '{"name": "relay", "devDependencies": {"x": "1.0.0"}}' >"$R/relay/package.json"
expect "a package with only devDependencies passes" 0 lib npm-no-deps relay/package.json
echo '{"name": "relay", "dependencies": {"x": "1.0.0"}}' >"$R/relay/package.json"
expect "a runtime dependency fails" 1 lib npm-no-deps relay/package.json
echo '{"name": "relay", "optionalDependencies": {"x": "1.0.0"}}' >"$R/relay/package.json"
expect "an optional dependency fails" 1 lib npm-no-deps relay/package.json
expect "a missing package.json fails" 1 lib npm-no-deps nope/package.json

printf 'disallowed-methods = [ { path = "std::fs::read" }, { path = "std::thread::spawn" } ]\n' >"$R/clippy.toml"
printf '# comment\ndisallowed-methods std::fs::read\ndisallowed-methods std::thread::spawn\n' >"$R/req.txt"
expect "a clippy.toml with every entry passes" 0 lib clippy-config clippy.toml req.txt
printf 'disallowed-methods = [ { path = "std::fs::read" } ]\n' >"$R/clippy.toml"
expect "a removed entry fails" 1 lib clippy-config clippy.toml req.txt
output_has "it is named" "no longer has std::thread::spawn in disallowed-methods"
printf 'disallowed-types = [ { path = "std::thread::spawn" }, { path = "std::fs::read" } ]\n' >"$R/clippy.toml"
expect "an entry in the wrong table fails" 1 lib clippy-config clippy.toml req.txt
expect "the real core clippy.toml keeps its entries" 0 bash "$LIB" clippy-config crates/taskdeck-core/clippy.toml scripts/gates/core-clippy-required.txt
# A crate that turns the disallowed lints off forbids nothing (SYNC_PLAN.md §6.2).
C="$R/crates/c"
mkdir -p "$C/src" "$C/tests"
printf 'disallowed-methods = [ { path = "std::fs::read" }, { path = "std::thread::spawn" } ]\n' >"$C/clippy.toml"
printf '[package]\nname = "c"\n' >"$C/Cargo.toml"
printf '#![allow(clippy::needless_return, dead_code)]\npub fn f() {}\n' >"$C/src/lib.rs"
expect "a crate that allows other lints passes" 0 lib clippy-config crates/c/clippy.toml req.txt
for a in "#![al""low(clippy::disallowed_methods)]" "#[ex""pect(clippy::disallowed_types)]" "#![cfg_attr(clippy, al""low(clippy::all))]" \
    "#[al""low(\n    dead_code,\n    clippy::style,\n)]" "#![al""low(warnings)]"; do
    printf "$a\npub fn g() {}\n" >"$C/tests/t.rs"
    expect "clippy-config: '$a' fails" 1 lib clippy-config crates/c/clippy.toml req.txt
    output_has "the attribute is named" "crates/c/tests/t.rs:1: turns the disallowed lints off"
done
rm "$C/tests/t.rs"
printf '[package]\nname = "c"\n[lints.clippy]\ndisallowed_methods = { level = "allow", priority = 1 }\n' >"$C/Cargo.toml"
expect "a Cargo.toml that allows them fails" 1 lib clippy-config crates/c/clippy.toml req.txt
output_has "it is named" "clippy::disallowed_methods is allowed by its lints"
printf '[package]\nname = "c"\n[lints]\nworkspace = true\n' >"$C/Cargo.toml"
printf '[workspace.lints.clippy]\nall = "allow"\n' >"$R/Cargo.toml"
expect "workspace lints the crate takes count" 1 lib clippy-config crates/c/clippy.toml req.txt
output_has "it is named" "clippy::all is allowed by its lints"
rm "$R/Cargo.toml"
mkdir -p "$R/.cargo"
printf '[build]\nrustflags = ["--cap-lints", "warn"]\n' >"$R/.cargo/config.toml"
expect "a .cargo config that caps the lints fails" 1 lib clippy-config crates/c/clippy.toml req.txt
output_has "it is named" ".cargo/config.toml: lowers lint levels"
printf '[env]\nCLIPPY_CONF_DIR = { value = "elsewhere", relative = true }\n' >"$R/.cargo/config.toml"
expect "a .cargo config that points clippy at another config fails" 1 lib clippy-config crates/c/clippy.toml req.txt
output_has "it is named" ".cargo/config.toml: sets CLIPPY_CONF_DIR"
rm -r "$R/.cargo"
# Clippy reads .clippy.toml before clippy.toml, so an empty one forbids nothing.
printf '# nothing\n' >"$C/.clippy.toml"
expect "a .clippy.toml beside the clippy.toml fails" 1 lib clippy-config crates/c/clippy.toml req.txt
output_has "it is named" "crates/c/.clippy.toml: clippy reads it instead of crates/c/clippy.toml"
rm "$C/.clippy.toml"
expect "CLIPPY_CONF_DIR in the environment fails" 1 env CLIPPY_CONF_DIR="$TMP" bash "$LIB" --root "$R" clippy-config crates/c/clippy.toml req.txt
output_has "it is named" "CLIPPY_CONF_DIR is set"
expect "without them the crate passes again" 0 lib clippy-config crates/c/clippy.toml req.txt

# --- removed-modules (a tree that is not a git checkout) ----------------------
R="$TMP/r"
mkdir -p "$R/src" "$R/docs"
echo 'mod board;' >"$R/src/lib.rs"
echo '[dependencies]' >"$R/Cargo.toml"
echo 'The old taskdeck-server is history.' >"$R/docs/history.txt"
echo 'The old taskdeck-server is history.' >"$R/SERVER.md"
expect "a tree without the removed modules passes" 0 lib removed-modules
echo 'fn x() {}' >"$R/src/phone.rs"
expect "a removed file still there fails" 1 lib removed-modules
output_has "it is named" "src/phone.rs still exists"
rm "$R/src/phone.rs"
echo 'mod phone;' >"$R/src/lib.rs"
expect "a mod line in the root package fails" 1 lib removed-modules
echo 'mod board;' >"$R/src/lib.rs"
printf '[[package]]\nname = "tiny_""http"\n' | sed 's/""//' >"$R/Cargo.lock"
expect "a removed dependency in Cargo.lock fails" 1 lib removed-modules
output_has "the line is named" "Cargo.lock:2:"

finish
