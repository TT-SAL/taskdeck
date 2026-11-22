#!/usr/bin/env bash
# Tests for scripts/ci/reads.sh: this repository passes, and each kind of read
# that changes.sh would miss (a path dependency, an include, a #[path], a
# symlink) fails, on throwaway trees. Offline; touches nothing outside a
# temporary directory.
set -uo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
READS="$SCRIPTS_DIR/ci/reads.sh"
unset GITHUB_OUTPUT CI_EVENT CI_BEFORE CI_AFTER CI_HEAD_MESSAGE
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1

expect "this repository: every read marks its reader's areas" 0 bash "$READS"
expect "this repository, from elsewhere" 0 env -C "$TMP" bash "$READS" "$REPO_DIR"

echo "-- a tree git cannot list fails"
NOGIT="$TMP/nogit"
mkdir -p "$NOGIT/src" "$NOGIT/inner/src"
printf '[package]\nname = "app"\n' >"$NOGIT/Cargo.toml"
printf 'const X: &str = include_str!("../docs/sync/status/x.txt");\n' >"$NOGIT/src/main.rs"
expect "a directory that is not a git work tree" 2 bash "$READS" "$NOGIT"
cp "$NOGIT/Cargo.toml" "$NOGIT/src/main.rs" "$NOGIT/inner/"
git -C "$NOGIT" init -q
expect "a directory inside a work tree, not its top" 2 bash "$READS" "$NOGIT/inner"
expect "a work tree that tracks no Cargo.toml" 2 bash "$READS" "$NOGIT"

# tree <dir>: a git repository of the files written under it.
tree() { git -C "$1" init -q && git -C "$1" add -A; }
# touch_in <dir> <path>...: empty files, so a read names a tracked file.
touch_in() { local d="$1" p; shift; for p in "$@"; do mkdir -p "$(dirname "$d/$p")" && : >"$d/$p"; done; }

echo "-- reads changes.sh would miss"
BAD="$TMP/bad"
mkdir -p "$BAD/src" "$BAD/crates/taskdeck-device/src" "$BAD/crates/taskdeck-core/src" \
    "$BAD/docs/sync/status" "$BAD/deploy" "$BAD/crates/taskdeck-seal/src" "$BAD/relay/src"
cat >"$BAD/Cargo.toml" <<'EOF'
[workspace.dependencies]
serde = "1"
taskdeck-core = { path = "crates/taskdeck-core" }
taskdeck-sync = { path = "crates/taskdeck-sync" }
taskdeck-recover = { features = [
    "x",
], path = 'crates/taskdeck-recover' }
[workspace.dependencies.taskdeck-seal]
path = "crates/taskdeck-seal"
[workspace.dependencies."relay"]
path = "relay"
[package]
name = "app"
build = "deploy/build.rs"
[[bin]]
name = "app"
path = "src/main.rs"
[dependencies]
serde.workspace = true
taskdeck-device = { path = "crates/taskdeck-device" }
taskdeck-core.workspace = true
taskdeck-seal = { workspace = true, features = [] }
taskdeck-backup = { path = 'crates/taskdeck-backup' }
taskdeck-sync = { features = [
    "x",
], workspace = true }
'taskdeck-recover'.workspace = true
taskdeck-android = { path = """crates/taskdeck-android""" }
[[test]]
name = "t"
path = 'docs/sync/runbooks/t.rs'
EOF
cat >"$BAD/crates/taskdeck-device/Cargo.toml" <<'EOF'
[lib]
path = "../../docs/sync/research/lib.rs"
[dependencies.taskdeck-core]
path = "../taskdeck-core"
[dev-dependencies.relay]
workspace = true
EOF
cat >"$BAD/crates/taskdeck-seal/Cargo.toml" <<'EOF'
dependencies.dotted.path = "../../docs/sync/research/dotted"
[target.'cfg(unix)'.dependencies]
unix = { path = "../../docs/sync/research/unix" }
[target."cfg(windows)".dev-dependencies.windows]
path = "../../docs/sync/research/windows"
[build-dependencies]
lit = { path = '''../../docs/sync/research/lit''' }
cont = { path = """
../../docs/sync/research/\
    cont""" }
