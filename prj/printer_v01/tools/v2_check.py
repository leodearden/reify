#!/usr/bin/env python3
"""Independent pairwise clearance sweep for printer_v01 rear routing v2.

Mirrors the DERIVATIONS in printer.ri's DriveTendons (not its literals), so a
disagreement means the .ri and this script disagree about the tangency rules.
Solids: cylinders (rope segments r=3; pulley discs r=18 half-width 5 along
their axis; drum core/flanges) and boxes (collar plates/webs).
Clearance = axis-segment distance minus radii (a true lower bound for
parallel/skew cylinders; for perpendicular discs-vs-rope it is conservative).
"""
import math, itertools

# ── parameters (must match printer.ri) ───────────────────────────────────────
R_P, ROPE, PHW = 18.0, 3.0, 5.0      # r_pitch, rope radius, pulley half-width
CLEAR = 6.0
CAP_X, PULLEY_X = 100.0, 50.0
PITCH_R = 24.0                        # Capstan pitch_r (half-round seat)
FLANGE_R, FLANGE_W = 33.0, 6.0
GL = 7.0 * (1300.0 / (math.pi * 48.0) + 4.0)   # groove_len 88.346
WZ, LS = GL / 4, GL / 8
W1I, W1O = WZ + LS + R_P, WZ - LS - R_P
W2I, W2O = -WZ + LS + R_P, -WZ - LS - R_P
DROP = -220.0
RAIL_X, ZA, ZB = 400.0, 80.0, 55.0
CORNER_Y = 382.0
IN_R, IN_L = RAIL_X - R_P, -RAIL_X + R_P        # 382 / -382
OUT_R, OUT_L = RAIL_X + R_P, -RAIL_X - R_P      # 418 / -418
RET_R, RET_L = RAIL_X + 2 * R_P, -RAIL_X - 2 * R_P
PLANE_IN = -CORNER_Y - R_P                       # -400
PLANE_OUT = PLANE_IN - 2 * PITCH_R               # -448
MOVED_Y = PLANE_OUT + R_P                        # -430
CAP_Y = PLANE_IN - PITCH_R                       # -424
STAGGER = R_P*2 + ROPE + CLEAR + 5
LOW_HI = -352.0
LOW_LO = LOW_HI - STAGGER
LOW_IN_HI = -364.0
LOW_IN_LO = LOW_IN_HI - STAGGER
RISE_MIN = R_P*2 + CLEAR
FL_VERT = PULLEY_X + R_P                         # 68
TURN_GAP = R_P * 4 + CLEAR                       # 78
FRAME_HX, FRAME_HZ, WEB_HZ, SIDE_Y, PLATE_T, WEB_T = 84.0, 72.0, 56.0, 40.0, 8.0, 8.0

# derived stations
FL_IN_A, FL_OUT_A = CAP_X - FL_VERT, CAP_X + FL_VERT       # 32 / 168
FL_IN_B, FL_OUT_B = -CAP_X - FL_VERT, -CAP_X + FL_VERT     # -168 / -32
FLZ_IN1, FLZ_OUT1 = DROP + W1I, DROP + W1O
FLZ_IN2, FLZ_OUT2 = DROP + W2I, DROP + W2O
IN_A1_TOP, IN_B1_TOP = ZA - R_P, ZB - R_P
IN_A2_TOP, IN_B2_TOP = -ZA - R_P, -ZB - R_P
OUT_A1_X = FL_OUT_A + TURN_GAP
OUT_B1_X = RAIL_X
OUT_A2_X = FL_IN_B - TURN_GAP
OUT_B2_X = 0.0
LO_HI_C, LO_LO_C = LOW_HI + R_P, LOW_LO + R_P
LO_IN_HI_C, LO_IN_LO_C = LOW_IN_HI + R_P, LOW_IN_LO + R_P

S = []
def cyl(n, c, ax, r, hl, kind, part=None):
    S.append(dict(n=n, k='c', c=c, ax=ax, r=r, hl=hl, kind=kind, part=part or n))
def box(n, lo, hi, kind, part=None):
    S.append(dict(n=n, k='b', lo=lo, hi=hi, kind=kind, part=part or n))
