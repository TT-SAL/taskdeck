# A made-up spec tree for test-spec-lint.sh and test-spec-coverage.sh. Source
# it after lib.sh. It is not a test file itself (run.sh runs test-*.sh only).
#
#   make_tree <dir>   writes <dir>/docs/sync/spec/ (the real README, six small
#                     specs that pass the lint), <dir>/SYNC.md and <dir>/SYNC_PLAN.md
#   spec_body <name>  the normative section of one spec, from $BODY_<name> if set

make_spec() {  # make_spec <file> <name> <normative section text> [<catalogue rows>]
    local file="$1" name="$2" body="$3" rows="${4:-}"
    {
        echo "# The $name spec (made up)"
        echo
        echo "## 1. Scope"
        echo
        echo "Made up for the tests."
        echo
        echo "## 2. Terms"
        echo
        echo "None."
        echo
        echo "## 3. Rules"
        echo
        printf '%s\n' "$body"
        echo
        echo "## 4. Error catalogue"
        echo
        echo "| Code | Meaning | Raised when | What the caller does |"
        echo "|---|---|---|---|"
        if [[ -n "$rows" ]]; then printf '%s\n' "$rows"; else echo "| \`$name/bad_input\` | bad input | always | give up |"; fi
        echo
        echo "## 5. Limits"
        echo
        echo "None."
        echo
        echo "## 6. Test vectors"
        echo
        echo "None."
        echo
        echo "## 7. Interfaces"
        echo
        echo "None."
        echo
        echo "## 8. Public API"
        echo
        echo "None."
        echo
        echo "## 9. Open questions"
        echo
        echo "None."
    } >"$file"
}

make_tree() {
    local root="$1" d="$1/docs/sync/spec"
    mkdir -p "$d"
    cp "$REPO_DIR/docs/sync/spec/README.md" "$d/README.md"
    # The README's map, emptied whatever its state in the repository.
    python3 -I - "$d/README.md" <<'PY'
import sys
p = sys.argv[1]
s = open(p, encoding="utf-8").read()
b, e = "<!-- requirement-map:begin -->", "<!-- requirement-map:end -->"
i, j = s.index(b) + len(b), s.index(e)
open(p, "w", encoding="utf-8").write(s[:i] + "\n" + s[j:])
PY
    make_spec "$d/core.md" core "- **CORE-AB-001** (core) \`apply\` MUST refuse a bad command.
- **CORE-AB-002** (core, engine) \`apply\` MUST NOT change the board
  when it refuses: see CORE-AB-001.
- **CORE-ABC-001** (core) The compile step MUST trim names.
- **CORE-AB-009** (withdrawn) Replaced by CORE-AB-002."
    make_spec "$d/seal.md" seal "- **SEAL-FRAME-001** (seal, relay) A frame MUST carry its length."
    make_spec "$d/relay.md" relay "- **RELAY-AUTH-001** (relay) The relay MUST refuse a request without a token."
    make_spec "$d/engine.md" engine "- **ENG-SEND-001** (engine) The engine MUST send each entry once."
    make_spec "$d/device.md" device "- **DEV-STORE-001** (device, desktop) A device MUST fsync before it acknowledges."
    make_spec "$d/bridge.md" bridge "- **BRIDGE-API-001** (android-shell) The bridge MUST answer every verb."
    cat >"$root/SYNC.md" <<'EOF'
# Made-up design

## 1. Intro
## 2. Background
## 3. Feature parity
## 4. Sync protocol
### 4.1 Storage
### 4.2 Frames
## 5. Cryptography
## 6. Hosting
## 7. Security
## 8. Build
## 9. Migration
## 10. Later
EOF
    cat >"$root/SYNC_PLAN.md" <<'EOF'
# Made-up plan

## 1. For the owner

## 2. What changed since SYNC.md

| Topic | SYNC.md | This plan |
|---|---|---|
| Milestones | M1 first | **No M1.** |
| `.ics` feed and exports | Export | **Out of scope.** |

## 3. Rules
EOF
}

# set_map <tree> <table lines>: puts the lines between the README's markers.
set_map() {
    python3 -I - "$1/docs/sync/spec/README.md" "$2" <<'PY'
import sys
p, rows = sys.argv[1], sys.argv[2]
s = open(p, encoding="utf-8").read()
b, e = "<!-- requirement-map:begin -->", "<!-- requirement-map:end -->"
i, j = s.index(b) + len(b), s.index(e)
open(p, "w", encoding="utf-8").write(s[:i] + "\n" + rows + "\n" + s[j:])
PY
}

GOOD_MAP='| SYNC.md ref | Requirement | Maps to |
|---|---|---|
| §3 | the planner works as today | stage: desktop |
| §4.1 | frames are stored | SEAL-FRAME-001, RELAY-AUTH-001 |
| §4.2 | a refused command changes nothing | CORE-AB-002 |
| §5 | no M1 | plan §2: Milestones |
| §6 | no feed | plan §2: `.ics` feed and exports; stage: relay |
| §7 | stores are durable | DEV-STORE-001 |
| §8 | the bridge answers | BRIDGE-API-001 |
| SYNC.md §9 | entries are sent once | ENG-SEND-001 |'
