#!/usr/bin/env python3
"""Sum the tokens of a workflow's agents from their transcripts. usage: tokens.py <journal.jsonl> [since HH:MM]"""
import json, sys, os, glob
J=sys.argv[1]; d=os.path.dirname(J)
rows=[json.loads(l) for l in open(J) if l.strip()]
lab={r['agentId']:r['label'] for r in rows if r['type']=='started'}
tot={'in':0,'cw':0,'cr':0,'out':0}
per=[]
for f in sorted(glob.glob(d+'/agent-*.jsonl'), key=os.path.getmtime):
    aid=os.path.basename(f)[6:-6]
    t={'in':0,'cw':0,'cr':0,'out':0}; seen=set()
    for l in open(f):
        try: r=json.loads(l)
        except Exception: continue
        m=r.get('message') or {}
        u=m.get('usage')
        if not u or r.get('type')!='assistant': continue
        mid=m.get('id')
        if mid in seen: continue
        seen.add(mid)
        t['in']+=u.get('input_tokens',0); t['cw']+=u.get('cache_creation_input_tokens',0)
        t['cr']+=u.get('cache_read_input_tokens',0); t['out']+=u.get('output_tokens',0)
    per.append((lab.get(aid,aid),t))
    for k in tot: tot[k]+=t[k]
for l,t in per:
    print(f"{l:38s} in {t['in']:>8,} cw {t['cw']:>9,} cr {t['cr']:>11,} out {t['out']:>8,}  in+cw+out {t['in']+t['cw']+t['out']:>9,}")
print('TOTAL', {k:f'{v:,}' for k,v in tot.items()}, 'in+cw+out', f"{tot['in']+tot['cw']+tot['out']:,}", 'all', f"{sum(tot.values()):,}")
