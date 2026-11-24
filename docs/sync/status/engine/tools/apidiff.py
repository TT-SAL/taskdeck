import re,sys,glob
def strip(s):
    s=re.sub(r'//[^\n]*','',s)
    return s
def defs(text):
    out={}
    text=strip(text)
    for m in re.finditer(r'pub\s+(struct|enum)\s+(\w+)\s*(<[^>{]*>)?\s*\{',text):
        i=m.end(); d=1
        while d and i<len(text):
            c=text[i]
            if c=='{': d+=1
            elif c=='}': d-=1
            i+=1
        body=text[m.end():i-1]
        body=re.sub(r'#\[[^\]]*\]','',body)
        body=re.sub(r'\bpub\s+','',body)
        body=re.sub(r'\s+','',body).rstrip(',')
        # split top-level commas
        parts=[];cur='';dep=0
        for c in body:
            if c in '<({[': dep+=1
            if c in '>)}]': dep-=1
            if c==',' and dep==0: parts.append(cur);cur=''
            else: cur+=c
        if cur: parts.append(cur)
        parts=[p.rstrip(',').replace(',}','}').replace(',)',')') for p in parts]
        out[m.group(2)]=parts
    return out
spec=open('docs/sync/spec/engine.md').read()
blocks=re.findall(r'```rust\n(.*?)```',spec,re.S)
sd={}
for b in blocks:
    for k,v in defs(b).items():
        if k in sd and sd[k]!=v and v: pass
        if v or k not in sd: sd[k]=v
src=''
for f in ['lib','model','events','records','session','receive','snapshots','ops','ui']:
    src+=open(f'crates/taskdeck-sync/src/{f}.rs').read()
cd=defs(src)
for k in sorted(set(sd)|set(cd)):
    a=sd.get(k);b=cd.get(k)
    if a is None: print('ONLY CODE',k); continue
    if b is None: print('ONLY SPEC',k); continue
    if a!=b:
        for x in a:
            if x not in b: print(k,'spec has',x)
        for x in b:
            if x not in a: print(k,'code has',x)
        if set(a)==set(b): print(k,'ORDER differs')
