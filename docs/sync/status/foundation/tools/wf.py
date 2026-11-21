#!/usr/bin/env python3
"""Workflow journal helper. Set WF_JOURNAL to the journal.jsonl of the running workflow.
  wf.py state            -> running and finished labels
  wf.py wait LABEL...    -> exit 0 once every LABEL has a result (prefix match), else 1
  wf.py report LABEL     -> print the result of the agent whose label starts with LABEL
"""
import json, os, sys
J=os.environ['WF_JOURNAL']  # <transcript dir>/journal.jsonl of the running workflow
rows=[json.loads(l) for l in open(J) if l.strip()]
lab={r['agentId']:r['label'] for r in rows if r['type']=='started'}
done={lab.get(r['agentId'],'?'):r.get('result') for r in rows if r['type']=='result'}
cmd=sys.argv[1] if len(sys.argv)>1 else 'state'
if cmd=='state':
    running=[l for a,l in lab.items() if l not in done]
    print('running :',running)
    print('finished:',list(done.keys()))
elif cmd=='wait':
    ok=all(any(d.startswith(w) for d in done) for w in sys.argv[2:])
    sys.exit(0 if ok else 1)
elif cmd=='report':
    for l,v in done.items():
        if l.startswith(sys.argv[2]):
            if isinstance(v,dict):
                for k,x in v.items():
                    if isinstance(x,list):
                        print(f'== {k}')
                        for i in x:
                            if isinstance(i,dict):
                                print('  -',' | '.join(f"{a}: {str(b)[:int(sys.argv[3]) if len(sys.argv)>3 else 400]}" for a,b in i.items()))
                            else: print('  -',str(i)[:int(sys.argv[3]) if len(sys.argv)>3 else 600])
                    else:
                        print(f'== {k}\n{x}')
            else: print(v)
