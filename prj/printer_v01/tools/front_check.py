# Incidence check for the FRONT/gantry zone (carriage idlers + front corners),
# which the rear-zone sweep does not cover.
import math
R_P, ROPE = 18.0, 3.0
RAIL_X, ZA, ZB = 400.0, 80.0, 55.0
IN_R, IN_L = RAIL_X - R_P, -RAIL_X + R_P
OUT_R, OUT_L = RAIL_X + R_P, -RAIL_X - R_P
RET_R, RET_L = RAIL_X + 2*R_P, -RAIL_X - 2*R_P
CORNER_Y = RAIL_X - R_P          # 382
SPLIT = R_P                       # ab_split
ropes, discs = [], []
def R(n, axis, fixed, lo, hi, z):
    ropes.append((n, axis, fixed, lo, hi, z))
def D(n, x, y, z, ax): discs.append((n, x, y, z, ax))
# rail half-spans (Y-runs at x=+/-400)
R('rail_r.a_upper','Y',RAIL_X,0,400,ZA);   R('rail_r.a_lower','Y',RAIL_X,0,400,-ZA)
R('rail_r.b_upper','Y',RAIL_X,-430,0,ZB);  R('rail_r.b_lower','Y',RAIL_X,-400,0,-ZB)
R('rail_l.a_upper','Y',-RAIL_X,-430,0,ZA); R('rail_l.a_lower','Y',-RAIL_X,-400,0,-ZA)
R('rail_l.b_upper','Y',-RAIL_X,0,400,ZB);  R('rail_l.b_lower','Y',-RAIL_X,0,400,-ZB)
# returns (Y-runs outboard)
R('return_r_u','Y',RET_R,-430,400,ZA);  R('return_r_l','Y',RET_R,-400,400,-ZA)
R('return_l_u','Y',RET_L,-430,400,ZB);  R('return_l_l','Y',RET_L,-400,400,-ZB)
# gantry crosses (X-runs at y=0)
R('cross.a_upper','X',0,-400,400,ZA);  R('cross.a_lower','X',0,-400,400,-ZA)
R('cross.b_upper','X',0,-400,400,ZB);  R('cross.b_lower','X',0,-400,400,-ZB)
# carriage idlers (vertical axis): right = A fore / B aft; left = converse
D('idlers_r.a_upper',IN_R, SPLIT, ZA,'Z');  D('idlers_r.a_lower',IN_R, SPLIT,-ZA,'Z')
D('idlers_r.b_upper',IN_R,-SPLIT, ZB,'Z');  D('idlers_r.b_lower',IN_R,-SPLIT,-ZB,'Z')
D('idlers_l.a_upper',IN_L,-SPLIT, ZA,'Z');  D('idlers_l.a_lower',IN_L,-SPLIT,-ZA,'Z')
D('idlers_l.b_upper',IN_L, SPLIT, ZB,'Z');  D('idlers_l.b_lower',IN_L, SPLIT,-ZB,'Z')
# front corners (vertical axis, outboard, U-turn onto the return)
D('corner_fr_u',OUT_R,CORNER_Y, ZA,'Z');  D('corner_fr_l',OUT_R,CORNER_Y,-ZA,'Z')
D('corner_fl_u',OUT_L,CORNER_Y, ZB,'Z');  D('corner_fl_l',OUT_L,CORNER_Y,-ZB,'Z')

def touches(r, d):
    n, axis, fixed, lo, hi, z = r
    dn, dx, dy, dz, dax = d
    if dax != 'Z' or abs(z - dz) > 1.5:      # vertical-axis pulley, rope in its plane
        return False
    if axis == 'Y':                           # rope runs along Y at x=fixed
        return abs(abs(fixed - dx) - R_P) < 1.5 and lo - 1.5 <= dy <= hi + 1.5
    else:                                     # rope runs along X at y=fixed
        return abs(abs(fixed - dy) - R_P) < 1.5 and lo - 1.5 <= dx <= hi + 1.5

bad = 0
for d in discs:
    inc = [r[0] for r in ropes if touches(r, d)]
    flag = '' if len(inc) == 2 else '   <-- NOT 2'
    if len(inc) != 2: bad += 1
    print(f"  {d[0]:18s} {len(inc)}  {inc}{flag}")
print(f"\nfront-zone pulleys: {len(discs)}   not-exactly-2: {bad}")
