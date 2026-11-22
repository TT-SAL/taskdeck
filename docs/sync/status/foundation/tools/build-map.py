#!/usr/bin/env python3
"""Assemble the requirement map from the mappers' row files and the lead's extra rows, sort it by SYNC.md
section, check it, and write the table.  usage: build-map.py <out.md> <rows file>...   (run from the repo root)
A rows file is any text file; every line that starts with '| §' is a row."""
import re, sys
out, files = sys.argv[1], sys.argv[2:]
REF = re.compile(r'^(?:SYNC\.md\s+)?§(\d+(?:\.\d+)*)')
STAGES = {"foundation","core","seal","relay","engine","android-shell","device","recovery","desktop","android","release","rc","backlog"}
ids = set()
for sp in ['seal','core','relay','engine','device','bridge']:
    for l in open(f'docs/sync/spec/{sp}.md', encoding='utf-8'):
        m = re.match(r'^- \*\*([A-Z]+-[A-Z0-9]+-\d{3})\*\* \(([^)]*)\)', l)
        if m and m.group(2).strip() != 'withdrawn': ids.add(m.group(1))
plan = open('SYNC_PLAN.md', encoding='utf-8').read().split('\n')
i = next(k for k, l in enumerate(plan) if re.match(r'^## 2\. ', l))
topics = []
for l in plan[i+1:]:
    if l.startswith('## '): break
    if l.startswith('|') and not re.match(r'^\|[-| :]+\|$', l):
        c = [x.strip() for x in l.strip().strip('|').split('|')]
        if c and c[0] != 'Topic': topics.append(c[0])
sections = set()
for l in open('SYNC.md', encoding='utf-8'):
    m = re.match(r'^#{2,6} (\d+(?:\.\d+)*)\.?\s', l)
    if m: sections.add(tuple(int(x) for x in m.group(1).split('.')))
rows, problems = [], []
for f in files:
    for n, l in enumerate(open(f, encoding='utf-8'), 1):
        l = l.rstrip('\n')
        if not l.startswith('| §'): continue
        cells = [x.strip() for x in l.strip().strip('|').split('|')]
        if len(cells) != 3: problems.append(f'{f}:{n}: {len(cells)} cells'); continue
        m = REF.match(cells[0])
        sec = tuple(int(x) for x in m.group(1).split('.'))
        if not 3 <= sec[0] <= 9 or sec not in sections: problems.append(f'{f}:{n}: no SYNC.md section {cells[0]!r}')
        for part in re.split(r'\s*[,;]\s*', cells[2]):
            part = part.strip('` ')
            if re.match(r'^[A-Z]+-[A-Z0-9]+-\d{3}$', part):
                if part not in ids: problems.append(f'{f}:{n}: unknown or withdrawn clause {part}')
            elif part.startswith('stage:'):
                if part[6:].strip() not in STAGES: problems.append(f'{f}:{n}: unknown stage {part!r}')
            elif part.startswith('plan §2:'):
                if part[8:].strip().strip('`') not in [t.strip('`') for t in topics] and part[8:].strip() not in topics:
                    problems.append(f'{f}:{n}: no plan §2 row {part!r}')
            else: problems.append(f'{f}:{n}: cannot read {part!r}')
        rows.append((sec, len(rows), cells))
rows.sort(key=lambda r: (r[0], r[1]))
covered = {r[0] for r in rows}
for s in sorted(sections):
    if 3 <= s[0] <= 9 and not any(t != s and t[:len(s)] == s for t in sections) and s not in covered:
        problems.append(f'no row for SYNC.md §{".".join(map(str, s))}')
with open(out, 'w', encoding='utf-8') as o:
    o.write('| SYNC.md ref | Requirement | Maps to |\n|---|---|---|\n')
    for _, _, c in rows: o.write('| ' + ' | '.join(c) + ' |\n')
kinds = {'clause': 0, 'stage': 0, 'plan §2': 0, 'backlog': 0}
for _, _, c in rows:
    if re.search(r'[A-Z]+-[A-Z0-9]+-\d{3}', c[2]): kinds['clause'] += 1
    if 'stage:' in c[2]: kinds['stage'] += 1
    if 'plan §2:' in c[2]: kinds['plan §2'] += 1
    if 'stage: backlog' in c[2]: kinds['backlog'] += 1
print(len(rows), 'rows;', kinds, '; problems:', len(problems))
for p in problems[:40]: print('  ', p)
