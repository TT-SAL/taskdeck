#!/usr/bin/env bash
# Tests for scripts/spec-lint.sh, on small made-up spec trees (spec-fixture.sh).
set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
source "$TESTS_DIR/spec-fixture.sh"

LINT="$SCRIPTS_DIR/spec-lint.sh"
# lint_tree <tree> [options]
lint_tree() {
    local t="$1"
    shift
    bash "$LINT" --spec-dir "$t/docs/sync/spec" --sync "$t/SYNC.md" --plan "$t/SYNC_PLAN.md" "$@"
}
# fresh <name>: a new copy of the passing tree in $TMP/<name>; prints its path.
fresh() { make_tree "$TMP/$1"; echo "$TMP/$1"; }
S=docs/sync/spec

# --- the passing tree ---------------------------------------------------------
T="$(fresh base)"
expect "a well-formed tree passes with --allow-empty-map" 0 lint_tree "$T" --allow-empty-map
output_has "it counts the clauses" "spec-lint: ok (9 clauses in 6 specs"
expect "an empty map is an error without --allow-empty-map" 1 lint_tree "$T"
output_has "the empty map is named" "the requirement map is empty"
expect "--summary prints counts" 0 lint_tree "$T" --allow-empty-map --summary
output_has "per spec, live and withdrawn" "core.md        3    1"
output_has "per owner stage" "engine             2"

# --- clauses ------------------------------------------------------------------
T="$(fresh dup)"
echo "- **SEAL-FRAME-001** (seal) A frame MUST be signed." >>"$T/$S/seal.md"
expect "a duplicate id fails" 1 lint_tree "$T" --allow-empty-map
output_has "the duplicate is named with both places" "duplicate id SEAL-FRAME-001 (first at docs/sync/spec/seal.md:"

T="$(fresh owner)"
sed -i 's/(seal, relay)/(seal, hub)/' "$T/$S/seal.md"
expect "a bad owner fails" 1 lint_tree "$T" --allow-empty-map
output_has "the owner is named" "SEAL-FRAME-001: 'hub' is not an allowed owner stage"

T="$(fresh owner2)"
sed -i 's/(seal, relay)/(seal, seal)/' "$T/$S/seal.md"
expect "an owner named twice fails" 1 lint_tree "$T" --allow-empty-map
output_has "the repetition is named" "an owner is named twice"

T="$(fresh prefix)"
sed -i 's/SEAL-FRAME-001/CORE-FRAME-001/' "$T/$S/seal.md"
expect "an id in the wrong file fails" 1 lint_tree "$T" --allow-empty-map
output_has "the prefix is the problem" "CORE-FRAME-001 is in seal.md, but its prefix names another spec"

T="$(fresh malformed)"
echo "- **ENG-SEND-02** (engine) The engine MUST retry." >>"$T/$S/engine.md"
expect "a malformed clause line fails" 1 lint_tree "$T" --allow-empty-map
output_has "it is reported as not a clause line" "list item opens with a bold id but is not a well-formed clause line"

T="$(fresh code)"
printf '\n```text\n- **RELAY-AUTH-002** (relay) The relay MUST log nothing.\n```\n' >>"$T/$S/relay.md"
expect "a clause in a code block fails" 1 lint_tree "$T" --allow-empty-map
output_has "it is reported" "clause-shaped line inside a code block"

T="$(fresh table)"
printf '\n| **RELAY-AUTH-003** | the relay checks tokens |\n' >>"$T/$S/relay.md"
expect "a clause in a table fails" 1 lint_tree "$T" --allow-empty-map
output_has "it is reported" "clause-shaped text inside a table or quotation"

# README §4.1: a clause the reader does not see, a clause nested in a clause,
# a clause without MUST.
T="$(fresh comment)"
printf '\n<!--\n- **RELAY-AUTH-004** (relay) The relay MUST hide.\n-->\n' >>"$T/$S/relay.md"
expect "a clause in an HTML comment fails" 1 lint_tree "$T" --allow-empty-map
output_has "it is reported" "relay.md:42: clause or MUST inside an HTML comment"
T="$(fresh comment-inline)"
printf '\nText <!-- - **RELAY-AUTH-004** (relay) x MUST y. --> more.\n' >>"$T/$S/relay.md"
expect "a clause in a one-line HTML comment fails" 1 lint_tree "$T" --allow-empty-map
output_has "it is reported" "clause or MUST inside an HTML comment"
T="$(fresh comment-code)"
printf '\nThe page holds `<!--td-prefs-->` and `<!--` in inline code.\n' >>"$T/$S/relay.md"
expect "an HTML comment in inline code is text" 0 lint_tree "$T" --allow-empty-map
T="$(fresh nested)"
printf '\n- **RELAY-AUTH-004** (relay) x MUST y.\n  - **RELAY-AUTH-005** (relay) nested MUST z.\n' >>"$T/$S/relay.md"
expect "a clause nested in a clause fails" 1 lint_tree "$T" --allow-empty-map
output_has "it is reported" "RELAY-AUTH-005 is nested in the clause above"
T="$(fresh nomust)"
printf '\n- **RELAY-AUTH-004** (relay) The relay should do it\n  when it can.\n- **RELAY-AUTH-005** (relay) The relay\n  MUST do it.\n' >>"$T/$S/relay.md"
expect "a clause without MUST fails" 1 lint_tree "$T" --allow-empty-map
output_has "it is reported" "RELAY-AUTH-004: the clause has no MUST or MUST NOT"
output_lacks "a MUST on a continuation line counts" "RELAY-AUTH-005: the clause has no MUST"
output_lacks "a withdrawn clause needs none" "CORE-AB-009: the clause has no MUST"