def rope_x(n, x0, x1, y, z): cyl(n, ((x0+x1)/2, y, z), 'X', ROPE, abs(x1-x0)/2, 'rope')
def rope_y(n, y0, y1, x, z): cyl(n, (x, (y0+y1)/2, z), 'Y', ROPE, abs(y1-y0)/2, 'rope')
def rope_z(n, z0, z1, x, y): cyl(n, (x, y, (z0+z1)/2), 'Z', ROPE, abs(z1-z0)/2, 'rope')
def disc(n, c, ax): cyl(n, c, ax, R_P, PHW, 'disc')

# ── rail / return feeds (rear portions) ──────────────────────────────────────
rope_y('rail_r_bu', -430, 0, RAIL_X, ZB)      # span_bu 430 -> B w1-out feed
rope_y('rail_r_bl', -400, 0, RAIL_X, -ZB)     # B w2-out feed
rope_y('rail_l_au', -430, 0, -RAIL_X, ZA)     # A w1-in feed (flipped)
rope_y('rail_l_al', -400, 0, -RAIL_X, -ZA)    # A w2-out feed
rope_y('ret_r_u', -430, 400, RET_R, ZA)       # A w1-out feed
rope_y('ret_r_l', -400, 400, RET_R, -ZA)      # A w2-in feed
rope_y('ret_l_u', -430, 400, RET_L, ZB)       # B w1-in feed
rope_y('ret_l_l', -400, 400, RET_L, -ZB)      # B w2-in feed

# ── rear pulleys ─────────────────────────────────────────────────────────────
disc('corner_bl_u', (IN_L, MOVED_Y, ZA), 'Z')          # A w1-IN
disc('corner_bl_l', (IN_L, -CORNER_Y, -ZA), 'Z')       # A w2-OUT emitter
disc('corner_br_u', (RAIL_X, MOVED_Y, ZB - R_P), 'X')  # B w1-OUT down-turn
disc('corner_br_l', (IN_R, -CORNER_Y, -ZB), 'Z')       # B w2-OUT emitter
disc('backidler_r_u', (OUT_R, MOVED_Y, ZA), 'Z')       # A w1-OUT emitter
disc('backidler_r_l', (OUT_R, -CORNER_Y, -ZA), 'Z')    # A w2-IN
disc('backidler_l_u', (OUT_L, MOVED_Y, ZB), 'Z')       # B w1-IN
disc('backidler_l_l', (OUT_L, -CORNER_Y, -ZB), 'Z')    # B w2-IN

# ── IN feeds ─────────────────────────────────────────────────────────────────
rope_x('run_in_a1', IN_L, FL_IN_A - R_P, PLANE_OUT, ZA)
rope_x('run_in_b1', OUT_L, FL_IN_B - R_P, PLANE_OUT, ZB)
rope_x('run_in_a2', FL_OUT_A + R_P, OUT_R, PLANE_IN, -ZA)
rope_x('run_in_b2', OUT_L, FL_OUT_B - R_P, PLANE_IN, -ZB)
disc('idler_in_a1', (FL_IN_A - R_P, PLANE_OUT, IN_A1_TOP), 'Y')
disc('idler_in_b1', (FL_IN_B - R_P, PLANE_OUT, IN_B1_TOP), 'Y')
disc('idler_in_a2', (FL_OUT_A + R_P, PLANE_IN, IN_A2_TOP), 'Y')
disc('idler_in_b2', (FL_OUT_B - R_P, PLANE_IN, IN_B2_TOP), 'Y')
rope_z('vert_in_a1', FLZ_IN1, IN_A1_TOP, FL_IN_A, PLANE_OUT)
rope_z('vert_in_b1', FLZ_IN1, IN_B1_TOP, FL_IN_B, PLANE_OUT)
rope_z('vert_in_a2', FLZ_IN2, IN_A2_TOP, FL_OUT_A, PLANE_IN)
rope_z('vert_in_b2', FLZ_IN2, IN_B2_TOP, FL_OUT_B, PLANE_IN)

