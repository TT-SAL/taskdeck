#!/usr/bin/env bash
# The spec lint (SYNC_PLAN.md §3.3, §6.1): checks docs/sync/spec/'s six specs
# and its README against the README's own rules, one line per problem.
#
#   bash scripts/spec-lint.sh [--summary] [--allow-empty-map]
#        [--spec-dir <dir>] [--sync <SYNC.md>] [--plan <SYNC_PLAN.md>]
#
# Exit 0 when clean, 1 with problems, 2 on a usage error.
#
# What it checks (README §3, §4, §6; the code is in spec-lint.py and
# speclib.py, which read the clause regular expression, the allowed owner
# names and the section titles from the README itself):
#   - every clause line matches the README's regular expression; a list item
#     that opens with a bold id but does not match is an error; ids are unique
#     across the six specs; each id's prefix names its file; every owner is an
#     allowed stage name, or the single word `withdrawn`, with no name twice;
#   - no clause-shaped line sits in a code block, a table or a quotation, and
#     the README holds none outside its code blocks;
#   - every uppercase MUST (and so MUST NOT) outside code blocks stands in a
#     clause: on its line or an indented continuation line of the same list
#     item. Headings, tables and prose are outside clauses;
#   - each spec starts with a level-one title, its level-two headings are
#     `## <n>. <Title>` numbered 1..n, it opens with the README's opening
#     sections and ends with its closing sections, in order, titles exact;
#   - every row of an "Error catalogue" table has the code `<spec>/<name>`,
#     <spec> being the file's own name, and the header the README names; no
#     code appears twice;
#   - every clause id mentioned anywhere in the specs (code blocks included)
#     or in the requirement map exists, and is well formed; a withdrawn id is
#     mentioned only inside a withdrawn clause or on a line that calls it
#     withdrawn;
#   - the requirement map between the README's marker comments is one table
#     `| SYNC.md ref | Requirement | Maps to |`: each row's ref is a section
#     of SYNC.md §3–§9 that exists, rows are sorted by section, and "Maps to"
#     holds only live clause ids, `stage: <name>` of a stage of SYNC_PLAN.md,
#     or `plan §2: <topic>` naming a row of SYNC_PLAN.md §2, at least one of
#     them; every section of SYNC.md §3–§9 without subsections has a row.
#     An empty map is an error unless --allow-empty-map is given.
#
# --summary prints clause counts per spec and per owner stage first.
set -uo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
command -v python3 >/dev/null || { echo "spec-lint: error: python3 is needed" >&2; exit 2; }
exec python3 -I -B "$here/spec-lint.py" "$@"
