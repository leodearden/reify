# Count tendon segments incident on each pulley: every pulley turns a rope,
# so exactly TWO segments (one in, one out) must be tangent to it.
import importlib.util, math, sys
spec = importlib.util.spec_from_file_location("v2", f"{sys.argv[1]}/v2_check.py")
v2 = importlib.util.module_from_spec(spec)
import io, contextlib
with contextlib.redirect_stdout(io.StringIO()):
    spec.loader.exec_module(v2)
S, AXV, R_P = v2.S, v2.AXV, v2.R_P
seg = v2.seg

def touches(rope, d):
    """Is this rope tangent to this pulley (endpoint OR pass-through)?"""
    p, q = seg(rope); ax = AXV[d['ax']]
    for e in (p, q):                       # endpoint tangency
        dv = [e[i] - d['c'][i] for i in range(3)]
        axial = sum(dv[j]*ax[j] for j in range(3))
        perp = [dv[i] - ax[i]*axial for i in range(3)]
        if abs(math.dist([0,0,0], perp) - R_P) < 1.5 and abs(axial) < 1.5:
            return True
    rd = [q[i]-p[i] for i in range(3)]; rl = math.dist([0,0,0], rd)
    if rl < 1e-9: return False
    rd = [x/rl for x in rd]
    if abs(sum(rd[j]*ax[j] for j in range(3))) > 0.01:   # rope must be perp to spin axis
        return False
    pv = [p[i]-d['c'][i] for i in range(3)]
    axial = abs(sum(pv[j]*ax[j] for j in range(3)))
    perp = [pv[i] - ax[i]*sum(pv[j]*ax[j] for j in range(3)) for i in range(3)]
    proj = sum(perp[j]*rd[j] for j in range(3))
    off = math.dist([0,0,0], [perp[i]-rd[i]*proj for i in range(3)])
    t = -proj            # parameter of the closest point measured FROM p
    # tangent line, and the tangent point must lie WITHIN the rope's extent
    return axial < 1.5 and abs(off - R_P) < 1.5 and -1.5 <= t <= rl + 1.5

discs = [s for s in S if s['kind'] == 'disc']
ropes = [s for s in S if s['kind'] == 'rope']
print(f"pulleys: {len(discs)}   rope segments: {len(ropes)}\n")
bad = []
for d in discs:
    inc = [r['n'] for r in ropes if touches(r, d)]
    if len(inc) != 2:
        bad.append((d['n'], inc))
if not bad:
    print("EVERY pulley has exactly 2 incident tendon segments.")
else:
    print(f"PULLEYS WITHOUT EXACTLY 2 SEGMENTS: {len(bad)} of {len(discs)}\n")
    for n, inc in sorted(bad, key=lambda t: len(t[1])):
        print(f"  {n:18s} {len(inc)} -> {inc}")
# also: rope segments touching no pulley at either end
print()
for r in ropes:
    hits = [d['n'] for d in discs if touches(r, d)]
    if len(hits) < 2:
        print(f"  segment {r['n']:16s} touches {len(hits)} pulley(s): {hits}")
