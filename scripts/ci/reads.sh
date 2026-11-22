#!/usr/bin/env bash
# Checks that a change to anything the code reads from elsewhere in the tree
# marks at least the CI areas that a change to the reading code marks
# (scripts/ci/README.md, "What the code reads"). Run by the `changes` job, which
# every CI run has, so a read that changes.sh would miss fails the run (`gate`).
#
#   bash scripts/ci/reads.sh [<tree>]      default: this repository
# <tree> must be the top of a git work tree that tracks a Cargo.toml (exit 2 if not).
#
# The reads, from the files git tracks:
#   - a path dependency in a Cargo.toml (any dependencies table, also under
#     [target.<cfg>], and [patch]), written with `path` or inherited with
#     `workspace = true` from the path in the root Cargo.toml's
#     [workspace.dependencies]: every tracked file under <dep> must mark every
#     area <manifest's directory>/src/lib.rs marks. So a crate the root package
#     links must mark root. A dependency on the top of the tree (the root
#     package) stands for the tracked files under src/, build.rs and Cargo.toml.
#   - a target's source in a Cargo.toml (`path` in [lib], [[bin]], [[test]],
#     [[example]], [[bench]]; `build` in [package]) must mark every area
#     <manifest's directory>/src/lib.rs marks, and be a file git tracks.
#   Each Cargo.toml is parsed with python3's tomllib, so any TOML spelling is
#   read; one that does not parse, or a path that is not a string, fails.
#   - the path in include!, include_bytes!, include_str!, include_image! (with
#     any delimiter) or #[path = ...] (also inside #[cfg_attr(...)]) in a .rs
#     file, or in a file that a #[path] or include! pulls in, from the file's
#     directory, must mark every area the file marks. For concat! it is its
#     literals joined; when they are followed by something else (a macro's
#     $stem), the joined literals ("dir/" stands for a file in "dir/"). Any other
#     argument (env!, a variable), a literal holding a backslash (an escape such
#     as \x2e, or a Windows separator), a #[path] in a file with an inline
#     module (rustc resolves it from that module's directory) and a path outside
#     the tree cannot be checked and fail. The path must name a file git tracks;
#     the joined literals that a macro's $stem follows, a directory with tracked
#     files ("dir/stem_" stands for every tracked file under "dir/").
#   - a symlink's target (one hop: a link to a link checks the second link),
#     which must be a file or a directory git tracks, must mark every area the
#     link's own path marks.
# Paths are taken as written, `..` resolved without following symlinks, so a
# read through a tracked symlink is checked against the link (and the link
# against its target), and a read through a symlinked directory names no
# tracked file and fails. A read path that no rule of changes.sh knows (a
# change to it would mark every area) fails too; a shared file passes.
# The areas come from scripts/ci/changes.sh in test mode. Exits 1 naming each
# read that marks too little.
set -euo pipefail
command -v python3 >/dev/null || { echo "reads: error: python3 is needed" >&2; exit 2; }
CHANGES="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/changes.sh"
TREE="$(cd "${1:-$(dirname "$CHANGES")/../..}" && pwd -P)"
BAD=0
# Every list below comes from git: a tree whose files git cannot list would pass unchecked.
if [[ "$(git -C "$TREE" rev-parse --show-toplevel 2>/dev/null)" != "$TREE" ]] \
    || ! git -C "$TREE" ls-files --error-unmatch -- Cargo.toml >/dev/null 2>&1; then
    echo "reads: error: $TREE is not the top of a git work tree that tracks a Cargo.toml" >&2
    exit 2
fi

report() { local m="$*"; echo "reads: ${m//[[:cntrl:]]/?}"; }

areas() {   # the areas a change to <path> marks, space-separated; "unknown" if no rule knows <path>
    local out
    out="$(printf '%s\n' "$1" | env -u GITHUB_OUTPUT -u CI_HEAD_MESSAGE CI_EVENT=push bash "$CHANGES" --paths-from - 2>&1)" || true
    if [[ $out == *"changes: everything: "* && $out != *"changes: everything: shared file "* ]]; then
        echo unknown
    else
        sed -n 's/=true$//p' <<<"$out" | grep -vx wide | tr '\n' ' ' || true
    fi
}

# <path> from the tree's root, with `..` resolved as written, not through symlinks.
norm() { realpath -m -s --relative-to="$TREE" "$TREE/$1"; }

