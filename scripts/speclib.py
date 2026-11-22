"""Reads the six specs of docs/sync/spec/ and their README the way
scripts/spec-lint.sh and scripts/spec-coverage.sh need them. Not a script of
its own: both import it.

What it takes from the README (docs/sync/spec/README.md), so the tools follow
the README instead of a copy of it:
  - the clause regular expression (§4.1, the text block after "The regular
    expression a tool uses");
  - the allowed owner names (§4.1, the backticked names after "Allowed names:");
  - the opening and closing section titles (§6, the bold list items before and
    after the normative sections).
The six files and their prefixes are fixed by SYNC_PLAN.md §6.1 item 3 and the
README §4.1; they are written here.
"""
import os
import re

# (file stem, id prefix), in the README's order.
SPECS = [
    ("core", "CORE"),
    ("seal", "SEAL"),
    ("relay", "RELAY"),
    ("engine", "ENG"),
    ("device", "DEV"),
    ("bridge", "BRIDGE"),
]
PREFIXES = "|".join(p for _, p in SPECS)

# A well-formed id, as a whole token.
ID_TOKEN = re.compile(
    r"(?<![A-Za-z0-9_-])((?:%s)-[A-Z][A-Z0-9]{1,7}-[0-9]{3})(?![A-Za-z0-9_-])" % PREFIXES
)
# Anything that looks like it means to be an id: a spec prefix, then at least
# two more hyphen-separated parts. Used to report malformed ids.
LOOSE_TOKEN = re.compile(r"(?<![A-Za-z0-9_-])((?:%s)-[A-Za-z0-9_]+-[A-Za-z0-9_-]+)" % PREFIXES)
# A bold id anywhere on a line.
BOLD_ID = re.compile(r"\*\*(?:%s)-[A-Za-z0-9_]+-[A-Za-z0-9_-]+\*\*" % PREFIXES)
# A list item that opens with a bold span: the shape of a clause line.
BOLD_OPENER = re.compile(r"^\s*[-*]\s+\*\*([^*]+)\*\*")
MUST = re.compile(r"\bMUST\b")
FENCE = re.compile(r"^\s*(```+|~~~+)")
HEADING = re.compile(r"^(#{1,6})\s")
LEVEL2 = re.compile(r"^## (\d+)\. (.+?)\s*$")
TABLE = re.compile(r"^\s*\|")
QUOTE = re.compile(r"^\s*>")
# A list item's marker; its content starts after the spaces that follow it.
LIST_ITEM = re.compile(r"^( *)([-*+]|\d{1,9}[.)])( +|$)")

# Build output, never searched by spec-coverage or the gate scans: these names
# anywhere, and `build` only where Gradle writes it, next to a Gradle build or
# settings file (a source directory named build is searched).
OUTPUT_DIRS = {"target", "node_modules", ".git", ".gradle"}
GRADLE_FILES = ("build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts")


def is_output_dir(parent, name):
    return name in OUTPUT_DIRS or (
        name == "build" and any(os.path.isfile(os.path.join(parent, f)) for f in GRADLE_FILES))


def split_comments(line, in_comment):
    """(visible text, text inside HTML comments, whether a comment is still
    open at the end of the line). `<!--` inside inline code opens nothing."""
    vis, hid, i, code = [], [], 0, False
    while i < len(line):
        if in_comment:
            j = line.find("-->", i)
            if j < 0:
                hid.append(line[i:])
                break
            hid.append(line[i:j])
            i, in_comment = j + 3, False
        elif not code and line.startswith("<!--", i):
            i, in_comment = i + 4, True
        else:
            code ^= line[i] == "`"
            vis.append(line[i])
            i += 1
    return "".join(vis), "".join(hid), in_comment


MAP_BEGIN = "<!-- requirement-map:begin -->"
MAP_END = "<!-- requirement-map:end -->"


class Clause:
    def __init__(self, cid, owners, spec, path, line):
        self.id = cid
        self.owners = owners  # list of stage names; None when withdrawn
        self.spec = spec
        self.path = path
        self.line = line

    @property
    def withdrawn(self):
        return self.owners is None


class ReadmeRules:
    def __init__(self):
        self.clause_re = None
        self.owners = []
        self.opening = []
        self.closing = []
        self.problems = []


def read_lines(path):
    with open(path, encoding="utf-8") as f:
        return f.read().split("\n")


