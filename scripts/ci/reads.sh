#!/usr/bin/env bash
# Checks that a change to anything the code reads from elsewhere in the tree
# marks at least the CI areas that a change to the reading code marks
# (scripts/ci/README.md, "What the code reads"). Run by the `changes` job, which
# every CI run has, so a read that changes.sh would miss fails the run (`gate`).
#
#   bash scripts/ci/reads.sh [<tree>]      default: this repository
#
# The reads, from the files git tracks:
#   - a path dependency in a Cargo.toml (any dependencies table, and [patch]),
#     written with `path` or inherited with `workspace = true` from the path in
#     the root Cargo.toml's [workspace.dependencies]: <dep>/src/lib.rs must mark
#     every area <manifest's directory>/src/lib.rs marks. So a crate the root
#     package links must mark root.
#   - a target's source in a Cargo.toml (`path` in [lib], [[bin]], [[test]],
#     [[example]], [[bench]]; `build` in [package]) must mark every area
#     <manifest's directory>/src/lib.rs marks.
#   - the path in include!, include_bytes!, include_str!, include_image! or
#     #[path = ...] in a .rs file, from the file's directory, must mark every
#     area the file marks. For concat!("dir/", ...) it is a file in "dir/". Any
#     other argument (env!, a variable) cannot be checked and fails.
#   - a symlink's target must mark every area the link's own path marks.
# The areas come from scripts/ci/changes.sh in test mode. Exits 1 naming each
# read that marks too little.
set -euo pipefail
CHANGES="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/changes.sh"
TREE="$(cd "${1:-$(dirname "$CHANGES")/../..}" && pwd -P)"
BAD=0

report() { local m="$*"; echo "reads: ${m//[[:cntrl:]]/?}"; }

areas() {   # the areas a change to <path> marks, space-separated
    printf '%s\n' "$1" | env -u GITHUB_OUTPUT -u CI_HEAD_MESSAGE CI_EVENT=push bash "$CHANGES" --paths-from - 2>/dev/null \
        | sed -n 's/=true$//p' | grep -vx wide | tr '\n' ' ' || true
}

# <path> from the tree's root, through .. and symlinks; a directory stands for a file in it.
as_file() {
    local r
    r="$(realpath -m --relative-to="$TREE" "$TREE/$1")"
    if [[ "$1" == */ || -d "$TREE/$r" ]]; then r="$r/x"; fi
    printf '%s\n' "$r"
}

check() {   # check <what> <reader path> <read path>
    local have missing="" a
    have=" $(areas "$3")"
    for a in $(areas "$2"); do
        if [[ "$have" != *" $a "* ]]; then missing+=" $a"; fi
    done
    if [[ -n "$missing" ]]; then
        report "$1: a change to $3 does not mark$missing, as a change to $2 does"
        BAD=1
    fi
}

# A manifest's reads, one per line: D <dep path>, W <name inherited from the
# workspace>, T <target source>, P <name> <path> for [workspace.dependencies].
manifest_reads() {
    awk 'function str(k,   s) {
            if (!match($0, k "[[:space:]]*=[[:space:]]*\"[^\"]*\"")) return ""
            s = substr($0, RSTART, RLENGTH); sub(/^[^"]*"/, "", s); sub(/"$/, "", s); return s }
        /^[[:space:]]*\[/ {
            h = $0; sub(/#.*/, "", h); gsub(/[[:space:]"]/, "", h); gsub(/^\[+|\]+$/, "", h)
            ws = h ~ /^workspace\.dependencies/; on = !ws && (h ~ /dependencies/ || h ~ /^patch/)
            tgt = h ~ /^(lib|bin|test|example|bench)$/; pkg = h == "package"
            sub_name = h; if (!sub(/.*dependencies\./, "", sub_name)) sub_name = ""
            next }
        /^[[:space:]]*#/ { next }
        { key = ""; if (match($0, /^[[:space:]]*"?[A-Za-z0-9_-]+/)) { key = substr($0, RSTART, RLENGTH); gsub(/[[:space:]"]/, "", key) }
          name = (key == "path" || key == "workspace") && sub_name != "" ? sub_name : key; p = str("path") }
        on && p != "" { print "D\t" p }
        on && /workspace[[:space:]]*=[[:space:]]*true/ { print "W\t" name }
        ws && p != "" { print "P\t" name "\t" p }
        tgt && key == "path" && p != "" { print "T\t" p }
        pkg && key == "build" && str("build") != "" { print "T\t" str("build") }' "$1"
}

declare -A WS=()
if [[ -f "$TREE/Cargo.toml" ]]; then
    while IFS=$'\t' read -r k name p; do
        if [[ "$k" == P && -n "$name" ]]; then WS["$name"]="$p"; fi
    done < <(manifest_reads "$TREE/Cargo.toml")
fi

while IFS= read -r -d '' m; do
    dir="$(dirname "$m")/"
    [[ "$dir" == ./ ]] && dir=""
    while IFS=$'\t' read -r k a; do
        case "$k" in
            D) check "$m depends on $a" "${dir}src/lib.rs" "$(realpath -m --relative-to="$TREE" "$TREE/$dir$a")/src/lib.rs" ;;
            W) if [[ -n "$a" && -n "${WS[$a]:-}" ]]; then
                   check "$m depends on ${WS[$a]} (workspace)" "${dir}src/lib.rs" "$(realpath -m --relative-to="$TREE" "$TREE/${WS[$a]}")/src/lib.rs"
               fi ;;
            T) check "$m builds $a" "${dir}src/lib.rs" "$(as_file "$dir$a")" ;;
        esac
    done < <(manifest_reads "$TREE/$m")
done < <(git -C "$TREE" ls-files -z -- Cargo.toml '*/Cargo.toml')

lead='(include(_bytes|_str|_image)?!|#\[path) *[(=] *(concat! *\( *)?'
while IFS= read -r -d '' f; do
    if [[ -L "$TREE/$f" || ! -f "$TREE/$f" ]]; then continue; fi   # a symlink is checked below
    while IFS= read -r arg; do
        if [[ "$arg" =~ ^r?#*\"([^\"]*)\"#*$ ]]; then
            check "$f reads ${BASH_REMATCH[1]}" "$f" "$(as_file "$(dirname "$f")/${BASH_REMATCH[1]}")"
        else
            report "$f: a read whose path is not a string literal cannot be checked: $arg"
            BAD=1
        fi
    done < <(grep -oE "$lead[^] ,)]*" "$TREE/$f" | sed -E "s/^$lead//")
done < <(git -C "$TREE" ls-files -z -- '*.rs')

while IFS= read -r -d '' entry; do
    if [[ "$entry" == 120000\ * ]]; then
        l="${entry#*$'\t'}"
        check "the symlink $l" "$l" "$(as_file "$l")"
    fi
done < <(git -C "$TREE" ls-files -s -z)

if [[ $BAD -ne 0 ]]; then
    report "FAILED: add the read path to the reader's areas in scripts/ci/changes.sh (see scripts/ci/README.md)"
    exit 1
fi
report "every read marks its reader's areas"