# --- MUST outside a clause ----------------------------------------------------
T="$(fresh must)"
python3 -I - "$T/$S/device.md" <<'PY'
import sys
p = sys.argv[1]
s = open(p).read().replace("Made up for the tests.", "Made up for the tests. A device MUST NOT lie.")
open(p, "w").write(s)
PY
expect "a MUST outside a clause fails" 1 lint_tree "$T" --allow-empty-map
output_has "file and line are named" "docs/sync/spec/device.md:5: MUST outside a clause"
T="$(fresh mustcont)"
printf -- '- **ENG-SEND-002** (engine) The engine sends entries.\n  It MUST keep their order.\n' >>"$T/$S/engine.md"
sed -i 's/^## 4. Error catalogue/&/' "$T/$S/engine.md"
# The continuation line is inside the clause; move the clause into section 3.
python3 -I - "$T/$S/engine.md" <<'PY'
import sys
p = sys.argv[1]
lines = open(p).read().split("\n")
tail = lines[-3:-1]
lines = lines[:-3] + [""]
i = lines.index("## 4. Error catalogue")
lines[i:i] = tail + [""]
open(p, "w").write("\n".join(lines))
PY
expect "a MUST on an indented continuation line is inside its clause" 0 lint_tree "$T" --allow-empty-map
T="$(fresh mustafter)"
python3 -I - "$T/$S/engine.md" <<'PY'
import sys
p = sys.argv[1]
s = open(p).read().replace("The engine MUST send each entry once.\n", "The engine MUST send each entry once.\nThe relay MUST ack it.\n")
open(p, "w").write(s)
PY
expect "an unindented line after a clause is outside it" 1 lint_tree "$T" --allow-empty-map
output_has "the line is named" "MUST outside a clause: The relay MUST ack it."
T="$(fresh mustcode)"
printf '\n```text\nMUST in an example\n```\n' >>"$T/$S/engine.md"
expect "a MUST inside a code block is not a problem" 0 lint_tree "$T" --allow-empty-map

# Indented code blocks and the content column of a list item (CommonMark):
# after a blank line, four columns past the open item (or the margin) is code;
# less than the item's content column ends the item.
# edit <tree> <spec> <old> <new>: replaces the text once.
edit() {
    python3 -I - "$1/$S/$2.md" "$3" "$4" <<'PY'
import sys
p, old, new = sys.argv[1:]
s = open(p).read()
assert s.count(old) == 1, old
open(p, "w").write(s.replace(old, new))
PY
}
T="$(fresh icode)"
edit "$T" core $'\n## 4. Error catalogue' $'\nAn example of the format:\n\n    - **CORE-AB-005** (core) An example MUST NOT count.\n\n## 4. Error catalogue'
expect "a clause in an indented code block fails" 1 lint_tree "$T" --allow-empty-map
output_has "it is reported" "core.md:21: clause-shaped line inside an indented code block"
T="$(fresh icode-item)"
edit "$T" core $'MUST trim names.\n' $'MUST trim names.\n\n      - **CORE-AB-006** (core) An example MUST NOT count.\n'
expect "a clause in a list item's indented code block fails" 1 lint_tree "$T" --allow-empty-map
output_has "it is reported" "clause-shaped line inside an indented code block"
T="$(fresh unindented)"
edit "$T" core $'MUST trim names.\n' $'MUST trim names.\n\n Every reader MUST also do something unnumbered.\n'
expect "a paragraph after a blank line left of the clause's content is outside it" 1 lint_tree "$T" --allow-empty-map
output_has "the line is named" "MUST outside a clause: Every reader MUST also do something unnumbered."
T="$(fresh contpara)"
edit "$T" core $'MUST trim names.\n' $'MUST trim names.\n\n  Every reader MUST also do it.\n\n    | Case | MUST |\n    |---|---|\n'
expect "paragraphs at or past the clause's content column belong to it" 0 lint_tree "$T" --allow-empty-map
T="$(fresh itempara)"
edit "$T" device "Made up for the tests." $'- A note.\n\n    A device MUST hold.'
expect "a MUST in another list item's indented paragraph is outside a clause" 1 lint_tree "$T" --allow-empty-map
output_has "the line is named" "MUST outside a clause: A device MUST hold."