def readme_rules(readme_path, rel):
    """Reads the rules the README states. Problems name what it could not find."""
    r = ReadmeRules()
    try:
        lines = read_lines(readme_path)
    except OSError as e:
        r.problems.append(f"{rel}: cannot read: {e.strerror}")
        return r
    # The clause regular expression: the first non-blank line of the first
    # code block after the sentence that introduces it.
    intro = next((i for i, l in enumerate(lines) if "regular expression a tool uses" in l), None)
    if intro is not None:
        i = intro + 1
        while i < len(lines) and not FENCE.match(lines[i]):
            i += 1
        i += 1
        while i < len(lines) and not lines[i].strip():
            i += 1
        if i < len(lines) and not FENCE.match(lines[i]):
            try:
                r.clause_re = re.compile(lines[i].strip())
            except re.error as e:
                r.problems.append(f"{rel}:{i + 1}: the clause regular expression does not compile: {e}")
    if r.clause_re is None and not r.problems:
        r.problems.append(f"{rel}: no clause regular expression found (README §4.1)")
    # The allowed owner names.
    for i, l in enumerate(lines):
        if "Allowed names:" in l:
            tail = l.split("Allowed names:", 1)[1].split(".", 1)[0]
            r.owners = re.findall(r"`([a-z0-9-]+)`", tail)
            break
    if not r.owners:
        r.problems.append(f"{rel}: no 'Allowed names:' list of owner stages found (README §4.1)")
    # The section titles of README §6.
    start = next((i for i, l in enumerate(lines) if re.match(r"^## \d+\. Sections of every spec", l)), None)
    if start is None:
        r.problems.append(f"{rel}: no '## <n>. Sections of every spec' section found (README §6)")
        return r
    seen_normative = False
    for l in lines[start + 1:]:
        if l.startswith("## "):
            break
        m = re.match(r"^- \*\*(.+?)\.\*\*", l)
        if m:
            (r.closing if seen_normative else r.opening).append(m.group(1))
        elif re.match(r"^- ", l):
            seen_normative = True
    if not r.opening or not r.closing or not seen_normative:
        r.problems.append(f"{rel}: cannot read the opening and closing section titles (README §6)")
    return r


class SpecFile:
    def __init__(self, name, prefix, path, rel):
        self.name = name
        self.prefix = prefix
        self.path = path
        self.rel = rel
        self.clauses = []
        self.problems = []
        self.headings = []  # (number, title, line)
        self.refs = []  # (id, line, withdrawn_ok)
        self.codes = []  # (code, line)
        self.lines = []


