#!/usr/bin/env python3
"""Exit when the journal holds more finished writer agents (labels impl:, fix:, specfix:) than SEEN.
usage: waitwriter.py <journal> <seen> [prefixes, comma separated]"""
import json, sys, time, os
J, seen = sys.argv[1], int(sys.argv[2])
pre = tuple((sys.argv[3] if len(sys.argv) > 3 else 'impl:,fix:,specfix:').split(','))
while True:
    done = []
    if os.path.exists(J):
        rows = []
        for l in open(J):
            l = l.strip()
            if not l: continue
            try: rows.append(json.loads(l))
            except Exception: pass
        lab = {r['agentId']: r.get('label', '?') for r in rows if r.get('type') == 'started'}
        done = [lab.get(r['agentId'], '?') for r in rows if r.get('type') == 'result' and lab.get(r['agentId'], '?').startswith(pre)]
    if len(done) > seen:
        print('writers finished:', done[seen:], '| total', len(done)); sys.exit(0)
    time.sleep(20)