check() {   # check <what> <reader path> <read path> file|dir|link
    # file: <read> must be a file (or symlink) git tracks; dir: a directory, and
    # every tracked file under it is checked; link: either.
    local have missing a f files=()
    if [[ "$3" == .. || "$3" == ../* ]]; then
        report "$1: $3 is outside the tree, so it cannot be checked"
        BAD=1 && return
    fi
    if [[ $4 == dir && $3 == . ]]; then   # the root package: its sources, not the whole tree
        mapfile -t -d '' files < <(git -C "$TREE" ls-files -z -- ':(literal)src/' ':(literal)build.rs' ':(literal)Cargo.toml')
    elif [[ $4 != dir ]] && git -C "$TREE" cat-file -e ":0:$3" 2>/dev/null; then
        files=("$3")
    elif [[ $4 != file ]]; then
        mapfile -t -d '' files < <(git -C "$TREE" ls-files -z -- ":(literal)$3/")
    fi
    if [[ ${#files[@]} -eq 0 ]]; then
        report "$1: $3 is not a $([[ $4 == dir ]] && echo directory || echo file) git tracks, so it cannot be checked"
        BAD=1 && return
    fi
    for f in "${files[@]}"; do
        have=" $(areas "$f")" missing=""
        if [[ $have == " unknown" ]]; then
            report "$1: no rule in scripts/ci/changes.sh knows $f"
            BAD=1 && continue
        fi
        for a in $(areas "$2"); do
            if [[ "$have" != *" $a "* ]]; then missing+=" $a"; fi
        done
        if [[ -n "$missing" ]]; then
            report "$1: a change to $f does not mark$missing, as a change to $2 does"
            BAD=1
        fi
    done
}

# Each manifest's reads, as NUL-separated <kind> <manifest> <value>: D a path
# dependency, W one inherited from the workspace (its path from the root), T a
# target's source, X a manifest that is not TOML or a path that is not a string.
mapfile -t -d '' MANIFESTS < <(git -C "$TREE" ls-files -z -- Cargo.toml '*/Cargo.toml')
READS="$(mktemp)"
trap 'rm -f "$READS"' EXIT
python3 -I - "$TREE" "${MANIFESTS[@]}" >"$READS" <<'PY'
import os, sys, tomllib
def emit(kind, m, value):
    if not isinstance(value, str) or "\0" in value:
        kind, value = "X", f"a path that is not a string: {value!r}"
    sys.stdout.write(f"{kind}\0{m}\0{value}\0")
def table(v):
    return v if isinstance(v, dict) else {}
docs = {}
for m in sys.argv[2:]:
    try:
        with open(os.path.join(sys.argv[1], m), "rb") as f:
            docs[m] = tomllib.load(f)
    except (OSError, ValueError) as e:
        emit("X", m, f"not valid TOML: {e}")
ws = table(table(docs.get("Cargo.toml", {}).get("workspace")).get("dependencies"))
kinds = ("dependencies", "dev-dependencies", "dev_dependencies", "build-dependencies", "build_dependencies")
for m, doc in docs.items():
    tables = [doc, *map(table, table(doc.get("target")).values())]
    deps = [table(t.get(k)) for t in tables for k in kinds] + list(map(table, table(doc.get("patch")).values()))
    for name, dep in ((n, table(d)) for t in deps for n, d in t.items()):
        if "path" in dep:
            emit("D", m, dep["path"])
        if dep.get("workspace") is True and "path" in table(ws.get(name)):
            emit("W", m, ws[name]["path"])
    for k in ("lib", "bin", "test", "example", "bench"):
        for t in doc[k] if isinstance(doc.get(k), list) else [doc.get(k)]:
            if "path" in table(t):
                emit("T", m, t["path"])
    if not isinstance(table(doc.get("package")).get("build", False), bool):
        emit("T", m, doc["package"]["build"])
PY
while IFS= read -r -d '' k && IFS= read -r -d '' m && IFS= read -r -d '' a; do
    dir="$(dirname "$m")/" && dir="${dir#./}"
    case "$k" in
        D) check "$m depends on $a" "${dir}src/lib.rs" "$(norm "$dir$a")" dir ;;
        W) check "$m depends on $a (workspace)" "${dir}src/lib.rs" "$(norm "$a")" dir ;;
        T) check "$m builds $a" "${dir}src/lib.rs" "$(norm "$dir$a")" file ;;
        *) report "$m: $a"; BAD=1 ;;
    esac
done <"$READS"