def parse_spec(sf, rules):
    """Fills sf from its file: clauses, headings, references, error codes, and
    the problems found on the way."""
    try:
        sf.lines = read_lines(sf.path)
    except OSError as e:
        sf.problems.append(f"{sf.rel}: cannot read: {e.strerror}")
        return
    cre = rules.clause_re
    allowed = set(rules.owners)
    in_code = False
    in_comment = False
    fence = ""
    in_clause = False
    open_clause = None  # [id, where, has MUST] of the live clause being read

    def close_clause():
        nonlocal open_clause
        if open_clause and not open_clause[2]:
            sf.problems.append(f"{open_clause[1]}: {open_clause[0]}: the clause has no MUST or MUST NOT")
        open_clause = None

    clause_withdrawn = False
    clause_indent = 0
    clause_col = 0  # where the clause's list item puts its content
    items = []  # content columns of the open list items, innermost last
    prev_blank, in_icode = True, False
    section = None
    table_row = 0  # rows seen in the current table, 0 when not in a table
    first = next((l for l in sf.lines if l.strip()), "")
    if not first.startswith("# "):
        sf.problems.append(f"{sf.rel}:1: the spec does not start with a level-one title")
    for n, line in enumerate(sf.lines, 1):
        where = f"{sf.rel}:{n}"
        blank_before, prev_blank = prev_blank, not line.strip()
        fm = FENCE.match(line) if not in_comment else None
        if fm:
            mark = fm.group(1)
            if not in_code:
                in_code, fence = True, mark
            elif mark[0] == fence[0] and len(mark) >= len(fence) and line.strip() == mark:
                in_code = False
            table_row = 0
            continue
        if in_code:
            if (cre and cre.match(line)) or BOLD_ID.search(line):
                sf.problems.append(f"{where}: clause-shaped line inside a code block")
            _collect_refs(sf, line, n, withdrawn_ok=False, skip=None)
            continue

        # An indented code block (CommonMark §4.4): after a blank line, a line
        # indented four columns past the content of the open list item, or past
        # the margin when none is open. A list item ends at a line after a blank
        # that is indented less than its content.
        ex = line.expandtabs(4)
        indent = len(ex) - len(ex.lstrip())
        if line.strip() and not in_comment:
            if blank_before or in_icode:
                while items and indent < items[-1]:
                    items.pop()
                in_icode = indent >= (items[-1] if items else 0) + 4
                if in_icode:
                    if (cre and cre.match(line)) or BOLD_ID.search(line):
                        sf.problems.append(f"{where}: clause-shaped line inside an indented code block")
                    _collect_refs(sf, line, n, withdrawn_ok=False, skip=None)
                    continue
            li = LIST_ITEM.match(ex)
            if li:
                while items and indent < items[-1]:
                    items.pop()
                gap = len(li.group(3))
                items.append(li.end() if 1 <= gap <= 4 else indent + len(li.group(2)) + 1)

        # An HTML comment hides its text from the reader: no clause or MUST
        # may stand in one. The rest of the line is read as usual.
        line, hidden, in_comment = split_comments(line, in_comment)
        if hidden:
            if (cre and cre.match(hidden)) or BOLD_ID.search(hidden) or MUST.search(hidden):
                sf.problems.append(f"{where}: clause or MUST inside an HTML comment, which the reader does not see")
            _collect_refs(sf, hidden, n, withdrawn_ok=False, skip=None)
            if not line.strip():
                continue

        m = cre.match(line) if cre else None
        if HEADING.match(line):
            in_clause = False
            close_clause()
            h2 = LEVEL2.match(line)
            if line.startswith("## "):
                if h2:
                    section = h2.group(2)
                    sf.headings.append((int(h2.group(1)), section, n))
                else:
                    sf.problems.append(f"{where}: level-two heading is not of the form '## <n>. <Title>'")
        elif m:
            cid, owners = m.group(1), m.group(2)
            indent = len(line) - len(line.lstrip())
            if in_clause and indent > clause_indent:
                sf.problems.append(f"{where}: {cid} is nested in the clause above: nested items of a clause do not start with an id")
            close_clause()
            if TABLE.match(line) or QUOTE.match(line):
                sf.problems.append(f"{where}: clause-shaped line inside a table or quotation")
            if not cid.startswith(sf.prefix + "-"):
                sf.problems.append(f"{where}: {cid} is in {sf.name}.md, but its prefix names another spec")
            if owners == "withdrawn":
                sf.clauses.append(Clause(cid, None, sf.name, sf.rel, n))
                clause_withdrawn = True
            else:
                names = owners.split(", ")
                for o in names:
                    if o == "withdrawn":
                        sf.problems.append(f"{where}: {cid}: 'withdrawn' must stand alone in place of the owners")
                    elif o not in allowed:
                        sf.problems.append(f"{where}: {cid}: '{o}' is not an allowed owner stage")
                if len(set(names)) != len(names):
                    sf.problems.append(f"{where}: {cid}: an owner is named twice")
                sf.clauses.append(Clause(cid, names, sf.name, sf.rel, n))
                clause_withdrawn = False
                open_clause = [cid, where, bool(MUST.search(line))]
            in_clause, clause_indent = True, indent
            clause_col = items[-1] if items else indent + 2
            _collect_refs(sf, line, n, withdrawn_ok=clause_withdrawn, skip=cid)
            table_row = 0
            continue
        else:
            opener = BOLD_OPENER.match(line)
            if opener and re.match(r"(?:%s)-" % PREFIXES, opener.group(1)):
                sf.problems.append(f"{where}: list item opens with a bold id but is not a well-formed clause line: {line.strip()[:90]}")
            elif (TABLE.match(line) or QUOTE.match(line)) and BOLD_ID.search(line):
                sf.problems.append(f"{where}: clause-shaped text inside a table or quotation")
            if in_clause and line.strip():
                indent = len(line) - len(line.lstrip())
                # After a blank line the clause goes on only at its content column.
                if indent <= clause_indent or (blank_before and indent < clause_col):
                    in_clause = False
                    close_clause()
                elif open_clause and MUST.search(line):
                    open_clause[2] = True

        if MUST.search(line) and not in_clause:
            sf.problems.append(f"{where}: MUST outside a clause: {line.strip()[:100]}")
        _collect_refs(sf, line, n, withdrawn_ok=in_clause and clause_withdrawn, skip=None)

        # Error catalogue tables.
        if TABLE.match(line):
            table_row += 1
            if section == rules_error_title(rules):
                _catalogue_row(sf, line, n, table_row)
        else:
            table_row = 0
    close_clause()
    if in_comment:
        sf.problems.append(f"{sf.rel}: an HTML comment is not closed")
    if in_code:
        sf.problems.append(f"{sf.rel}: a code block is not closed")
    _check_sections(sf, rules)


def rules_error_title(rules):
    return next((t for t in rules.closing if t.lower().startswith("error catalogue")), "Error catalogue")