[patch.crates-io]
num = { path = 7 }
EOF
mkdir -p "$BAD/crates/taskdeck-broken"
printf '[package]\nname = "broken"\n[dependencies\nx = { path = "../x" }\n' >"$BAD/crates/taskdeck-broken/Cargo.toml"
cat >"$BAD/src/main.rs" <<'EOF'
const A: &str = include_str!("../docs/sync/status/a.rs");
#[path = "../deploy/b.rs"]
mod b;
const C: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/deploy/c"));
const D: &str = include_str!["../docs/sync/status/d.txt"];
const E: &str = include_str!{"../docs/sync/status/e.txt"};
#[cfg_attr(all(), path = "../docs/sync/status/f.rs")]
mod f;
const G: &str = include_str!(
    "../docs/sync/status/g.txt"
);
EOF
cat >"$BAD/src/esc.rs" <<'EOF'
pub const A: &str = include_str!("\x2e\x2e/docs/sync/status/x.txt");
#[path = "\x2e\x2e/docs/sync/status/m.rs"] mod m;
pub const U: &str = include_str!("\u{2e}./docs/sync/status/x.txt");
#[cfg(windows)] pub const W: &[u8] = include_bytes!(r"..\docs\sync\status\x.txt");
pub const C: &[u8] = include_bytes!(concat!("\x2e\x2e/docs/sync/status/", "x.txt"));
EOF
cat >"$BAD/src/joined.rs" <<'EOF'
pub const A: &str = include_str!(concat!("", "../docs/sync/status/ja.txt"));
pub const B: &str = include_str!(concat!("../", "docs/sync/status/jb.txt",));
pub const C: &str = include_str!(concat!("../weather_svgs_2/", "../docs/sync/status/jc.txt"));
pub const D: &str = include_str!(concat!("../", "docs/sync/status/j,d.txt"));
pub const E: &str = include_str!(r#"../docs/sync/status/je")"#);
pub const O: &str = include_str!("../../outside.txt");
EOF
printf '#[path = "probe_m.inc"] pub mod probe_m;\n' >"$BAD/src/probe_inc.rs"
printf 'pub const X: &str = include_str!("../docs/sync/status/inc.txt");\n' >"$BAD/src/probe_m.inc"
printf 'mod a { #[path = "../../docs/sync/status/m.rs"] pub mod m; }\n' >"$BAD/src/inline.rs"
printf 'mod a /* ; */ {\n#[path = "a/m.rs"]\npub mod m; }\n' >"$BAD/src/inline2.rs"
ln -s ../docs/sync/status/x.rs "$BAD/src/sym.rs"
ln -s ../docs/sync/status "$BAD/src/assets"
touch_in "$BAD" crates/taskdeck-{device,core,seal,backup,sync,recover,android}/src/lib.rs relay/src/lib.rs \
    docs/sync/research/{dotted,lit,cont,unix,windows}/src/lib.rs docs/sync/research/lib.rs docs/sync/runbooks/t.rs \
    deploy/build.rs deploy/b.rs docs/sync/status/{a.rs,d.txt,e.txt,f.rs,g.txt,ja.txt,jb.txt,jc.txt,inc.txt,x.rs}
tree "$BAD"
expect "a tree with missed reads fails" 1 bash "$READS" "$BAD"
output_has "the root package's path dependency on a crate that does not mark root" \
    "Cargo.toml depends on crates/taskdeck-device: a change to crates/taskdeck-device/src/lib.rs does not mark root"
output_lacks "a path dependency whose crate marks the reader's areas passes" "depends on ../taskdeck-core"
output_lacks "a path outside a dependencies table is not a dependency" "depends on src/main.rs"
output_has "a dependency inherited from [workspace.dependencies] with name.workspace" \
    "Cargo.toml depends on crates/taskdeck-core (workspace): a change to crates/taskdeck-core/src/lib.rs does not mark root"
output_has "a dependency inherited from [workspace.dependencies] with { workspace = true }" \
    "Cargo.toml depends on crates/taskdeck-seal (workspace): a change to crates/taskdeck-seal/src/lib.rs does not mark root"
output_has "a dependency table inherited from a [workspace.dependencies] table" \
    "crates/taskdeck-device/Cargo.toml depends on relay (workspace): a change to relay/src/lib.rs does not mark foundation"
output_lacks "a registry dependency inherited from the workspace is not a read" "depends on serde"
output_has "a [lib] path into a path that needs no CI" \
    "crates/taskdeck-device/Cargo.toml builds ../../docs/sync/research/lib.rs: a change to docs/sync/research/lib.rs does not mark"
output_has "a build script in a path that needs no CI" \
    "Cargo.toml builds deploy/build.rs: a change to deploy/build.rs does not mark root"
output_has "include_str! into a path that needs no CI" \
    "src/main.rs reads ../docs/sync/status/a.rs: a change to docs/sync/status/a.rs does not mark root"
output_has "#[path] into a path that needs no CI" "src/main.rs reads ../deploy/b.rs: a change to deploy/b.rs does not mark root"
output_has "a read through env! cannot be checked" "cannot be checked: env!"
output_has "a path dependency written as a TOML literal string" \
    "Cargo.toml depends on crates/taskdeck-backup: a change to crates/taskdeck-backup/src/lib.rs does not mark root"
output_has "workspace = true after a multi-line array in its inline table" \
    "Cargo.toml depends on crates/taskdeck-sync (workspace): a change to crates/taskdeck-sync/src/lib.rs does not mark root"
output_has "a [workspace.dependencies] path after a multi-line array, inherited under a quoted key" \
    "Cargo.toml depends on crates/taskdeck-recover (workspace): a change to crates/taskdeck-recover/src/lib.rs does not mark root"
output_has "a path written as a multi-line TOML string" \
    "Cargo.toml depends on crates/taskdeck-android: a change to crates/taskdeck-android/src/lib.rs does not mark root"
output_has "a path written as a multi-line literal TOML string" \
    "crates/taskdeck-seal/Cargo.toml depends on ../../docs/sync/research/lit: a change to docs/sync/research/lit/src/lib.rs does not mark"
output_has "a path written as a multi-line string with a line-ending backslash" \
    "crates/taskdeck-seal/Cargo.toml depends on ../../docs/sync/research/cont: a change to docs/sync/research/cont/src/lib.rs does not mark"
output_has "a dependency written as a dotted key" \
    "crates/taskdeck-seal/Cargo.toml depends on ../../docs/sync/research/dotted: a change to docs/sync/research/dotted/src/lib.rs does not mark"
output_has "a dependency in a target-specific dependencies table" \
    "crates/taskdeck-seal/Cargo.toml depends on ../../docs/sync/research/unix: a change to docs/sync/research/unix/src/lib.rs does not mark"
output_has "a dependency table under a target-specific dev-dependencies table" \
    "crates/taskdeck-seal/Cargo.toml depends on ../../docs/sync/research/windows: a change to docs/sync/research/windows/src/lib.rs does not mark"
output_has "a manifest that is not valid TOML fails and is named" "crates/taskdeck-broken/Cargo.toml: not valid TOML"
output_has "a path that is not a string fails" "crates/taskdeck-seal/Cargo.toml: a path that is not a string: 7"
output_has "a target path written as a TOML literal string" \
    "Cargo.toml builds docs/sync/runbooks/t.rs: a change to docs/sync/runbooks/t.rs does not mark root"
output_has "include_str! with [] delimiters" \
    "src/main.rs reads ../docs/sync/status/d.txt: a change to docs/sync/status/d.txt does not mark root"
output_has "include_str! with {} delimiters" \
    "src/main.rs reads ../docs/sync/status/e.txt: a change to docs/sync/status/e.txt does not mark root"
output_has "a path inside #[cfg_attr(...)]" \
    "src/main.rs reads ../docs/sync/status/f.rs: a change to docs/sync/status/f.rs does not mark root"
output_has "an include spanning lines" \
    "src/main.rs reads ../docs/sync/status/g.txt: a change to docs/sync/status/g.txt does not mark root"
output_has "include_str! with an escape cannot be checked" \
    'src/esc.rs: a read whose path is not a string literal without a backslash cannot be checked: "\x2e\x2e/docs/sync/status/x.txt"'
output_has "#[path] with an escape cannot be checked" \
    'cannot be checked: "\x2e\x2e/docs/sync/status/m.rs"'
output_has "include_str! with a \\u{} escape cannot be checked" 'cannot be checked: "\u{2e'
output_has "a raw literal with Windows separators cannot be checked" \
    'cannot be checked: r"..\docs\sync\status\x.txt"'
output_has "concat! whose first literal has an escape cannot be checked" \
    'cannot be checked: "\x2e\x2e/docs/sync/status/"'
output_has "concat! whose first literal is empty is checked joined" \
    "src/joined.rs reads ../docs/sync/status/ja.txt: a change to docs/sync/status/ja.txt does not mark root"
output_has "concat! whose first literal is a broad directory, with a trailing comma, is checked joined" \
    "src/joined.rs reads ../docs/sync/status/jb.txt: a change to docs/sync/status/jb.txt does not mark root"
output_has "concat! whose first literal names a code directory is checked joined" \
    "a change to docs/sync/status/jc.txt does not mark root"
output_has "concat! with a comma inside a literal cannot be checked" 'cannot be checked: "docs/sync/status/j'
output_has "a raw literal holding a quote and a bracket cannot be checked" 'cannot be checked: r#"../docs/sync/status/je"'
output_has "a read outside the tree fails" "src/joined.rs reads ../../outside.txt: ../outside.txt is outside the tree"
output_has "a file that #[path] pulls in is read like a .rs file" \
    "src/probe_m.inc reads ../docs/sync/status/inc.txt: a change to docs/sync/status/inc.txt does not mark root"
output_has "#[path] inside an inline module cannot be checked" \
    'src/inline.rs: a #[path] in a file with an inline module'
output_has "#[path] inside an inline module behind a comment cannot be checked" \
    'src/inline2.rs: a #[path] in a file with an inline module'
output_has "a symlink into a path that needs no CI" \
    "the symlink src/sym.rs: a change to docs/sync/status/x.rs does not mark root"
output_has "a symlink to a directory that needs no CI" \
    "the symlink src/assets: a change to docs/sync/status/a.rs does not mark root"

echo "-- reads checked as written, and reads of no tracked file or of a path no rule knows"
# A read through a tracked symlink is checked against the link: retargeting a
# link kept under a no-CI path changes what the code embeds with CI skipped.
ODD="$TMP/odd"
mkdir -p "$ODD/src/data" "$ODD/docs/sync/status/probe" "$ODD/weird"
printf '[package]\nname = "app"\n' >"$ODD/Cargo.toml"
touch_in "$ODD" src/probe_data.txt src/data/d.txt weird/unknown.bin docs/sync/status/s.txt
ln -s ../../../../src/probe_data.txt "$ODD/docs/sync/status/probe/link.txt"
ln -s ../../../src/data "$ODD/docs/sync/status/dirlink"
ln -s data "$ODD/src/dl"
ln -s ../docs/sync/status/l2 "$ODD/src/l1"
ln -s ../../../src/probe_data.txt "$ODD/docs/sync/status/l2"
cat >"$ODD/src/main.rs" <<'EOF'
const L: &str = include_str!("../docs/sync/status/probe/link.txt");
const D: &str = include_str!("../docs/sync/status/dirlink/d.txt");
const S: &str = include_str!("dl/d.txt");
#[cfg(any())]
const C: &str = include_str!("not-there.txt");
#[cfg(any())]
const G: &[u8] = include_bytes!("../weird/gone.bin");
const U: &[u8] = include_bytes!("../weird/unknown.bin");
macro_rules! m { ($s:literal) => {
    include_bytes!(concat!("../docs/sync/status/", $s));
    include_bytes!(concat!("../nodir/", $s));
    include!(concat!("data/", $s));
} }
EOF
tree "$ODD"
expect "a tree with reads that cannot be checked as written fails" 1 bash "$READS" "$ODD"
output_has "a read of a tracked symlink is checked against the link" \
    "src/main.rs reads ../docs/sync/status/probe/link.txt: a change to docs/sync/status/probe/link.txt does not mark root"
output_has "a read through a symlinked directory names no tracked file" \
    "src/main.rs reads ../docs/sync/status/dirlink/d.txt: docs/sync/status/dirlink/d.txt is not a file git tracks"
output_has "a read through a symlinked directory inside the reader's area names no tracked file" \
    "src/main.rs reads dl/d.txt: src/dl/d.txt is not a file git tracks"
output_has "a symlink to a symlink is checked against the second link" \
    "the symlink src/l1: a change to docs/sync/status/l2 does not mark root"
output_has "a read of a file that does not exist fails" "src/main.rs reads not-there.txt: src/not-there.txt is not a file git tracks"
output_has "a read into a directory that does not exist fails" \
    "src/main.rs reads ../weird/gone.bin: weird/gone.bin is not a file git tracks"
output_has "a read of a path no rule knows does not pass by marking every area" \
    "src/main.rs reads ../weird/unknown.bin: no rule in scripts/ci/changes.sh knows weird/unknown.bin"
output_has "concat! with a \$stem checks every tracked file under its directory" \
    "src/main.rs reads ../docs/sync/status/...: a change to docs/sync/status/s.txt does not mark root"
output_has "concat! with a \$stem must name a tracked directory" \
    "src/main.rs reads ../nodir/...: nodir is not a directory git tracks"
output_has "include! whose file is not one literal path cannot be checked" \
    'src/main.rs: an include! or #[path] whose file is not a single literal path cannot be checked: concat!("data/"'

echo "-- reads changes.sh covers"
GOOD="$TMP/good"
mkdir -p "$GOOD/src" "$GOOD/weather_svgs_2" "$GOOD/crates/a/src" "$GOOD/crates/b/src"
cat >"$GOOD/Cargo.toml" <<'EOF'
[workspace.dependencies]
a = { path = "crates/a" }
b = { path = "crates/b" }
[package]
name = "app"
build = "build.rs"
[[bin]]
name = "app"
path = "src/main.rs"
[dependencies]
# old = { path = "deploy/old" }
EOF
printf '[dev-dependencies]\nb = { path = \x27../b\x27 }\n[dependencies]\nb.workspace = true\n[lib]\npath = "src/lib.rs"\n' \
    >"$GOOD/crates/a/Cargo.toml"
cat >"$GOOD/src/main.rs" <<'EOF'
const I: &[u8] = include_bytes!("../icon.png");
const F: &[u8] = include_bytes!(r#"../fonts/x.ttf"#);
const W: &[u8] = include_bytes!(concat!("../weather_svgs_2/", "04", ".svg"));
const P: &str = include_str!("page.html");
EOF
printf '#[path = "m.inc"]\nmod m ;\n' >"$GOOD/src/lib.rs"
printf 'pub const I: &[u8] = include_bytes!("../icon.png");\n#[path = "n.rs"] mod n;\n' >"$GOOD/src/m.inc"
printf 'include!("../weather_svgs_2/gen.rs");\n#[cfg(test)]\nmod tests { const X: &[u8] = include_bytes!("../fonts/y.ttf"); }\n' >"$GOOD/src/n.rs"
cat >"$GOOD/src/w.rs" <<'EOF'
macro_rules! w { ($($stem:literal),+) => { $(include_bytes!(concat!("../weather_svgs_2/", $stem, ".svg"));)+ } }
const T: &str = include_str!("../Cargo.toml");
EOF
ln -s main.rs "$GOOD/src/other.rs"
touch_in "$GOOD" icon.png fonts/x.ttf fonts/y.ttf weather_svgs_2/04.svg weather_svgs_2/gen.rs src/page.html build.rs \
    crates/a/src/lib.rs crates/b/src/lib.rs
tree "$GOOD"
expect "a tree whose reads changes.sh covers passes" 0 bash "$READS" "$GOOD"

finish
