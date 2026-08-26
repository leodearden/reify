#!/usr/bin/env python3
"""Independent pairwise clearance sweep for printer_v01 rear routing v3.1.

v3 (2026-08-25, symmetry round): unit B side-swapped (mirror-twin balanced
config — FL_IN_B is now the INBOARD vertical), b2-in feeds from the right
RAIL. v3.1 (2026-08-26): ONE OUT scheme ×4 — every OUT feed turns straight
down at its own strand line. Rail strands (b1/a2) descend at ±rail_x into
their tangent planes; return strands (a1/b2) descend at ±436 hanging at the
CAP_Y mid-plane line, and their low runs SLANT in plan to the unchanged rise
bases (tilted-axis corner/up-turn discs). The v3 all-four-down-turns proof
assumed axis-aligned in-plane low runs (A3); the slant breaks A3 — this
sweep caught both bad all-in-plane drafts and stays the oracle for the
slant's crossings.

Mirrors the DERIVATIONS in printer.ri's DriveTendons (not its literals), so a
disagreement means the .ri and this script disagree about the tangency rules.
Solids: cylinders (rope segments r=3; pulley discs r=18 half-width 5 along
their axis; drum core/flanges) and boxes (collar plates/webs). Cylinder axes
are 'X'/'Y'/'Z' or an arbitrary unit-vector tuple (v3.1 slant hardware).
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
FRAME_HX, FRAME_HZ, WEB_HZ, SIDE_Y, PLATE_T, WEB_T = 84.0, 72.0, 56.0, 40.0, 8.0, 8.0

# derived stations
FL_IN_A, FL_OUT_A = CAP_X - FL_VERT, CAP_X + FL_VERT       # 32 / 168
FL_IN_B, FL_OUT_B = -CAP_X + FL_VERT, -CAP_X - FL_VERT     # -32 / -168 (v3 side-swap)
FLZ_IN1, FLZ_OUT1 = DROP + W1I, DROP + W1O
FLZ_IN2, FLZ_OUT2 = DROP + W2I, DROP + W2O
IN_A1_TOP, IN_B1_TOP = ZA - R_P, ZB - R_P
IN_A2_TOP, IN_B2_TOP = -ZA - R_P, -ZB - R_P
# v3.1: EVERY OUT feed descends at its own strand line; the return-strand
# discs sit on the DNT_Y line so their descents hang at CAP_Y (mid-plane)
OUT_A1_X = RET_R                  # +436, down-turn on the right return (slant feed)
OUT_B1_X = RAIL_X                 # +400, down-turn feed
OUT_A2_X = -RAIL_X                # -400, down-turn feed
OUT_B2_X = RET_L                  # -436, down-turn on the left return (slant feed)
DNT_Y = CAP_Y + R_P               # -406, return-strand down-turn disc line
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
def rope_seg(n, p, q):
    """Rope between two arbitrary 3D points (v3.1 slant runs)."""
    L = math.dist(p, q)
    ax = tuple((q[i] - p[i]) / L for i in range(3))
    cyl(n, tuple((p[i] + q[i]) / 2 for i in range(3)), ax, ROPE, L / 2, 'rope')

# ── rail / return feeds (rear portions) ──────────────────────────────────────
# The +z rail strands are modelled to -430 = rail default span (-400) PLUS the
# lead_bk_* 30mm leaf continuations (#6592: RailTendons span overrides are
# inert, so the .ri models the continuation as separate collinear segments —
# ONE physical rope, one solid here). The -z strands render the 400 default,
# overshooting their -382 wrap tangents by 18mm of fictitious rope that the
# pass-through tangency suppression absorbs.
rope_y('rail_r_bu', -430, 0, RAIL_X, ZB)      # -> b1-OUT down-turn (incl. lead_bk_b1)
rope_y('rail_r_bl', -400, 0, RAIL_X, -ZB)     # -> b2-IN rear turn (18mm overshoot)
rope_y('rail_l_au', -430, 0, -RAIL_X, ZA)     # -> a1-IN rear turn (incl. lead_bk_a1)
rope_y('rail_l_al', -400, 0, -RAIL_X, -ZA)    # -> a2-OUT down-turn (18mm overshoot)
rope_y('ret_r_u', -406, 400, RET_R, ZA)       # 806mm -> a1-OUT down-turn at DNT_Y (v3.1)
rope_y('ret_r_l', -382, 400, RET_R, -ZA)      # 782mm -> a2-IN rear turn (v3 trim)
rope_y('ret_l_u', -430, 400, RET_L, ZB)       # b1-IN rear turn
rope_y('ret_l_l', -406, 400, RET_L, -ZB)      # 806mm -> b2-OUT down-turn at DNT_Y (v3.1)

# ── rear discs (v3.1: IN turns vertical-axis on one side per plane, ALL OUT
# feeds X-axis down-turns on their own strand lines) ─────────────────────────
disc('corner_bl_u', (IN_L, MOVED_Y, ZA), 'Z')            # a1-IN rear turn
disc('backidler_l_u', (OUT_L, MOVED_Y, ZB), 'Z')         # b1-IN rear turn
disc('backidler_r_l', (OUT_R, -CORNER_Y, -ZA), 'Z')      # a2-IN rear turn
disc('corner_br_l', (IN_R, -CORNER_Y, -ZB), 'Z')         # b2-IN rear turn (v3; was b2-OUT emitter)
disc('dnturn_b1', (OUT_B1_X, MOVED_Y, IN_B1_TOP), 'X')   # b1-OUT down-turn (v2 corner_br_u)
disc('dnturn_a2', (OUT_A2_X, -CORNER_Y, IN_A2_TOP), 'X') # a2-OUT down-turn
disc('dnturn_a1', (OUT_A1_X, DNT_Y, IN_A1_TOP), 'X')     # a1-OUT down-turn (right return, v3.1)
disc('dnturn_b2', (OUT_B2_X, DNT_Y, IN_B2_TOP), 'X')     # b2-OUT down-turn (left return, v3.1)

# ── IN feeds ─────────────────────────────────────────────────────────────────
rope_x('run_in_a1', IN_L, FL_IN_A - R_P, PLANE_OUT, ZA)
rope_x('run_in_b1', OUT_L, FL_IN_B - R_P, PLANE_OUT, ZB)
rope_x('run_in_a2', FL_OUT_A + R_P, OUT_R, PLANE_IN, -ZA)
rope_x('run_in_b2', FL_OUT_B + R_P, IN_R, PLANE_IN, -ZB)
disc('idler_in_a1', (FL_IN_A - R_P, PLANE_OUT, IN_A1_TOP), 'Y')
disc('idler_in_b1', (FL_IN_B - R_P, PLANE_OUT, IN_B1_TOP), 'Y')
disc('idler_in_a2', (FL_OUT_A + R_P, PLANE_IN, IN_A2_TOP), 'Y')
disc('idler_in_b2', (FL_OUT_B + R_P, PLANE_IN, IN_B2_TOP), 'Y')
rope_z('vert_in_a1', FLZ_IN1, IN_A1_TOP, FL_IN_A, PLANE_OUT)
rope_z('vert_in_b1', FLZ_IN1, IN_B1_TOP, FL_IN_B, PLANE_OUT)
rope_z('vert_in_a2', FLZ_IN2, IN_A2_TOP, FL_OUT_A, PLANE_IN)
rope_z('vert_in_b2', FLZ_IN2, IN_B2_TOP, FL_OUT_B, PLANE_IN)

# ── OUT feeds (v3.1: all four down-turn; the discs live in the rear-disc
# block. Return feeds' descents hang at CAP_Y and their low runs SLANT in
# plan to the unchanged rise bases — tilted corner/up-turn discs.) ───────────
# Stagger (v3 levels kept): the slant feed (a1 / b2) takes the HIGH level and
# the in-plane feed (b1 / a2) the DEEP one; the in-plane feed's low run
# passes UNDER the slant feed's up-turn/rise at the rise station, while the
# slant run crosses the in-plane feed's DESCENT beside it in plan (~21mm).
def _plan_u(p1, p2):
    L = math.hypot(p2[0] - p1[0], p2[1] - p1[1])
    return ((p2[0] - p1[0]) / L, (p2[1] - p1[1]) / L)
A1P1, A1P2 = (OUT_A1_X, CAP_Y), (FL_OUT_A, PLANE_OUT)
B2P1, B2P2 = (OUT_B2_X, CAP_Y), (FL_IN_B, PLANE_IN)
A1U, B2U = _plan_u(A1P1, A1P2), _plan_u(B2P1, B2P2)
rope_z('desc_a1', LO_HI_C, IN_A1_TOP, OUT_A1_X, CAP_Y)
rope_z('desc_b1', LO_LO_C, IN_B1_TOP, OUT_B1_X, PLANE_OUT)
rope_z('desc_a2', LO_IN_LO_C, IN_A2_TOP, OUT_A2_X, PLANE_IN)
rope_z('desc_b2', LO_IN_HI_C, IN_B2_TOP, OUT_B2_X, CAP_Y)
disc('corner_lo_a1', (A1P1[0] + R_P*A1U[0], A1P1[1] + R_P*A1U[1], LO_HI_C), (-A1U[1], A1U[0], 0.0))
disc('corner_lo_b1', (OUT_B1_X - R_P, PLANE_OUT, LO_LO_C), 'Y')
disc('corner_lo_a2', (OUT_A2_X + R_P, PLANE_IN, LO_IN_LO_C), 'Y')
disc('corner_lo_b2', (B2P1[0] + R_P*B2U[0], B2P1[1] + R_P*B2U[1], LO_IN_HI_C), (-B2U[1], B2U[0], 0.0))
disc('idler_up_a1', (A1P2[0] - R_P*A1U[0], A1P2[1] - R_P*A1U[1], LO_HI_C), (-A1U[1], A1U[0], 0.0))
disc('idler_up_b1', (FL_OUT_B + R_P, PLANE_OUT, LO_LO_C), 'Y')
disc('idler_up_a2', (FL_IN_A - R_P, PLANE_IN, LO_IN_LO_C), 'Y')
disc('idler_up_b2', (B2P2[0] - R_P*B2U[0], B2P2[1] - R_P*B2U[1], LO_IN_HI_C), (-B2U[1], B2U[0], 0.0))
rope_seg('run_lo_a1', (A1P1[0] + R_P*A1U[0], A1P1[1] + R_P*A1U[1], LOW_HI),
                      (A1P2[0] - R_P*A1U[0], A1P2[1] - R_P*A1U[1], LOW_HI))
rope_x('run_lo_b1', FL_OUT_B + R_P, OUT_B1_X - R_P, PLANE_OUT, LOW_LO)
rope_x('run_lo_a2', OUT_A2_X + R_P, FL_IN_A - R_P, PLANE_IN, LOW_IN_LO)
rope_seg('run_lo_b2', (B2P1[0] + R_P*B2U[0], B2P1[1] + R_P*B2U[1], LOW_IN_HI),
                      (B2P2[0] - R_P*B2U[0], B2P2[1] - R_P*B2U[1], LOW_IN_HI))
rope_z('rise_a1', LO_HI_C, FLZ_OUT1, FL_OUT_A, PLANE_OUT)
rope_z('rise_b1', LO_LO_C, FLZ_OUT1, FL_OUT_B, PLANE_OUT)
rope_z('rise_a2', LO_IN_LO_C, FLZ_OUT2, FL_IN_A, PLANE_IN)
rope_z('rise_b2', LO_IN_HI_C, FLZ_OUT2, FL_IN_B, PLANE_IN)

# ── drive units (drums, fairlead pulleys, leads, collars) ────────────────────
# v3: unit B carries the mirror-twin balanced pulley arrangement (side −1):
# its in-pulleys sit on its local +x (inboard) instead of −x.
for tag, ux, sd in (('A', CAP_X, 1.0), ('B', -CAP_X, -1.0)):
    cyl(f'cap{tag}_core', (ux, CAP_Y, DROP), 'Z', PITCH_R, GL / 2, 'drum', f'drum{tag}')
    cyl(f'cap{tag}_flU', (ux, CAP_Y, DROP + GL / 2 + FLANGE_W / 2), 'Z', FLANGE_R, FLANGE_W / 2, 'drum', f'drum{tag}')
    cyl(f'cap{tag}_flL', (ux, CAP_Y, DROP - GL / 2 - FLANGE_W / 2), 'Z', FLANGE_R, FLANGE_W / 2, 'drum', f'drum{tag}')
    disc(f'fl{tag}_w1in', (ux - sd * PULLEY_X, PLANE_OUT, DROP + W1I), 'Y')
    disc(f'fl{tag}_w1out', (ux + sd * PULLEY_X, PLANE_OUT, DROP + W1O), 'Y')
    disc(f'fl{tag}_w2in', (ux + sd * PULLEY_X, PLANE_IN, DROP + W2I), 'Y')
    disc(f'fl{tag}_w2out', (ux - sd * PULLEY_X, PLANE_IN, DROP + W2O), 'Y')
    rope_x(f'lead{tag}_w1in', ux - sd * PULLEY_X, ux, PLANE_OUT, DROP + W1I - R_P)
    rope_x(f'lead{tag}_w1out', ux, ux + sd * PULLEY_X, PLANE_OUT, DROP + W1O + R_P)
    rope_x(f'lead{tag}_w2in', ux, ux + sd * PULLEY_X, PLANE_IN, DROP + W2I - R_P)
    rope_x(f'lead{tag}_w2out', ux - sd * PULLEY_X, ux, PLANE_IN, DROP + W2O + R_P)
    for sy, sn in ((SIDE_Y, 'pos'), (-SIDE_Y, 'neg')):
        box(f'fr{tag}_plate_{sn}', (ux - FRAME_HX, CAP_Y + sy - PLATE_T / 2, DROP - FRAME_HZ),
            (ux + FRAME_HX, CAP_Y + sy + PLATE_T / 2, DROP + FRAME_HZ), 'frame', f'collar{tag}')
    for wx, wn in ((FRAME_HX - WEB_T / 2, 'pos'), (-FRAME_HX + WEB_T / 2, 'neg')):
        box(f'fr{tag}_web_{wn}', (ux + wx - WEB_T / 2, CAP_Y - SIDE_Y, DROP - WEB_HZ),
            (ux + wx + WEB_T / 2, CAP_Y + SIDE_Y, DROP + WEB_HZ), 'frame', f'collar{tag}')

# ── geometry helpers ─────────────────────────────────────────────────────────
class _Axv(dict):
    """'X'/'Y'/'Z' → basis vector; an arbitrary unit-vector tuple passes
    through unchanged (v3.1 slant hardware). Keeps incidence.py's
    AXV[s['ax']] contract intact."""
    def __missing__(self, k):
        return k
AXV = _Axv(X=(1, 0, 0), Y=(0, 1, 0), Z=(0, 0, 1))
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
        aA, aB = AXV[A['ax']], AXV[B['ax']]
        if abs(sum(aA[i] * aB[i] for i in range(3))) > 0.9999:
            # Parallel cylinders: exact face/rim separation. The generic
            # capsule bound below wildly over-flags two thin coaxial-offset
            # discs (their capsules nearly touch while the real rims are far
            # apart in the axial direction). Vector form so tuple axes
            # (v3.1) get the same treatment as the letter axes.
            dc = [B['c'][i] - A['c'][i] for i in range(3)]
            ad = sum(dc[i] * aA[i] for i in range(3))
            gap = abs(ad) - A['hl'] - B['hl']
            d = math.dist([0, 0, 0], [dc[i] - aA[i] * ad for i in range(3)])
            rad = d - A['r'] - B['r']
            if gap <= 0:
                return rad
            if rad <= 0:
                return gap
            return math.hypot(gap, rad)
        p1, q1 = seg(A); p2, q2 = seg(B)
        return seg_seg(p1, q1, p2, q2) - A['r'] - B['r']
    if A['k'] == 'b' and B['k'] == 'b':
        return max(max(A['lo'][i] - B['hi'][i], B['lo'][i] - A['hi'][i]) for i in range(3))
    bx, cy = (A, B) if A['k'] == 'b' else (B, A)
    ax = AXV[cy['ax']]
    # rim basis seed: least-aligned axis vector, so a tuple axis near ±X
    # (or any basis direction) never collapses the Gram-Schmidt step
    u = min(((1, 0, 0), (0, 1, 0), (0, 0, 1)),
            key=lambda b: abs(sum(b[i] * ax[i] for i in range(3))))
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
print(f"  verticals: A in {FL_IN_A:.0f} out {FL_OUT_A:.0f} | B in {FL_IN_B:.0f} out {FL_OUT_B:.0f}   (v3 side-swap: station set mirror-symmetric)")
print(f"  descents:  a1 {OUT_A1_X:.0f}@{CAP_Y:.0f}  b1 {OUT_B1_X:.0f}  a2 {OUT_A2_X:.0f}  b2 {OUT_B2_X:.0f}@{CAP_Y:.0f}   (v3.1: a1/b2 slant runs {math.degrees(math.atan2(abs(CAP_Y-PLANE_OUT), abs(A1P2[0]-A1P1[0]))):.1f}deg/{math.degrees(math.atan2(abs(PLANE_IN-CAP_Y), abs(B2P2[0]-B2P1[0]))):.1f}deg)")
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