def _cells(line):
    s = line.strip()
    if s.startswith("|"):
        s = s[1:]
    if s.endswith("|"):
        s = s[:-1]
    return [c.strip() for c in s.split("|")]


def _catalogue_row(sf, line, n, row):
    where = f"{sf.rel}:{n}"
    cells = _cells(line)
    if row == 1:
        # README §6: "A table with columns: code, meaning, raised when, what
        # the caller does." A parenthesised remark in a header cell is allowed.
        want = ["code", "meaning", "raised when", "what the caller does"]
        if [re.sub(r"\s*\(.*?\)", "", c).lower() for c in cells] != want:
            sf.problems.append(f"{where}: error catalogue table header is not 'Code | Meaning | Raised when | What the caller does' (README §6)")
        return
    if row == 2:
        return  # the separator row
    code = cells[0]
    m = re.fullmatch(r"`?([^`\s]+)`?", code)
    if not m or not re.fullmatch(r"%s/[a-z][a-z0-9_]*" % re.escape(sf.name), m.group(1)):
        sf.problems.append(f"{where}: error code {code!r} is not of the form `{sf.name}/<name>` with <name> in [a-z][a-z0-9_]*")
        return
    sf.codes.append((m.group(1), n))


# A mention that itself calls the id withdrawn: "the withdrawn X-001",
# "X-001 (withdrawn)", "X-001 is withdrawn".
CALLED_WITHDRAWN_BEFORE = re.compile(r"\bwithdrawn\s+[`*]*\Z", re.I)
CALLED_WITHDRAWN_AFTER = re.compile(r"\A[`*]*\s*(?:\(\s*|(?:is|was)\s+)?withdrawn\b", re.I)


def _collect_refs(sf, line, n, withdrawn_ok, skip):
    """Records each id the line mentions; withdrawn_ok (the line is part of a
    withdrawn clause) or a mention that calls that id withdrawn lets it name a
    withdrawn id."""
    skipped = False
    for m in ID_TOKEN.finditer(line):
        cid = m.group(1)
        if cid == skip and not skipped:
            skipped = True
            continue
        called = CALLED_WITHDRAWN_BEFORE.search(line, 0, m.start(1)) or CALLED_WITHDRAWN_AFTER.match(line[m.end(1):])
        sf.refs.append((cid, n, withdrawn_ok or bool(called)))
    for m in LOOSE_TOKEN.finditer(line):
        tok = m.group(1).rstrip("-")
        if not ID_TOKEN.fullmatch(tok):
            sf.problems.append(f"{sf.rel}:{n}: malformed clause id {tok!r}")


def _check_sections(sf, rules):
    if not sf.headings:
        sf.problems.append(f"{sf.rel}: no level-two sections")
        return
    nums = [h[0] for h in sf.headings]
    if nums != list(range(1, len(nums) + 1)):
        sf.problems.append(f"{sf.rel}: level-two sections are not numbered 1..n without gaps: {nums}")
    titles = [h[1] for h in sf.headings]
    k, c = len(rules.opening), len(rules.closing)
    if not rules.opening or not rules.closing:
        return
    if titles[:k] != rules.opening:
        sf.problems.append(f"{sf.rel}: the first sections are {titles[:k]}, the README names {rules.opening}")
    if len(titles) < k + 1 + c:
        sf.problems.append(f"{sf.rel}: {len(titles)} sections; needs {k} opening, at least one normative and {c} closing")
    if titles[-c:] != rules.closing:
        sf.problems.append(f"{sf.rel}: the closing sections are {titles[-c:]}, the README names {rules.closing} in that order")


class Specs:
    def __init__(self):
        self.rules = None
        self.files = []
        self.clauses = {}  # id -> Clause (the first definition)
        self.problems = []


def load(spec_dir, rel_dir="docs/sync/spec"):
    """Parses the README and the six specs under spec_dir. Paths in problems
    are written as rel_dir/<file>."""
    s = Specs()
    s.rules = readme_rules(os.path.join(spec_dir, "README.md"), f"{rel_dir}/README.md")
    s.problems.extend(s.rules.problems)
    for name, prefix in SPECS:
        sf = SpecFile(name, prefix, os.path.join(spec_dir, name + ".md"), f"{rel_dir}/{name}.md")
        if s.rules.clause_re is not None:
            parse_spec(sf, s.rules)
        s.files.append(sf)
        for c in sf.clauses:
            if c.id in s.clauses:
                first = s.clauses[c.id]
                s.problems.append(f"{c.path}:{c.line}: duplicate id {c.id} (first at {first.path}:{first.line})")
            else:
                s.clauses[c.id] = c
    return s
