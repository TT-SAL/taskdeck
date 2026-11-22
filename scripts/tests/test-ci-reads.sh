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

# tree <dir>: a git repository of the files written under it.
tree() { git -C "$1" init -q && git -C "$1" add -A; }

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
ln -s ../docs/sync/status/x.rs "$BAD/src/sym.rs"
ln -s ../docs/sync/status "$BAD/src/assets"
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
output_has "a path value the check cannot read fails" \
    'Cargo.toml: a value this check cannot read (write it on one line, as a plain string): taskdeck-android = { path = """'
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
output_has "a symlink into a path that needs no CI" \
    "the symlink src/sym.rs: a change to docs/sync/status/x.rs does not mark root"
output_has "a symlink to a directory that needs no CI" \
    "the symlink src/assets: a change to docs/sync/status/x does not mark root"

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
ln -s main.rs "$GOOD/src/other.rs"
tree "$GOOD"
expect "a tree whose reads changes.sh covers passes" 0 bash "$READS" "$GOOD"

finish
