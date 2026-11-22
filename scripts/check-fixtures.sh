#!/usr/bin/env bash
# The fixtures check (a CI check of the foundation area): regenerates
# fixtures/ into a temporary directory with scripts/regen-fixtures.sh and fails
# on any difference from the committed files — a file changed, missing, or
# present without the generator writing it. Also checks MANIFEST.sha256 with
# sha256sum where there is one.
#
#   bash scripts/check-fixtures.sh [<fixtures dir>]    default: this repository's fixtures/
#
# Exit 0 when they match, 1 when they do not (each difference is named), 2 when
# the regeneration itself failed.
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fixtures="$(cd "${1:-$here/../fixtures}" && pwd)" || { echo "check-fixtures: no fixtures directory" >&2; exit 2; }
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

if ! bash "$here/regen-fixtures.sh" "$tmp/fixtures"; then
    echo "check-fixtures: FAILED: the generator did not run to the end" >&2
    exit 2
fi

bad=0
# Every committed entry outside the generator crate, the README and
# .gitattributes must be a
# regular file (not a symlink, even to identical bytes) that the generator
# writes, and every file it writes must be committed, unchanged.
while IFS= read -r -d '' file; do
    rel="${file#"$fixtures"/}"
    case "$rel" in
        README.md|.gitattributes|generator/*) continue ;;
    esac
    if [[ -L $file || ! -f $file ]]; then
        echo "check-fixtures: fixtures/$rel is not a regular file (the generator writes only regular files)"
        bad=1
    elif [[ ! -e "$tmp/fixtures/$rel" ]]; then
        echo "check-fixtures: fixtures/$rel is not written by the generator"
        bad=1
    elif ! cmp -s "$file" "$tmp/fixtures/$rel"; then
        echo "check-fixtures: fixtures/$rel differs from what the generator writes"
        bad=1
    fi
done < <(find "$fixtures" -path "$fixtures/generator" -prune -o ! -type d -print0 | sort -z)
while IFS= read -r -d '' file; do
    rel="${file#"$tmp/fixtures"/}"
    if [[ ! -e "$fixtures/$rel" ]]; then
        echo "check-fixtures: fixtures/$rel is missing (the generator writes it)"
        bad=1
    fi
done < <(find "$tmp/fixtures" ! -type d -print0 | sort -z)

if command -v sha256sum >/dev/null && [[ -f "$fixtures/MANIFEST.sha256" ]]; then
    if ! (cd "$fixtures" && sha256sum -c --quiet MANIFEST.sha256); then
        echo "check-fixtures: MANIFEST.sha256 does not match the files"
        bad=1
    fi
fi

if [[ $bad -ne 0 ]]; then
    echo "check-fixtures: FAILED: run bash scripts/regen-fixtures.sh and review the difference"
    exit 1
fi
echo "check-fixtures: fixtures/ is exactly what the generator writes"
