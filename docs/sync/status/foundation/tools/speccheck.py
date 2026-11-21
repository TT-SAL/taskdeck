#!/usr/bin/env python3
"""Lead-side sanity check of the six specs (not the stage's spec-lint; that is task 6).
Prints counts and problems: clause format, unique ids, prefix/file, owners, MUST outside clauses,
closing sections in order, dangling clause references, duplicate error codes."""
import re, sys, os, collections
D = sys.argv[1] if len(sys.argv) > 1 else '/home/user/taskdeck/docs/sync/spec'
SPECS = [('seal','SEAL'),('core','CORE'),('relay','RELAY'),('engine','ENG'),('device','DEV'),('bridge','BRIDGE')]
OWN = {'core','seal','relay','engine','device','recovery','desktop','android-shell','android','release','rc'}
CL = re.compile(r'^\s*[-*]\s+\*\*((?:CORE|SEAL|RELAY|ENG|DEV|BRIDGE)-[A-Z][A-Z0-9]{1,7}-[0-9]{3})\*\*\s+\(([a-z0-9, -]+)\)\s')
LOOSE = re.compile(r'\*\*((?:CORE|SEAL|RELAY|ENG|DEV|BRIDGE)-[A-Z][A-Z0-9]{1,7}-[0-9]{3})\*\*')
REF = re.compile(r'\b((?:CORE|SEAL|RELAY|ENG|DEV|BRIDGE)-[A-Z][A-Z0-9]{1,7}-[0-9]{3})\b')
CLOSING = ['Error catalogue','Limits','Test vectors','Interfaces','Public API','Open questions']
ids = {}; problems = []; counts = {}; owners = collections.Counter(); refs = []; withdrawn = set()
for name, pre in SPECS:
    lines = open(os.path.join(D, name + '.md'), encoding='utf-8').read().split('\n')
    incode = False; inclause = False; n = 0; heads = []
    for i, l in enumerate(lines, 1):
        if l.strip().startswith('```'): incode = not incode; inclause = False; continue
        m = CL.match(l)
        if incode:
            if m: problems.append(f'{name}.md:{i}: clause-shaped line inside a code block')
            continue
        h = re.match(r'^## (\d+)\. (.+)$', l)
        if h: heads.append((int(h.group(1)), h.group(2).strip())); inclause = False
        if m:
            cid, ow = m.group(1), m.group(2)
            n += 1; inclause = True
            if cid in ids: problems.append(f'{name}.md:{i}: duplicate id {cid} (first at {ids[cid]})')
            ids[cid] = f'{name}.md:{i}'
            if not cid.startswith(pre + '-'): problems.append(f'{name}.md:{i}: id {cid} in the wrong file')
            if ow.strip() == 'withdrawn': withdrawn.add(cid)
            else:
                for o in ow.split(', '):
                    if o not in OWN: problems.append(f'{name}.md:{i}: bad owner "{o}" in {cid}')
                    owners[o] += 1
                if not re.search(r'\bMUST\b', l) and not any(re.search(r'\bMUST\b', x) for x in lines[i:i+6]):
                    problems.append(f'{name}.md:{i}: clause {cid} has no MUST nearby')
        else:
            lm = LOOSE.search(l)
            if lm and re.match(r'^\s*[-*]\s+\*\*', l): problems.append(f'{name}.md:{i}: clause-like line does not match the format: {l[:90]}')
            if l.strip() == '' : pass
            elif not (l.startswith(' ') or l.startswith('\t')) : inclause = False
            if re.search(r'\bMUST( NOT)?\b', l) and not inclause and not l.lstrip().startswith('|') and not l.startswith('#'):
                problems.append(f'{name}.md:{i}: MUST outside a clause: {l.strip()[:100]}')
        for r in REF.findall(l): refs.append((r, f'{name}.md:{i}'))
    counts[name] = n
    nums = [a for a, _ in heads]
    if nums != list(range(1, len(nums) + 1)): problems.append(f'{name}.md: level-two sections are not numbered 1..n without gaps: {nums}')
    tail = [t for _, t in heads][-6:]
    if tail != CLOSING: problems.append(f'{name}.md: closing sections are {tail}')
    if heads[:2] and [t for _, t in heads[:2]] != ['Scope', 'Terms']: problems.append(f'{name}.md: first sections are {[t for _, t in heads[:2]]}')
for r, where in refs:
    if r not in ids: problems.append(f'{where}: dangling reference {r}')
print('clauses:', counts, 'total', sum(counts.values()), '| withdrawn', len(withdrawn))
print('owners :', dict(owners))
print('problems:', len(problems))
lim = int(sys.argv[2]) if len(sys.argv) > 2 else 60
for p in problems[:lim]: print('  ', p)
