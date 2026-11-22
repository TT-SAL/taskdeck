#!/usr/bin/env python3
"""Write an index of every clause of the six specs: one line per clause under its section heading.
usage: clause-index.py <spec dir> <out file> [max chars per clause, default: whole clause]
The lead's helper for tasks that must find "which clause says this" without reading the specs in full."""
import re, sys
d, out = sys.argv[1], sys.argv[2]
cap = int(sys.argv[3]) if len(sys.argv) > 3 else None
C = re.compile(r'^- \*\*([A-Z]+-[A-Z0-9]+-\d{3})\*\* \(([^)]*)\) (.*)$')
res = []
n = 0
for sp in ['seal', 'core', 'relay', 'engine', 'device', 'bridge']:
    res.append(f'# {sp}.md')
    head = None; shown = None; cur = None; fence = False
    def flush():
        global cur, n, shown
        if cur:
            if head != shown:
                res.append(f'\n## {head}'); shown = head
            t = ' '.join(cur[2].split())
            if cap and len(t) > cap: t = t[:cap] + ' …'
            res.append(f'- {cur[0]} ({cur[1]}) {t}'); n += 1
        cur = None
    for l in open(f'{d}/{sp}.md', encoding='utf-8'):
        l = l.rstrip('\n')
        if l.startswith('```'): fence = not fence; flush(); continue
        if fence: continue
        if re.match(r'^#{2,4} ', l):
            flush(); head = l.lstrip('#').strip(); continue
        m = C.match(l)
        if m: flush(); cur = [m.group(1), m.group(2), m.group(3)]; continue
        if cur and l.startswith('  ') and l.strip(): cur[2] += ' ' + l.strip(); continue
        flush()
    flush()
    res.append('')
open(out, 'w', encoding='utf-8').write('\n'.join(res) + '\n')
print(n, 'clauses,', sum(len(x) + 1 for x in res), 'bytes')