# --- references ---------------------------------------------------------------
T="$(fresh dangling)"
sed -i 's/Made up for the tests./Made up for the tests; see DEV-STORE-002./' "$T/$S/device.md"
expect "a dangling reference fails" 1 lint_tree "$T" --allow-empty-map
output_has "it is named" "dangling reference DEV-STORE-002 (no such clause)"

T="$(fresh withdrawn)"
sed -i 's/Made up for the tests./Made up for the tests; CORE-AB-009 applies./' "$T/$S/core.md"
expect "a withdrawn id referenced as live fails" 1 lint_tree "$T" --allow-empty-map
output_has "it is named" "refers to CORE-AB-009 as live, but it is withdrawn"
T="$(fresh withdrawn2)"
sed -i 's/Made up for the tests./Made up for the tests; CORE-AB-009 (withdrawn) is gone./' "$T/$S/core.md"
expect "a mention that calls it withdrawn is allowed" 0 lint_tree "$T" --allow-empty-map
T="$(fresh withdrawn3)"
sed -i 's/A frame MUST carry its length./A frame MUST carry its length; it replaces the withdrawn CORE-AB-009./' "$T/$S/seal.md"
expect "a live clause that calls the id withdrawn is allowed" 0 lint_tree "$T" --allow-empty-map
T="$(fresh withdrawn4)"
sed -i 's/A frame MUST carry its length./A frame MUST be checked as CORE-AB-009 says (not withdrawn here)./' "$T/$S/seal.md"
expect "the word withdrawn elsewhere on the line does not excuse a live reference" 1 lint_tree "$T" --allow-empty-map
output_has "it is named" "refers to CORE-AB-009 as live, but it is withdrawn"
T="$(fresh withdrawn5)"
sed -i 's/Made up for the tests./Made up for the tests; the withdrawn CORE-AB-009 is gone, yet CORE-AB-009 applies./' "$T/$S/core.md"
expect "only the mention that calls it withdrawn is excused" 1 lint_tree "$T" --allow-empty-map
output_has "it is named" "refers to CORE-AB-009 as live, but it is withdrawn"

T="$(fresh badid)"
sed -i 's/Made up for the tests./Made up for the tests; see ENG-SEND-1./' "$T/$S/engine.md"
expect "a malformed id in prose fails" 1 lint_tree "$T" --allow-empty-map
output_has "it is named" "malformed clause id 'ENG-SEND-1'"

# --- sections and the error catalogue -----------------------------------------
T="$(fresh order)"
sed -i 's/^## 5. Limits/## 5. Test vectors/; s/^## 6. Test vectors/## 6. Limits/' "$T/$S/bridge.md"
expect "closing sections out of order fail" 1 lint_tree "$T" --allow-empty-map
output_has "the order is named" "docs/sync/spec/bridge.md: the closing sections are"

T="$(fresh title)"
sed -i 's/^## 7. Interfaces/## 7. Interface/' "$T/$S/bridge.md"
expect "a closing title that is not exact fails" 1 lint_tree "$T" --allow-empty-map

T="$(fresh gap)"
sed -i 's/^## 9. Open questions/## 10. Open questions/' "$T/$S/bridge.md"
expect "a gap in the numbering fails" 1 lint_tree "$T" --allow-empty-map
output_has "it is named" "not numbered 1..n without gaps"

T="$(fresh code1)"
make_spec "$T/$S/relay.md" relay "- **RELAY-AUTH-001** (relay) The relay MUST refuse a request without a token." \
    '| `relay/Bad` | bad | always | give up |'
expect "an error code not of the form <spec>/<name> fails" 1 lint_tree "$T" --allow-empty-map
output_has "it is named" "error code '\`relay/Bad\`' is not of the form"
T="$(fresh code2)"
make_spec "$T/$S/relay.md" relay "- **RELAY-AUTH-001** (relay) The relay MUST refuse a request without a token." \
    '| `seal/bad` | bad | always | give up |'
expect "another spec's code in a catalogue fails" 1 lint_tree "$T" --allow-empty-map
T="$(fresh code3)"
make_spec "$T/$S/relay.md" relay "- **RELAY-AUTH-001** (relay) The relay MUST refuse a request without a token." \
    '| `relay/gone` | gone | always | give up |