# A string literal without a backslash (an escape, or a Windows separator): its text in L.
lit() {
    local re='^(r(#*))?"([^"\\]*)"(#*)$'
    [[ "$1" =~ $re && "${BASH_REMATCH[2]}" == "${BASH_REMATCH[4]}" ]] && L="${BASH_REMATCH[3]}"
}
# The path a read's argument names, in P: a literal, or concat! of literals joined
# (K=file). For concat! whose literals are followed by something else (a macro's
# $stem), the joined literals (K=prefix). Fails when it cannot be checked, naming
# the argument in A.
path_of() {
    local t n=0 parts re='^concat *! *[[({] *(.*)$'
    A="$1" K=file
    if lit "$1"; then P="$L"; return 0; fi
    [[ "$1" =~ $re ]] || return 1
    P="" && IFS=, read -ra parts <<<"${BASH_REMATCH[1]}"
    for t in "${parts[@]}"; do
        t="${t# }" && t="${t% }" && n=$((n + 1)) && A="$t"
        if lit "$t"; then P+="$L"; continue; fi
        [[ $n -gt 1 && -n $t && $t != *[\"\\]* ]] || return 1
        K=prefix && break
    done
}

# Each Rust file's reads, from the file with its whitespace squeezed to single
# spaces, so a read can span lines. A file that #[path] or include! pulls in is
# Rust whatever its name, so it is read too (QUEUE holds <file> <directory its
# #[path] resolves from>: an include!d file's is its includer's).
lead='(include(_bytes|_str|_image)? *! *[[({]|# *\[ *path *=|cfg_attr *\([^]]*[(,] *path *=) *'
inline='(^|[^[:alnum:]_])mod +[^ ;]+([^ ;[:alnum:]_]| +[^ ;])'   # `mod x {`: rustc resolves #[path] in it from elsewhere
QUEUE=()
while IFS= read -r -d '' f; do QUEUE+=("$f" "$(dirname "$f")"); done < <(git -C "$TREE" ls-files -z -- '*.rs')
declare -A SEEN=()
for ((i = 0; i < ${#QUEUE[@]}; i += 2)); do
    f="${QUEUE[i]}" mdir="${QUEUE[i + 1]}"
    if [[ -n "${SEEN["$mdir/$f"]:-}" || $f == ../* || ! -f "$TREE/$f" ]]; then continue; fi
    SEEN["$mdir/$f"]=1
    while IFS=' ' read -r kind arg; do
        if ! path_of "$arg"; then
            report "$f: a read whose path is not a string literal without a backslash cannot be checked: $A"
            BAD=1 && continue
        fi
        base="$(dirname "$f")"
        if [[ $kind == path ]]; then
            base="$mdir"
            if tr -s '[:space:]' ' ' <"$TREE/$f" | grep -qE "$inline"; then
                report "$f: a #[path] in a file with an inline module (or the word mod before something other than ;) cannot be checked: $arg"
                BAD=1 && continue
            fi
        fi
        if [[ $K == prefix ]]; then
            if [[ $kind != data ]]; then
                report "$f: an include! or #[path] whose file is not a single literal path cannot be checked: $arg"
                BAD=1 && continue
            fi
            check "$f reads $P..." "$f" "$(norm "$(dirname "$base/${P}x")")" dir
            continue
        fi
        r="$(norm "$base/$P")"
        check "$f reads $P" "$f" "$r" file
        case "$kind" in
            path) QUEUE+=("$r" "$(dirname "$r")") ;;
            include) QUEUE+=("$r" "$mdir") ;;
        esac
    done < <(tr -s '[:space:]' ' ' <"$TREE/$f" | grep -oE "$lead(concat *! *[[({][^])}]*|[^] ,)}]*)" \
        | sed -E "s/^include(_bytes|_str|_image) *! *[[({] */data /; t; s/^include *! *[[({] */include /; t; s/^$lead/path /")
done

while IFS= read -r -d '' entry; do
    if [[ "$entry" == 120000\ * ]]; then
        l="${entry#*$'\t'}" sha="${entry#120000 }" && sha="${sha%% *}"
        t="$(git -C "$TREE" cat-file blob "$sha")"
        if [[ $t != /* ]]; then t="$TREE/$(dirname "$l")/$t"; fi
        check "the symlink $l" "$l" "$(realpath -m -s --relative-to="$TREE" "$t")" link
    fi
done < <(git -C "$TREE" ls-files -s -z)

if [[ $BAD -ne 0 ]]; then
    report "FAILED: add the read path to the reader's areas in scripts/ci/changes.sh (see scripts/ci/README.md)"
    exit 1
fi
report "every read marks its reader's areas"