# ── OUT feeds ────────────────────────────────────────────────────────────────
rope_x('run_out_a1', OUT_A1_X + R_P, OUT_R, PLANE_OUT, ZA)
rope_x('run_out_a2', IN_L, OUT_A2_X - R_P, PLANE_IN, -ZA)
rope_x('run_out_b2', OUT_B2_X + R_P, IN_R, PLANE_IN, -ZB)
disc('idler_dn_a1', (OUT_A1_X + R_P, PLANE_OUT, IN_A1_TOP), 'Y')
disc('idler_dn_a2', (OUT_A2_X - R_P, PLANE_IN, IN_A2_TOP), 'Y')
disc('idler_dn_b2', (OUT_B2_X + R_P, PLANE_IN, IN_B2_TOP), 'Y')
rope_z('desc_a1', LO_HI_C, IN_A1_TOP, OUT_A1_X, PLANE_OUT)
rope_z('desc_b1', LO_LO_C, IN_B1_TOP, OUT_B1_X, PLANE_OUT)
rope_z('desc_a2', LO_IN_LO_C, IN_A2_TOP, OUT_A2_X, PLANE_IN)
rope_z('desc_b2', LO_IN_HI_C, IN_B2_TOP, OUT_B2_X, PLANE_IN)
disc('corner_lo_a1', (OUT_A1_X - R_P, PLANE_OUT, LO_HI_C), 'Y')
disc('corner_lo_b1', (OUT_B1_X - R_P, PLANE_OUT, LO_LO_C), 'Y')
disc('corner_lo_a2', (OUT_A2_X + R_P, PLANE_IN, LO_IN_LO_C), 'Y')
disc('corner_lo_b2', (OUT_B2_X - R_P, PLANE_IN, LO_IN_HI_C), 'Y')
disc('idler_up_a1', (FL_OUT_A + R_P, PLANE_OUT, LO_HI_C), 'Y')
disc('idler_up_b1', (FL_OUT_B + R_P, PLANE_OUT, LO_LO_C), 'Y')
disc('idler_up_a2', (FL_IN_A - R_P, PLANE_IN, LO_IN_LO_C), 'Y')
disc('idler_up_b2', (FL_IN_B + R_P, PLANE_IN, LO_IN_HI_C), 'Y')
rope_x('run_lo_a1', FL_OUT_A + R_P, OUT_A1_X - R_P, PLANE_OUT, LOW_HI)
rope_x('run_lo_b1', FL_OUT_B + R_P, OUT_B1_X - R_P, PLANE_OUT, LOW_LO)
rope_x('run_lo_a2', OUT_A2_X + R_P, FL_IN_A - R_P, PLANE_IN, LOW_IN_LO)
rope_x('run_lo_b2', FL_IN_B + R_P, OUT_B2_X - R_P, PLANE_IN, LOW_IN_HI)
rope_z('rise_a1', LO_HI_C, FLZ_OUT1, FL_OUT_A, PLANE_OUT)
rope_z('rise_b1', LO_LO_C, FLZ_OUT1, FL_OUT_B, PLANE_OUT)
rope_z('rise_a2', LO_IN_LO_C, FLZ_OUT2, FL_IN_A, PLANE_IN)
rope_z('rise_b2', LO_IN_HI_C, FLZ_OUT2, FL_IN_B, PLANE_IN)

# ── drive units (drums, fairlead pulleys, leads, collars) ────────────────────
for tag, ux in (('A', CAP_X), ('B', -CAP_X)):
    cyl(f'cap{tag}_core', (ux, CAP_Y, DROP), 'Z', PITCH_R, GL / 2, 'drum', f'drum{tag}')
    cyl(f'cap{tag}_flU', (ux, CAP_Y, DROP + GL / 2 + FLANGE_W / 2), 'Z', FLANGE_R, FLANGE_W / 2, 'drum', f'drum{tag}')
    cyl(f'cap{tag}_flL', (ux, CAP_Y, DROP - GL / 2 - FLANGE_W / 2), 'Z', FLANGE_R, FLANGE_W / 2, 'drum', f'drum{tag}')
    disc(f'fl{tag}_w1in', (ux - PULLEY_X, PLANE_OUT, DROP + W1I), 'Y')
    disc(f'fl{tag}_w1out', (ux + PULLEY_X, PLANE_OUT, DROP + W1O), 'Y')
    disc(f'fl{tag}_w2in', (ux + PULLEY_X, PLANE_IN, DROP + W2I), 'Y')
    disc(f'fl{tag}_w2out', (ux - PULLEY_X, PLANE_IN, DROP + W2O), 'Y')
    rope_x(f'lead{tag}_w1in', ux - PULLEY_X, ux, PLANE_OUT, DROP + W1I - R_P)
    rope_x(f'lead{tag}_w1out', ux, ux + PULLEY_X, PLANE_OUT, DROP + W1O + R_P)
    rope_x(f'lead{tag}_w2in', ux, ux + PULLEY_X, PLANE_IN, DROP + W2I - R_P)
    rope_x(f'lead{tag}_w2out', ux - PULLEY_X, ux, PLANE_IN, DROP + W2O + R_P)
    for sy, sn in ((SIDE_Y, 'pos'), (-SIDE_Y, 'neg')):
        box(f'fr{tag}_plate_{sn}', (ux - FRAME_HX, CAP_Y + sy - PLATE_T / 2, DROP - FRAME_HZ),
            (ux + FRAME_HX, CAP_Y + sy + PLATE_T / 2, DROP + FRAME_HZ), 'frame', f'collar{tag}')
    for wx, wn in ((FRAME_HX - WEB_T / 2, 'pos'), (-FRAME_HX + WEB_T / 2, 'neg')):
        box(f'fr{tag}_web_{wn}', (ux + wx - WEB_T / 2, CAP_Y - SIDE_Y, DROP - WEB_HZ),
            (ux + wx + WEB_T / 2, CAP_Y + SIDE_Y, DROP + WEB_HZ), 'frame', f'collar{tag}')

