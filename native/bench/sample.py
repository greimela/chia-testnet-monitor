import subprocess, sys, time, re, json, statistics as st

def procs():
    out = subprocess.run(["ps","-axo","pid=,ppid=,rss=,cputime=,command="],capture_output=True,text=True).stdout
    rows=[]
    for l in out.splitlines():
        m=re.match(r"\s*(\d+)\s+(\d+)\s+(\d+)\s+([\d:.]+)\s+(.*)",l)
        if m: rows.append((int(m[1]),int(m[2]),int(m[3]),m[4],m[5]))
    return rows

def cpu_secs(t):
    parts=t.split(":"); s=0.0
    for p in parts: s=s*60+float(p)
    return s

def footprint(pid):
    r=subprocess.run(["footprint","-p",str(pid),"--noCategories"],capture_output=True,text=True).stdout
    m=re.search(r"Footprint:\s*([\d.]+)\s*(KB|MB|GB)",r)
    if not m: m=re.search(r"\[\d+\]\s+\S.*?\s([\d.]+)\s*(KB|MB|GB)\s*$",r,re.M)
    if not m: return None
    v=float(m[1]); return v*{"KB":1/1024,"MB":1,"GB":1024}[m[2]]

def select(rows, needle):
    return [r for r in rows if needle in r[4] and "sample.py" not in r[4] and "pgrep" not in r[4]]

def run(targets, secs, every):
    # targets: {label: needle}
    series={k:[] for k in targets}
    t0=time.time(); first={}
    while time.time()-t0<secs:
        rows=procs(); now=time.time()
        for k,needle in targets.items():
            sel=select(rows,needle)
            rss=sum(r[2] for r in sel)/1024
            cpu=sum(cpu_secs(r[3]) for r in sel)
            fp=[footprint(r[0]) for r in sel]
            fp=sum(x for x in fp if x)
            series[k].append((now,rss,cpu,fp,len(sel)))
        time.sleep(every)
    res={}
    for k,s in series.items():
        wall=s[-1][0]-s[0][0]
        # CPU% from cumulative CPU time of processes alive throughout (tree is stable in steady state)
        cpu_pct=(s[-1][2]-s[0][2])/wall*100
        res[k]=dict(procs=s[-1][4],cpu_pct=round(cpu_pct,2),rss_mb_mean=round(st.mean(x[1] for x in s),1),rss_mb_max=round(max(x[1] for x in s),1),
                    footprint_mb_mean=round(st.mean(x[3] for x in s),1),footprint_mb_max=round(max(x[3] for x in s),1),samples=len(s),wall=round(wall,1))
    return res

if __name__=="__main__":
    secs=float(sys.argv[1]); every=float(sys.argv[2]); targets=json.loads(sys.argv[3])
    print(json.dumps(run(targets,secs,every),indent=1))