| `relay/gone` | gone again | later | give up |'
expect "a code twice fails" 1 lint_tree "$T" --allow-empty-map
output_has "it is named" "error code relay/gone appears twice"

# --- the requirement map ------------------------------------------------------
T="$(fresh map)"
set_map "$T" "$GOOD_MAP"
expect "a well-formed map passes" 0 lint_tree "$T"
output_has "its rows are counted" "requirement map: 8 rows"

T="$(fresh map-dangling)"
set_map "$T" "${GOOD_MAP/ENG-SEND-001/ENG-SEND-007}"
expect "a map row with a dangling id fails" 1 lint_tree "$T"
output_has "it is named" "dangling reference ENG-SEND-007"
T="$(fresh map-withdrawn)"
set_map "$T" "${GOOD_MAP/CORE-AB-002/CORE-AB-009}"
expect "a map row with a withdrawn id fails" 1 lint_tree "$T"
output_has "it is named" "maps to CORE-AB-009, which is withdrawn"
T="$(fresh map-empty-target)"
set_map "$T" "${GOOD_MAP/stage: desktop/}"
expect "a map row that maps to nothing fails" 1 lint_tree "$T"
output_has "it is named" "the row maps to no clause id, stage or 'plan §2' row"
T="$(fresh map-stage)"
set_map "$T" "${GOOD_MAP/stage: desktop/stage: hub}"
expect "an unknown stage fails" 1 lint_tree "$T"
T="$(fresh map-backlog)"
set_map "$T" "${GOOD_MAP/| ENG-SEND-001 |/| stage: backlog |}"
expect "a map row deferred to the plan's backlog passes" 0 lint_tree "$T"
T="$(fresh owner-backlog)"
sed -i 's/(seal, relay)/(seal, backlog)/' "$T/$S/seal.md"
expect "a clause owned by the backlog fails" 1 lint_tree "$T" --allow-empty-map
output_has "it is named" "SEAL-FRAME-001: 'backlog' is not an allowed owner stage"
T="$(fresh map-plan)"
set_map "$T" "${GOOD_MAP/plan §2: Milestones/plan §2: Hubs}"
expect "an unknown plan §2 row fails" 1 lint_tree "$T"
output_has "it is named" "cannot read 'plan §2: Hubs'"
T="$(fresh map-sort)"
set_map "$T" "${GOOD_MAP/| §3 |/| §7 |}"
expect "an unsorted map fails" 1 lint_tree "$T"
output_has "it is named" "the map is not sorted by SYNC.md section"
T="$(fresh map-nosection)"
set_map "$T" "${GOOD_MAP/| §4.2 |/| §4.7 |}"
expect "a section SYNC.md lacks fails, and the uncovered one is named" 1 lint_tree "$T"
output_has "the missing section" "SYNC.md has no section §4.7"
output_has "the uncovered section" "no map row for SYNC.md §4.2"
for cell in "§4.1 see also §9.9" "§4.1.x" "§4.1, §4.2"; do
    T="$(fresh map-refcell)"
    set_map "$T" "${GOOD_MAP/| §4.1 |/| $cell |}"
    expect "a reference cell '$cell' fails" 1 lint_tree "$T"
    output_has "it is named" "'$cell' is not a SYNC.md section reference"
done
T="$(fresh map-refcell-sync)"
set_map "$T" "${GOOD_MAP/| §4.1 |/| SYNC.md §4.1 |}"
expect "a reference cell 'SYNC.md §4.1' passes" 0 lint_tree "$T"
T="$(fresh map-parent)"
set_map "$T" "$(grep -v '^| §4\.' <<<"$GOOD_MAP" | sed 's/^| §5 |/| §4 | the whole protocol | stage: engine |\n| §5 |/')"
expect "a row for §4 does not stand in for §4.1 and §4.2" 1 lint_tree "$T"
output_has "§4.1 is named" "no map row for SYNC.md §4.1"
output_has "§4.2 is named" "no map row for SYNC.md §4.2"
output_lacks "§4 itself has subsections and needs no row" "no map row for SYNC.md §4 "
T="$(fresh map-scope)"
set_map "$T" "$GOOD_MAP
| §10 | later | stage: rc |"
expect "a row outside §3–§9 fails" 1 lint_tree "$T"
T="$(fresh map-header)"
set_map "$T" "${GOOD_MAP/Maps to/Target}"
expect "a wrong header fails" 1 lint_tree "$T"
T="$(fresh map-text)"
set_map "$T" "$GOOD_MAP
some text"
expect "prose between the markers fails" 1 lint_tree "$T"

finish