# ── geometry helpers ─────────────────────────────────────────────────────────
AXV = {'X': (1, 0, 0), 'Y': (0, 1, 0), 'Z': (0, 0, 1)}
def seg(s):
    c, a = s['c'], AXV[s['ax']]
    return tuple(c[i] - a[i] * s['hl'] for i in range(3)), tuple(c[i] + a[i] * s['hl'] for i in range(3))
def seg_seg(p1, q1, p2, q2):
    d1 = [q1[i] - p1[i] for i in range(3)]; d2 = [q2[i] - p2[i] for i in range(3)]
    r = [p1[i] - p2[i] for i in range(3)]
    a = sum(x * x for x in d1); e = sum(x * x for x in d2); f = sum(d2[i] * r[i] for i in range(3))
    c = sum(d1[i] * r[i] for i in range(3)); b = sum(d1[i] * d2[i] for i in range(3))
    den = a * e - b * b
    s_ = 0.0 if den < 1e-12 else min(1.0, max(0.0, (b * f - c * e) / den))
    t_ = (b * s_ + f) / e if e > 1e-12 else 0.0
    t_ = min(1.0, max(0.0, t_))
    s_ = 0.0 if a < 1e-12 else min(1.0, max(0.0, (b * t_ - c) / a))
    cp1 = [p1[i] + d1[i] * s_ for i in range(3)]; cp2 = [p2[i] + d2[i] * t_ for i in range(3)]
    return math.dist(cp1, cp2)
def box_pt(bx, p):
    return math.dist(p, [min(max(p[i], bx['lo'][i]), bx['hi'][i]) for i in range(3)])
def clearance(A, B):
    if A['k'] == 'c' and B['k'] == 'c':
        p1, q1 = seg(A); p2, q2 = seg(B)
        return seg_seg(p1, q1, p2, q2) - A['r'] - B['r']
    if A['k'] == 'b' and B['k'] == 'b':
        return max(max(A['lo'][i] - B['hi'][i], B['lo'][i] - A['hi'][i]) for i in range(3))
    bx, cy = (A, B) if A['k'] == 'b' else (B, A)
    ax = AXV[cy['ax']]
    u = (1,0,0) if cy['ax'] != 'X' else (0,1,0)
    u = [u[i] - ax[i]*sum(u[j]*ax[j] for j in range(3)) for i in range(3)]
    un = math.dist([0,0,0], u); u = [x/un for x in u]
    w = [ax[1]*u[2]-ax[2]*u[1], ax[2]*u[0]-ax[0]*u[2], ax[0]*u[1]-ax[1]*u[0]]
    best = 1e9
    for t in range(-1, 2):          # both faces + mid
        for k in range(72):         # rim samples
            th = 2*math.pi*k/72
            for rr in (cy['r'], cy['r']*0.5, 0.0) if cy['kind'] != 'rope' else (cy['r'],):
                pt = [cy['c'][i] + ax[i]*cy['hl']*t + (u[i]*math.cos(th)+w[i]*math.sin(th))*rr for i in range(3)]
                best = min(best, box_pt(bx, pt))
    return best

# ── by-design contacts to suppress ───────────────────────────────────────────
def suppressed(A, B):
    if A['part'] == B['part']:
        return 'same rigid part'
    ka, kb = A['kind'], B['kind']
    if {ka, kb} == {'rope', 'disc'}:
        rope, d = (A, B) if ka == 'rope' else (B, A)
        p, q = seg(rope)
        # rope tangent to this pulley's pitch circle: an endpoint lies r_pitch
        # from the disc centre in the disc's own plane
        for e in (p, q):
            dv = [e[i] - d['c'][i] for i in range(3)]
            ax = AXV[d['ax']]
            inplane = math.dist([0, 0, 0], [dv[i] - ax[i] * sum(dv[j] * ax[j] for j in range(3)) for i in range(3)])
            if abs(inplane - R_P) < 1.5 and abs(sum(dv[j] * ax[j] for j in range(3))) < 1.5:
                return 'pulley tangency'
        # pass-through tangency: the rope LINE runs tangent to the pitch circle
        ax = AXV[d['ax']]
        pv = [p[i] - d['c'][i] for i in range(3)]
        rd = [q[i] - p[i] for i in range(3)]
        rl = math.dist([0,0,0], rd)
        if rl > 1e-9:
            rd = [x/rl for x in rd]
            if abs(sum(rd[j]*ax[j] for j in range(3))) < 0.01:      # rope perp to disc axis
                axial = abs(sum(pv[j]*ax[j] for j in range(3)))
                perp = [pv[i] - ax[i]*sum(pv[j]*ax[j] for j in range(3)) for i in range(3)]
                along = sum(perp[j]*rd[j] for j in range(3))
                off = math.dist([0,0,0], [perp[i] - rd[i]*along for i in range(3)])
                if axial < 1.5 and abs(off - R_P) < 1.5:
                    return 'pulley pass-through tangency'
    if {ka, kb} == {'rope', 'drum'}:
        rope, dr = (A, B) if ka == 'rope' else (B, A)
        p, q = seg(rope)
        for e in (p, q):
            if abs(math.hypot(e[0] - dr['c'][0], e[1] - dr['c'][1]) - PITCH_R) < 1.5:
                return 'drum tangency'
    if ka == 'rope' and kb == 'rope':
        p1, q1 = seg(A); p2, q2 = seg(B)
        for e1 in (p1, q1):
            for e2 in (p2, q2):
                if math.dist(e1, e2) < 1.0:
                    return 'shared endpoint'
    return None

rows, supp, ok = [], [], 0
for A, B in itertools.combinations(S, 2):
    why = suppressed(A, B)
    d = clearance(A, B)
    if why:
        supp.append((A['n'], B['n'], d, why)); continue
    if d >= CLEAR: ok += 1
    else: rows.append((d, A['n'], B['n']))
rows.sort()

print(f"solids: {len(S)}   pairs: {len(S)*(len(S)-1)//2}")
print(f"\n=== stations (cap_x={CAP_X:.0f}) ===")
print(f"  verticals: A in {FL_IN_A:.0f} out {FL_OUT_A:.0f} | B in {FL_IN_B:.0f} out {FL_OUT_B:.0f}   mid corridor +/-{CAP_X-PULLEY_X-R_P-ROPE-CLEAR:.0f}")
print(f"  descents:  a1 {OUT_A1_X:.0f}  b1 {OUT_B1_X:.0f}  a2 {OUT_A2_X:.0f}  b2 {OUT_B2_X:.0f}")
print(f"  low runs:  hi {LOW_HI:.0f}  lo {LOW_LO:.0f}   fairlead z: {FLZ_IN1:.1f} {FLZ_OUT1:.1f} {FLZ_IN2:.1f} {FLZ_OUT2:.1f}")
print(f"\n=== findings < {CLEAR}mm ===")
if not rows:
    print("  NONE — every non-by-design pair clears >= 6mm")
for d, a, b in rows:
    v = 'INTERFERENCE' if d < 0 else ('CRITICAL' if d < 2 else 'TIGHT')
    print(f"  {d:8.2f}  {v:12s}  {a} vs {b}")
print(f"\nOK pairs (>= {CLEAR}mm): {ok}")
print(f"suppressed by-design: {len(supp)}")
worst = sorted(supp, key=lambda r: r[2])[:6]
for a, b, d, w in worst:
    print(f"    {d:8.2f}  {a} vs {b}  [{w}]")
