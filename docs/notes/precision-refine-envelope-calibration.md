# Precision refine-envelope calibration

Measurement note for **task 6166** (precision-nominal α). Calibrates the refine
envelope `achieved ≤ K · requested` per surface class, splits the per-pass cost into
tessellation and measurement, and chooses `REFINE_ATTEMPT_CAP`.

Extends — does not re-derive — §2, §2.1, §2.2 and §4.2 of
`docs/prds/v0_6/precision-nominal-representation-guarantee.md`.

Every number below is either **measured** (a run whose output is quoted) or **derived**
(arithmetic on measured numbers). The distinction is marked at each use and never
blurred.

---

## 0. Identity, and the standing caveats

| | |
|---|---|
| binary | `target/release/reify`, built 2026-08-10 08:48 |
| HEAD | `5db884e30b` (branch `task/6166`); last `crates/` commit `d542ae8027` |
| kernel | OCCT 7.8 (56 `libTK*.so.7.8` linked; `cfg(has_occt)` live) |
| machine | AMD Ryzen 9 3950X, 16C/32T, Linux 6.14.0-37 |
| load | **77 – 334** 1-min loadavg across the session |

**Caveat 1 — contention.** The box is shared with other warm lanes and ran at 2.4×–10×
oversubscription throughout. *Ratios are unaffected and exact* (§1); *wall clocks are
contended* and are upper bounds on quiet-machine time. Two runs that timed out at 90 s
had completed in 71 s and 51 s minutes earlier at lower load. Every timeout in this note
is a **budget wall, not a property of the geometry**.

**Caveat 2 — the `OK` trap.** A subject that fails to realize prints
`OK <checker>#constraint[0]` and **exits 0**. Against these fixtures' 1 µm bound any
genuinely measured curved surface *must* violate, so:

> `OK` never means "achieved ≈ 0". It means nothing was measured.

A run yields a datum **only** if it emits
`error: RepresentationWithin: sampled facet deviation <X> m exceeds bound 1.000e-6 m`.
Every number in §1 and §2 passed that gate. Anyone re-running this must apply it — the
most likely failure mode of this whole exercise is a table of confident near-zero ratios
that are silent non-realizations.

There is also a *third* outcome, distinct from both: `INDETERMINATE`, which is what a
construct that compiles but never realizes produces (a degenerate cone — §1.3).

Loft was listed in that parenthesis until task #6188 made it realizable; §1.6 records what it
measures now, and its same-plane form is a live instance of the `OK` trap above.

**Caveat 3 — sampled lower bound.** The reported deviation is a sampled lower bound on
the true Hausdorff chord error (4 interior points per facet), and per PRD §2.1 only the
**global max** is trustworthy. Nothing downstream may key on per-triangle values.

---

## 1. Achieved / requested, per class

Notation: `d` = requested `#precision`, `a` = achieved sampled facet deviation,
`K = sup(a/d)` over the regime walked. All rows **measured**.

### 1.1 Summary

| class | regime coordinate | sup K | at | status |
|---|---|---|---|---|
| loft | x-offset / `bottom_r`, `d` ‡ | **≥ 70.75** | `x/r` = 1.0, `d` = 0.06694 mm | **lower bound**, chaotic in `d`, budget-limited |
| sphere | `d/R` | **2.079** | `d/R` = 3.12e-4 | supremum |
| torus | `minor/major`, `d/minor` | 0.978 | 0.02, 0.015 | supremum |
| cone | `top_r/bottom_r` | 0.970 | 0.8, `d/R` = 6e-4 | supremum |
| fillet blend | `fillet_r/feature` | 0.925 | 0.49, `d/R` = 6e-4 | supremum |
| nurbs surface | `d/span` † | **1.0010** | `d/span` = 1.4386e-4 | **lower bound**, `K` > 1 |
| pipe | pipe_r / path curvature | 0.598 | `d/R` = 5e-2 | **lower bound** |
| sweep | profile / path curvature | 0.534 | `d/R` = 1e-2 | **lower bound** |
| spline | profile / path curvature | 0.013 | `d/R` = 2e-2 | **lower bound** |

† Two caveats, distinct from the other lower-bound rows. First, `d/span` reflects the
committed **1000 mm × 1000 mm control net** only (§1.5) — unlike cone/torus/fillet, whose
shape regime (`top/bottom`, `minor/major`, `r/feature`) was independently walked, this
task scoped a d-ladder only, and the net shape itself was not walked. Second, unlike
pipe/sweep/spline, this class is not budget-limited, and `lower bound` here carries a
weaker meaning than on any other row. The oscillation that made an earlier ladder's
`sup K = 0.996` wrong **has since been resolved** (§1.5, task #7128): it has no period —
local maxima recur at irregular spacing — and `a` is piecewise-constant on plateaus
~1e-4 mm wide, with the ratio peaking at each plateau's **lower edge**. Bisecting those
edges to 1e-5 mm pins **1.0010 at `d` = 0.14386 mm**, the one measurement in this note
where achieved *exceeds* requested; the true ratio there lies in [1.000626, 1.001321),
entirely above 1, so **`K` > 1 is established** for this class. What is still not proven
is the *value*: a dense search raises a lower bound and cannot prove a supremum over a
continuum, and only three plateau edges of the very many in [0.12, 0.18] mm were pinned
(P1–P3; a fourth plateau was walked as a control, with its lower edge left unbracketed).
So `lower bound` no longer means the structure is un-understood, and never meant a wall
was hit — it means 1.0010 is a floor that further walking can only raise.

‡ The worst class measured, and the least settled. The committed coaxial spelling is **not**
the subject of this row: it measures as a cone (byte-identical `a` to `cone(500mm, 250mm,
800mm)` at 19 of 19 rungs, K = 0.9210). The row is an **off-axis** loft — the top section's
centre offset in x — whose `a` is piecewise-constant in `d` with isolated spikes: 70.75 at
0.06694 mm sits beside ≈ 20 on both neighbouring plateaus, so no ladder samples it, and no
turnover is shown. The largest values sit at the fine end of the walked range, where each
rung costs minutes; one regime coordinate (the offset) was walked, at one top radius and one
height. `K` > 16 is established — the true ratio at the headline `d` lies in [70.742,
70.757) — but the value is a floor, and §3.1 consumes it as one. Full account: §1.6.

The deviation is **deterministic**: `torus(1000mm,100mm)` at `d`=10 mm returned
`5.665e-3` on three consecutive runs. The ratios carry no run-to-run error.

### 1.2 Sphere — the staircase, reproduced

`R` = 1000 mm. PRD §2's staircase reproduces exactly, including the tight period near
0.30 mm.

| d | a | a/d | branch |
|---|---|---|---|
| 100 mm | 6.006e-2 | 0.601 | floor |
| 50 mm | 6.006e-2 | 1.201 | floor |
| 20 mm | 2.258e-2 | 1.129 | |
| 10 mm | 7.482e-3 | 0.748 | tooth |
| 3 mm | 2.280e-3 | 0.760 | tooth |
| 1.5 mm | 1.141e-3 | 0.761 | tooth |
| 1.45 mm | 1.088e-3 | 0.750 | tooth |
| **1.4 mm** | 2.905e-3 | **2.075** | tread |
| 1.3 mm | 2.650e-3 | 2.038 | tread |
| 1.2 mm | 9.068e-4 | 0.756 | tooth |
| 1 mm | 2.058e-3 | 2.058 | tread |
| 0.8 mm | 1.644e-3 | 2.055 | tread |
| 0.6 mm | 4.541e-4 | 0.757 | tooth |
| 0.4 mm | 8.300e-4 | 2.075 | tread |
| 0.330 mm | 6.852e-4 | 2.076 | tread |
| 0.324 mm | 6.702e-4 | 2.069 | tread |
| 0.318 mm | 2.423e-4 | 0.762 | tooth |
| **0.312 mm** | 6.487e-4 | **2.079** | tread ← sup |
| 0.306 mm | 6.343e-4 | 2.073 | tread |
| 0.300 mm | 6.202e-4 | 2.067 | tread |

Two branches, ~0.75–0.76 and ~2.04–2.08, alternating with a period of **~0.006 mm**
near 0.30 mm — exactly as §2 reports. A coarse sweep lands on one branch and misses the
other; the 0.318 mm row sits between two ~2.07 rows.

### 1.3 Cone, torus, fillet blend — walking the regime, not sampling it

Spot values are actively misleading on these classes, so each was walked.

**Cone** `cone(bottom_r, top_r, height)`, bottom 500 mm, height 1000 mm, `d` = 10 mm:

| `top/bottom` | 1.000 | 0.998 | 0.980 | 0.900 | 0.800 | 0.500 | 0.200 | 0.040 | 0.000 |
|---|---|---|---|---|---|---|---|---|---|
| a/d | *indet.* | 0.960 | 0.956 | 0.935 | 0.908 | 0.816 | 0.714 | 0.658 | 0.644 |

`d`-walk at `top/bottom` = 0.8: 100 mm 0.135 (floor) · 20 mm 0.674 (floor) · 10 mm 0.908
· 5 mm 0.880 · 3 mm 0.940 · 1 mm 0.954 · 0.6 mm 0.947 · **0.3 mm 0.970**.

The ratio climbs monotonically toward the cylinder limit — but that limit is
*unreachable*: `cone(r, r, h)` is degenerate and yields `INDETERMINATE`, never a mesh.

**Torus** `torus(major_r, minor_r)`, major 1000 mm, `d` = 10 mm:

| `minor/major` | 0.90 | 0.50 | 0.20 | 0.10 | 0.05 | 0.02 | 0.012 |
|---|---|---|---|---|---|---|---|
| a/d | 0.972 | 0.845 | 0.624 | 0.567 | 0.508 | 0.501 | 0.490 |

`d/minor_r` knife edge at minor = 20 mm: `d`= 40 mm 0.187 (floor) · 20 mm 0.375 (floor)
· 10 mm 0.501 · 5 mm 0.522 · 2 mm 0.568 · 1 mm 0.635 · **0.3 mm 0.978**.

PRD §2's 0.982 spot value sits right at this supremum. A pre-session probe recording
0.2175 for `torus(1000mm,100mm)` @10 mm does **not** reproduce: the committed fixture
returns 0.567 there, deterministically. Recorded as measured.

**Fillet blend** `fillet(box(1000mm³), r)`, `d` = 10 mm:

| `r/feature` | 0.49 | 0.25 | 0.10 | 0.03 | 0.01 |
|---|---|---|---|---|---|
| a/d | 0.771 | 0.526 | 0.210 | 0.063 | 0.021 |

`d`-walk at `r/feature` = 0.49: 20 mm 0.515 · 10 mm 0.771 · 3 mm 0.829 · 1 mm 0.725 ·
**0.3 mm 0.925**. The ratio *falls* as the blend shrinks — the blend surface gets its own
local deflection budget rather than being starved by the planar bulk. This is the one
class where the intuition "small feature ⇒ worse ratio" is backwards.

### 1.4 The coarse floor — why a coarse-only sweep lies

Every class shows a **floor** at coarse `d`: achieved stops falling because the
tessellator has hit its minimum facet count. Sphere is pinned at `6.006e-2` for all
`d ≥ 50 mm`; cone is identical at 100 mm and 20 mm; torus identical at 40 mm and 20 mm;
nurbs_surface identical at 400 mm, 200 mm and 100 mm (§1.5), so its topmost mandated rung
sits inside the floor as well; loft's coaxial spelling is identical from 400 mm down to 20 mm
(`1.156e-2`, the same string as the cone control) and its off-axis spelling at 100, 50 and 20
mm (`2.910e-3` at x-offset 200 mm; §1.6).
In the floor regime `a/d < 1` **trivially**, so a coarse sweep reports a falsely
comfortable envelope. This is the trap the non-analytic classes could not escape (§2.4), and
loft shows it in its most extreme measured form: the x-offset-500 spelling reads 0.1458 at 20
mm, 2.3840 at 10 mm, 5.2975 at 4 mm and **70.75 at 0.06694 mm** (§1.6), so a sweep that
stopped at the floor would have reported 0.1458.

### 1.5 Non-analytic classes

| class | subject | finest affordable rung | a/d |
|---|---|---|---|
| sweep | `sweep(circle(100mm), interp(…))` | 10 mm | 0.534 |
| pipe | `pipe(helix(100mm,80mm,300mm), 20mm)` | 5 mm | 0.598 |
| spline | `sweep(circle(100mm), bezier(…))` | 20 mm | 0.013 |
| nurbs surface | `nurbs_surface(3x3 point3 net, …)` | 0.1 mm ‡ | 1.0010 @ 0.14386 mm ‡ |

‡ Unlike the other three rows, 0.1 mm is not where the 90 s budget stopped this class —
it is merely where this task stopped walking it (see Not-budget-limited below). And the
a/d quoted is not the value at that finest rung (0.1 mm itself reads 0.934): it is the
highest value found anywhere on the ladder *or* on the dense sub-0.01 mm walk that
followed it — **1.0010 at 0.14386 mm** (task #7128). 0.12 mm's 0.9975 was 6545's best and
is superseded. See the full ladder and the dense walk below.

sweep: 100 mm 0.148 (floor) · 50 mm 0.296 (floor) · 20 mm 0.379 · **10 mm 0.534**.
pipe (shape-shrunk): 20 mm 0.135 · 10 mm 0.258 · **5 mm 0.598**; 2 mm timed out.
pipe (full size `pipe(helix(300mm,200mm,900mm), 60mm)`): 100 mm 0.123, 20 mm 0.580.
spline: 100 mm 0.003 · 50 mm 0.007 · **20 mm 0.013**; 10 mm timed out.

Sweep, pipe and spline were each still **rising** at their finest affordable rung —
every attempt to go finer timed out at 90 s (§0 Caveat 1), so the rung set each stops at
is a property of the **budget**, not of these three classes. Their entries in §1.1 are
lower bounds, not suprema — see §2.4.

nurbs_surface is different again: nothing in its ladder timed out even down to 0.1 mm
(see Not-budget-limited below), so unlike the three classes above, its rung set is not a
property of the budget — it is affordable far past the mandated 100/50/20/10 mm spine,
which the extended ladder below demonstrates. Its §1.1 entry is nonetheless a **lower
bound, not a supremum**, for the opposite reason: an initial pass read a two-rung fall as
a turnover and entered `sup K = 0.996` as confirmed, but review correctly challenged that
call, and a deeper walk (below) found a **higher** value, 0.9975 at 0.12 mm, in a dense
oscillation task 6545 did not fully resolve (task #7128 later did, and found higher still
— see the dense walk below). Full account — the original
reading, why review challenged it, and the amended ladder — is under "Amendment" below.

Not measurable, recorded honestly:

* **sweep along a helix**, either profile — `TIMEOUT > 90 s` at every `d` tried.
* **`nurbs(…)`** is excluded on semantics, not behaviour: it returns a **Wire**, which
  has no facets and therefore no chord deviation.

(**loft** was listed here as not measurable until task #6188 made it realizable. It is
measured in §1.6, not here, and is the worst class in §1.1.)

**`nurbs_surface(…)` is measurable and does not belong in the list above.** The
`INDETERMINATE` in 0.22 s originally recorded here was **not** a capability gap: it was
an artifact of a flat, bare-`[x,y,z]`-literal control-point/weight encoding rejected at
eval decode — precisely diagnosed, not silently. `control_points`/`weights` are NESTED
(u-major × v) grids (the `GeometryOp::NurbsSurface` variant in
`crates/reify-ir/src/geometry.rs`), and every pole must be a `point3(…)`. The flat
form's 9 bare triples are read as 9 ROWS: row 0 (`[0mm,0mm,0mm]`) itself passes the
row-is-a-List check, so its three elements are each then decoded as a POLE by the pole
decoder `accept_length_point3` (`crates/reify-eval/src/geometry_ops.rs`), called per
pole from the control-point grid loop in `compile_geometry_op`'s `SurfaceKind::Nurbs`
arm (same file). Pole 0 of row 0 is the bare scalar `0mm` — a `Value::Scalar`, not a
`Value::Point`/`Value::Vector` — so it fails the SHAPE check first; the LENGTH-dimension
requirement (task 5745) is a real, separate gate that fires only once the shape check
passes, and is not why this form is rejected (its components are already `mm`).
(`point3_components`, in the same file, survives only as the un-gated decoder for the
three DIRECTION positions.) The compiler gates only arity (`check_arg_count_exact`
checks for exactly 6, in the `"nurbs_surface"` arm of `compile_geometry_call_inner`,
`crates/reify-compiler/src/geometry.rs`), so the wrong-shape call compiles clean, but
eval-time decode then rejects it with a precise per-pole diagnostic: `error: failed to
compile geometry operation: nurbs_surface: control_points[0][0] must be a
Point3<Length>, got Scalar { .. }`, surfaced by the `Err(String)` → `Diagnostic::error`
conversion in the `Err(err)` arm of `execute_realization_ops`
(`crates/reify-eval/src/engine_build.rs`), followed by the message "all geometry
operations failed; no geometry output produced" (emitted from two identical-text call
sites in the same file, so the message text is the citation, not a line). What made the
2026-08-10 observation read as a capability gap is the EXIT CODE, which stays 0 because
the constraint resolves INDETERMINATE (subject undefined) rather than erroring — not a
missing diagnostic.
Re-measured 2026-08-24 at HEAD=`2306e029ec` with the corrected nested
encoding (3x3 u-major control net of `point3(…)` poles, nested unit weights, the same
clamped knots `[0,0,0,1,1,1]` in both directions and `u_degree = v_degree = 2` as
before). EXCERPT of a longer transcript (elides the leading `warning: constraint
expression has type PnrgSpline, expected Bool` line and truncates the trailing `for
PnrgSplineCheck` suffix):

    error: RepresentationWithin: sampled facet deviation 1.713e-2 m exceeds bound 1.000e-6 m

The corrected call is now committed as its own runnable fixture,
`tests/prd-gate/fixtures/pnrg_envelope_nurbs_surface.ri` (`reify check` it), rather than
living only as a comment, so a future measurer does not have to reconstruct it from
prose. It reproduces the same **deviation** — 1.713e-2 m — but not the excerpt above
verbatim: the fixture's checker is named `PnrgNurbsSurfaceCheck` rather than
`PnrgSplineCheck`, and its elided warning names `PnrgNurbsSurface`. Identifier only; the
measurement is identical. No commit sha is cited for that run on purpose — until the
fixture lands it exists only on a task branch, and every amend or rebase orphans a
branch-local sha.

**Full d-ladder, measured 2026-09-01** (task #6545), reproducing the fixture above at
twenty-two rungs from the mandated 100/50/20/10 mm spine down to 0.1 mm. Each rung was
produced by editing `#precision(...)` in a scratch copy of the committed fixture and
re-running `reify check`; the committed file itself stays pinned at 20 mm:

| d | a (m) | a/d | note |
|---|---|---|---|
| 100 mm | 6.518e-2 | 0.6518 | floor |
| 50 mm | 2.494e-2 | 0.4988 | |
| 20 mm | 1.713e-2 | 0.8565 | |
| 10 mm | 6.974e-3 | 0.6974 | |
| 5 mm | 4.107e-3 | 0.8214 | |
| 3 mm | 2.854e-3 | 0.9513 | |
| 1 mm | 8.419e-4 | 0.8419 | |
| 0.8 mm | 7.658e-4 | 0.9573 | |
| 0.6 mm | 5.867e-4 | 0.9778 | |
| 0.5 mm | 4.980e-4 | 0.9960 | initial apparent peak — superseded below |
| 0.4 mm | 3.930e-4 | 0.9825 | |
| 0.3 mm | 2.875e-4 | 0.9583 | |
| 0.25 mm | 2.405e-4 | 0.9620 | |
| 0.2 mm | 1.949e-4 | 0.9745 | |
| 0.18 mm | 1.795e-4 | 0.9972 | |
| 0.17 mm | 1.686e-4 | 0.9918 | |
| 0.16 mm | 1.595e-4 | 0.9969 | |
| 0.15 mm | 1.495e-4 | 0.9967 | |
| 0.14 mm | 1.376e-4 | 0.9829 | |
| 0.13 mm | 1.287e-4 | 0.9900 | |
| **0.12 mm** | 1.197e-4 | **0.9975** | ← highest on this ladder — superseded by the #7128 dense walk below (1.0010 at 0.14386 mm) |
| 0.1 mm | 9.342e-5 | 0.9342 | |

The 20 mm row reproduces the `1.713e-2` excerpted above exactly, confirming this ladder
measures the same committed fixture.

**Amendment, same day, post-review.** The first pass stopped at 0.3 mm (the twelve rows
down to there) and read the fall from 0.996 at 0.5 mm to 0.958 at 0.3 mm as a turnover,
entering `sup K = 0.996` as a genuine supremum in §1.1 and §3.1. Review correctly
challenged this: that two-rung fall (0.038) is smaller than the oscillation amplitude
already visible earlier in the same ladder (0.16 between the 20 mm and 10 mm rows; 0.11
between the 3 mm and 1 mm rows), so it cannot by itself distinguish a turnover from a
trough — exactly the trap §1.2's sphere staircase demonstrates, where branches alternate
between a ~0.76 and a ~2.07 band over a period of only ~0.006 mm, and a coarse sweep lands
on one branch and misses the other entirely.

Ten more rungs, 0.25 mm down to 0.1 mm, were walked to test this (all affordable — see
Not-budget-limited below). They found a **higher** value: **0.9975 at 0.12 mm**, sitting
in a dense, unresolved cluster of near-ties (0.9972 at 0.18 mm, 0.9969 at 0.16 mm, 0.9967
at 0.15 mm) that alternates sharply with lower rows in between (0.9918 at 0.17 mm, 0.9829
at 0.14 mm) — the same staircase shape as the sphere, not a clean turnover. This falsifies
the original supremum claim directly: the true envelope is not `0.996`.

Unlike the sphere, this ladder has **not** resolved the oscillation's period — the sphere
pinned its period by dense 0.006 mm sampling bracketing a suspected peak (§1.2); the
analogous exercise here would need sub-0.01 mm sampling that this task did not attempt. So
the highest value actually found, 0.9975 at 0.12 mm, is presented as exactly that — the
best lower bound this ladder reaches — and **not** as a confirmed supremum. §1.1 and §3.1
are corrected accordingly: this class's status changes from `supremum` to `lower bound`,
joining sweep/pipe/spline as a fourth class whose §1.1 entry is not exhaustive, but for a
different reason: unresolved oscillation, not the 90 s budget wall.

**Floor.** Achieved is pinned at `6.518e-2` for all `d ≥ 100 mm`: probed additionally at
200 mm and 400 mm, both return the identical `6.518e-2`. The 100 mm row above sits inside
that floor, mirroring how §1.4 phrases the sphere's pin.

**Deterministic.** The original fourteen rungs (the twelve 100 mm→0.3 mm rows tabulated
above plus the 200 mm and 400 mm floor probes) returned byte-identical achieved values
across 3 consecutive full-ladder repetitions. The ten rungs added below 0.3 mm (0.25 mm
down to 0.1 mm) returned byte-identical achieved values across 2 repetitions each,
including the three highest candidates (0.18 mm, 0.16 mm, 0.12 mm). Wall clocks varied
under load, per §0 Caveat 1; no achieved value did, at either rep count.

**Not budget-limited.** No rung timed out, all the way down to 0.1 mm: one timed rep gave
14.8 s (0.25 mm), 16.7 s (0.2 mm), 20.7 s (0.15 mm) and 40.8 s (0.1 mm) — still comfortably
inside the 90 s wall, unlike sweep, pipe and spline above, whose ladders were cut short by
the wall, not by the geometry. Per the 1/deflection cost scaling (§2.1), another halving
to 0.05 mm would cost roughly 80 s and start to approach that wall — the reason this
amendment's walk stopped at 0.1 mm rather than going finer still.

**Provenance for this block** — own stamp, a different binary/HEAD/session than §0's
identity table:

| | |
|---|---|
| binary | `target/release/reify`, built 2026-09-01 01:27 (newer than every `crates/` commit reachable from HEAD, including the task/5784 merge `cb8b55d3fa` landed 01:08 that same morning — no rebuild needed for either the original 14-rung round or this amendment) |
| HEAD | `01c1e3e445` (branch `task/6545`) for the original 14-rung round; `b8042b4880` (same branch, after this task's own doc commits) for the twenty-two-rung amendment — neither moves the binary |
| kernel | OCCT 7.8 (53 `libTK*.so.7.8` linked; `has_occt` live, confirmed functionally — all 42 original runs (14 rungs × 3 reps) plus 20 amendment runs (10 new rungs × 2 reps) realized and passed the §0 Caveat-2 datum gate) |
| machine | AMD Ryzen 9 3950X, 16C/32T (same box as §0) |
| load | 88.85 – 118.40 1-min loadavg across the original 3 reps; 90.65 during the amendment |

`Operation::SurfaceNurbs` remains genuinely absent from `occt_capability_descriptor()`
(`crates/reify-kernel-occt/src/register.rs`) — that fact still holds. What was wrong was
the inference that the absence prevents realization: it resolves instead via the
`DEFAULT_KERNEL_NAME` fallback, which is exactly how the corrected call above realizes.
The full d-ladder for this class is recorded above and summarized in §1.1 and §3.1,
closing out follow-up task #6545 (ticket `tkt_0RSV7JNW3WXWDSFJGRDMHDT63T`).


**Dense sub-0.01 mm walk, measured 2026-09-12** (task #7128), resolving the oscillation
the 6545 amendment above left open. Same committed fixture, same method — edit
`#precision(...)` in a scratch copy, re-run `reify check` — on a different binary, HEAD,
kernel and session than either block above.

**Provenance for this block** — own stamp; deliberately *not* §0's identity table, and
not the 6545 block's:

| | |
|---|---|
| binary | `target/release/reify`, built 2026-09-11 23:14 (newer than every `crates/` commit reachable from HEAD; no rebuild needed) |
| HEAD | `ebecf20df5` (branch `task/7128`) |
| kernel | OCCT 7.8 (26 `libTK*.so.7.8` ldd lines, 23 distinct sonames; `has_occt` live, confirmed functionally — every probe below realized and passed the §0 Caveat-2 datum gate). 27 `libTK*.so.7.9` lines are also linked, via gmsh; reify's own calls bind 7.8, which is why the counts here differ from §0's 56 and the 6545 block's 53 without the measurement differing |
| machine | AMD Ryzen 9 3950X, 16C/32T (same box as §0), Linux 7.0.0-28 — a **different kernel** than §0's 6.14.0-37 |
| load | 368.83 – 424.17 1-min loadavg across this session — 3–5× the load of either block above, and per §0 Caveat 1 this moves wall clocks only |

**Reproduction gate — cross-session, cross-binary, cross-HEAD, cross-kernel.** Before any
new datum was trusted, three rungs already published in the 6545 ladder above were
re-measured on this session's apparatus:

| d | a (m) | a/d | published above |
|---|---|---|---|
| 20 mm | 1.713e-2 | 0.8565 | 1.713e-2 / 0.8565 |
| 0.18 mm | 1.795e-4 | 0.9972 | 1.795e-4 / 0.9972 |
| 0.12 mm | 1.197e-4 | 0.9975 | 1.197e-4 / 0.9975 |

All three match the published strings exactly, and all three emitted the §0 Caveat-2
datum line. This is a **stronger determinism datum than the same-session repetitions the
6545 block records**: those establish that a fixed binary repeats itself, whereas these
show the achieved value survives a rebuilt binary, a different HEAD, a different kernel
and a 3–5× load change. It also validates the apparatus used below — a mismatched value
here would have indicted the harness rather than the geometry, and the gate is
genuinely falsifiable: a wrong binary, a stale fixture, the `E_MODULE_PATH_MISMATCH`
scratch-file trap (§4) or a silent non-realization each fail it loudly.

**Stage A — the 0.005 mm bracket walk over [0.12, 0.18] mm.** The six new midpoints,
plus a re-probe of all seven published 0.01 mm rungs in the same bracket so the whole
sweep is one internally consistent session. Ratios by `decimal.Decimal` /
`ROUND_HALF_UP` at 4 dp throughout (§4):

| d | a (m) | a/d | note |
|---|---|---|---|
| 0.12 mm | 1.197e-4 | 0.9975 | reproduces 6545 |
| 0.125 mm | 1.245e-4 | 0.9960 | new |
| 0.13 mm | 1.287e-4 | 0.9900 | reproduces 6545 |
| 0.135 mm | 1.305e-4 | 0.9667 | new — local trough |
| 0.14 mm | 1.376e-4 | 0.9829 | reproduces 6545 |
| **0.145 mm** | 1.450e-4 | **1.0000** | new ← highest in Stage A |
| 0.15 mm | 1.495e-4 | 0.9967 | reproduces 6545 |
| 0.155 mm | 1.539e-4 | 0.9929 | new |
| 0.16 mm | 1.595e-4 | 0.9969 | reproduces 6545 |
| 0.165 mm | 1.638e-4 | 0.9927 | new |
| 0.17 mm | 1.686e-4 | 0.9918 | reproduces 6545 |
| 0.175 mm | 1.729e-4 | 0.9880 | new |
| 0.18 mm | 1.795e-4 | 0.9972 | reproduces 6545 |

All seven re-probed rungs reproduce the 6545 ladder's achieved strings exactly, so the
six new midpoints interleave a verified ladder rather than a drifting one.

**A midpoint beats the published peak.** 0.145 mm reads `a` = 1.450e-4 against `d` =
1.45e-4 — a ratio of **1.0000**, above the 0.9975 at 0.12 mm that 6545 recorded as this
class's best lower bound. That rung lies exactly halfway between two published rungs
(0.14 mm, 0.9829 and 0.15 mm, 0.9967), neither of which hints at it. The published
ladder did not merely fail to resolve the oscillation's period; it stepped over the
highest value in its own bracket.

**0.005 mm does not resolve the structure either.** It is a necessary refinement of the
6545 ladder's 0.01 mm spine, but not a sufficient one, and this table shows why on its own
terms. The ratio is non-monotone between every pair of adjacent rungs, and the swing
between adjacent 0.005 mm rungs reaches 0.0233 (0.9900 at 0.130 mm to 0.9667 at 0.135 mm),
and across two rungs — 0.010 mm, the 6545 ladder's own step — 0.0333 (0.9667 at 0.135 mm
to 1.0000 at 0.145 mm). Meanwhile the five leading values — 0.9967, 0.9969, 0.9972, 0.9975
and 1.0000 — are separated from one another by as little as 0.0002. **The between-rung
swing is two orders of magnitude larger than the gaps between the candidates the sweep is
trying to rank**, so a 0.005 mm grid cannot establish which of them is the true local
maximum, nor that any of them is a local maximum at all: each is simply the largest value
on whichever grid happened to be sampled. This is the §1.2 aliasing trap in its exact
form — the sphere's branches alternate over ~0.006 mm, and a grid at that same order lands
on one branch and misses the other.

**Sub-brackets carrying the leaders**, to be walked at 0.001 mm in Stage B: **[0.143,
0.147] mm** around the new 1.0000; **[0.118, 0.122] mm** around the published 0.9975; and
**[0.148, 0.152]**, **[0.158, 0.162]** and **[0.178, 0.182] mm** around the three
near-ties at 0.15, 0.16 and 0.18 mm. The trough at 0.135 mm is not walked — it is the one
rung in this bracket that is unambiguously far from the leaders.


**Stage B — the 0.001 mm fine walk**, across the five sub-brackets Stage A flagged. A
fourth column is added here: the **deficit** `δ = d − a`. At these magnitudes `a` prints
as `X.XXXe-4`, so the display quantum is exactly 1e-7 m and δ is an integer count of
quanta — a sharper lens than the ratio, because `a/d` compresses the whole interesting
range into its last two digits while δ reads it directly.

| d | a (e-4 m) | δ (quanta) | a/d | note |
|---|---|---|---|---|
| 0.118 mm | 1.175 | 5 | 0.9958 | |
| 0.119 mm | 1.183 | 7 | 0.9941 | |
| 0.120 mm | 1.197 | 3 | 0.9975 | 6545's peak |
| 0.121 mm | 1.183 | 27 | 0.9777 | |
| 0.122 mm | 1.174 | 46 | 0.9623 | |
| 0.143 mm | 1.429 | 1 | 0.9993 | |
| **0.144 mm** | 1.440 | **0** | **1.0000** | `a` = `d` to 4 s.f. |
| **0.145 mm** | 1.450 | **0** | **1.0000** | `a` = `d` to 4 s.f. |
| 0.146 mm | 1.451 | 9 | 0.9938 | |
| 0.147 mm | 1.468 | 2 | 0.9986 | |
| 0.148 mm | 1.476 | 4 | 0.9973 | |
| 0.149 mm | 1.479 | 11 | 0.9926 | |
| 0.150 mm | 1.495 | 5 | 0.9967 | |
| 0.151 mm | 1.507 | 3 | 0.9980 | |
| 0.152 mm | 1.512 | 8 | 0.9947 | |
| 0.158 mm | 1.567 | 13 | 0.9918 | |
| 0.159 mm | 1.585 | 5 | 0.9969 | |
| 0.160 mm | 1.595 | 5 | 0.9969 | |
| 0.161 mm | 1.605 | 5 | 0.9969 | |
| 0.162 mm | 1.617 | 3 | 0.9981 | |
| 0.178 mm | 1.771 | 9 | 0.9949 | |
| 0.179 mm | 1.775 | 15 | 0.9916 | |
| 0.180 mm | 1.795 | 5 | 0.9972 | plateau with 0.181 |
| 0.181 mm | 1.795 | 15 | 0.9917 | identical `a` |
| 0.182 mm | 1.789 | 31 | 0.9830 | |

**Cross-check.** The five rungs in [0.118, 0.122] mm were measured in a separate earlier
session, on the same binary but before any commit in this block, and returned 0.9958 /
0.9941 / 0.9975 / 0.9777 / 0.9623. This stage reproduces all five exactly. They are
re-measured here, not copied.

**`a` is not monotone in `d`.** Requesting a *coarser* precision can yield a *smaller*
deviation: 0.120 mm → 1.197e-4 but 0.121 mm → 1.183e-4, and 0.181 mm → 1.795e-4 but
0.182 mm → 1.789e-4. Two different `d` can also return identical `a` (0.119 mm and
0.121 mm both read 1.183e-4). Any reasoning that assumes `a` rises with `d` — including
any bisection that assumes it — is unsound on this class.

**Shape: no single period, and at least three distinct local behaviours.** Within
[0.143, 0.152] mm — the one bracket walked contiguously across 0.010 mm — the ratio has
local maxima at 0.144–0.145 mm, 0.147 mm and 0.151 mm, i.e. spacings of **0.002 and
0.004 mm**. They do not recur on a regular interval, so these samples do **not** exhibit a
period, and none is asserted. That is a real difference from the sphere (§1.2), whose two
branches alternate on a clean ~0.006 mm period; this class is not a two-branch staircase,
and the sphere's "period" has no direct analogue here. The three behaviours visible:

* **Exact plateau** — 0.180 and 0.181 mm return byte-identical `a` = 1.795e-4, so `a` is
  locally constant while `d` varies. Inside a plateau the ratio *falls* as `d` rises, so
  its maximum sits at the plateau's **lower** edge.
* **Unit-slope tracking** — 0.159, 0.160 and 0.161 mm hold δ constant at 5 quanta while
  `a` rises in exact 1e-7 m steps with `d`. Here `a = d − c` for fixed `c`, so the ratio
  `1 − c/d` *rises* as `d` rises and its maximum sits at the segment's **upper** edge.
* **Sharp sawtooth** — δ runs 3 → 27 → 46 quanta across 0.120 → 0.122 mm, an order of
  magnitude of change in two steps.

Because the ratio's maximum sits at a *lower* edge in the first regime and an *upper* edge
in the second, there is no single direction to search, and this is why Stage C bisects
each candidate individually rather than applying one rule to all of them.

*Hypothesis (not established by these samples):* an interleaved u/v subdivision, in which
two independent facet-count staircases beat against each other, would produce exactly this
— irregular maxima spacing and locally varying behaviour, rather than the single period a
one-dimensional staircase gives. Distinguishing it would require walking the control net's
u and v spans independently, which this task does not do.

**The δ = 0 rungs, and what Stage C must ask of them.** At 0.144 mm and 0.145 mm the
deficit reaches the display floor: `a` equals `d` to all four significant figures printed.
0.143 mm returns a different `a` (1.429e-4), so if 0.144 mm sits on a plateau, that
plateau's lower edge lies in (0.143, 0.144] mm — and **any `d` below 0.144 mm that still
returns 1.440e-4 yields a ratio strictly above 1**. That is the one measurement in reach
that could settle the K = 1 question above the display floor, and it is Stage C's target.

**Stage C — sub-0.001 mm plateau-edge bisection**, 31 probes. This is the stage that
pins the answer, and it exploits the structure rather than gridding it. Where `a` is
locally constant on a plateau, `d` falling inside that plateau leaves `a` fixed, so `a/d`
rises to a local maximum at the plateau's **lower edge**. The supremum is therefore an
edge property, findable by bisection — about ten probes per edge, against the ~600 a
1e-4 mm grid over [0.12, 0.18] mm would need. Because Stage B showed `a` is not monotone
in `d`, each bracket was scanned rather than blind-bisected, and each pinned edge is
bracketed by a probe on both sides that returns a *different* `a`.

**P1 — `a` = 1.440e-4, the leader.** Lower edge pinned to 1e-5 mm:

| d | a (m) | a/d | |
|---|---|---|---|
| 0.14385 mm | 1.439e-4 | 1.0003 | below the edge — different `a` |
| **0.14386 mm** | 1.440e-4 | **1.0010** | ← `d_lo`, pinned lower edge |
| 0.14387 mm | 1.440e-4 | 1.0009 | |
| 0.14388 mm | 1.440e-4 | 1.0008 | |
| 0.14389 mm | 1.440e-4 | 1.0008 | |
| 0.1439 mm | 1.440e-4 | 1.0007 | |
| 0.14395 mm | 1.440e-4 | 1.0003 | |
| 0.144 mm | 1.440e-4 | 1.0000 | Stage B's δ = 0 rung |
| 0.1442 mm | 1.441e-4 | 0.9993 | above the plateau — different `a` |

The ratio falls monotonically across the plateau exactly as the model predicts, and
Stage B's 1.0000 at 0.144 mm is revealed as the plateau's *upper* end, not its peak.
`d_lo` ∈ (0.14385, 0.14386] mm — edge resolution **1e-5 mm**. Measured plateau width
≥ 0.00014 mm, upper edge bracketed in [0.144, 0.1442) mm, so width ∈ [0.00014, 0.00035) mm.
The supremum over P1 is `1.440e-4 / d_lo` = **1.0010**, and that value is stable across
the whole pinned edge bracket (1.0010 at both `d_lo` = 0.14386 and `d_lo` → 0.14385⁺), so
pinning the edge finer would not change it at 4 dp.

**P2 — `a` = 1.433e-4.** A narrow plateau confined to (0.14319, 0.14325) mm:

| d | a (m) | a/d | |
|---|---|---|---|
| 0.14319 mm | 1.428e-4 | 0.9973 | below — different `a` |
| 0.1432 mm | 1.433e-4 | 1.0007 | `d_lo` ∈ (0.14319, 0.1432] |
| 0.14325 mm | 1.432e-4 | 0.9997 | above — different `a` |

**P3 — `a` = 1.450e-4.** Stage B's other δ = 0 rung, likewise not its own plateau's peak:

| d | a (m) | a/d | |
|---|---|---|---|
| 0.1449 mm | 1.449e-4 | 1.0000 | below — different `a` |
| 0.14495 mm | 1.450e-4 | 1.0003 | `d_lo` ∈ (0.1449, 0.14495] |
| 0.145 mm | 1.450e-4 | 1.0000 | Stage B's δ = 0 rung |

**P4 — `a` = 1.428e-4**, walked as a control, and deliberately **not** counted as a
pinned edge: no probe below 0.14305 mm returns a different `a`, so its lower edge is
unbracketed and it fails this section's own test. It is a *low* plateau, and it shows the
mechanism cleanly in the direction that does not flatter the result — `a` byte-identical
across seven probes spanning 0.00014 mm while the ratio falls monotonically with rising
`d`:

| d | 0.14305 | 0.1431 | 0.14315 | 0.14316 | 0.14317 | 0.14318 | 0.14319 |
|---|---|---|---|---|---|---|---|
| a (m) | 1.428e-4 | 1.428e-4 | 1.428e-4 | 1.428e-4 | 1.428e-4 | 1.428e-4 | 1.428e-4 |
| a/d | 0.9983 | 0.9979 | 0.9976 | 0.9975 | 0.9974 | 0.9973 | 0.9973 |

The 0.0002 mm scan of [0.179, 0.180] mm also corrected a Stage B reading: 0.1798 mm
returns 1.796e-4 (0.9989), *above* the 1.795e-4 that 0.180 and 0.181 mm share, so the
plateau Stage B saw there is not that bracket's local maximum either. The same scan over
[0.1442, 0.1448] mm found 0.9993 / 0.9979 / 0.9972 / 0.9965 — falling away, confirming P1
is left behind above 0.1442 mm.

**Result: the achieved deviation exceeds the requested precision.** The best value found
is **1.0010 at `d` = 0.14386 mm**, and it clears the display floor by a margin that makes
it unambiguous rather than marginal. `a` prints as 1.440e-4, so the true achieved value
lies in [1.4395e-4, 1.4405e-4); `d` is exact at 1.4386e-4 m because it is the *request*,
not a measurement. The true ratio therefore lies in **[1.000626, 1.001321)** — an interval
lying *entirely* above 1. Two further plateau edges (P2 at 1.0007, P3 at 1.0003) exceed 1
independently, as do 0.1439, 0.14395 and 0.14385 mm, so the finding does not rest on a
single probe.

**What this is, and is not.** 1.0010 is a supremum **over the plateaus walked** — P1 to P4
plus the brackets scanned around them. A dense search raises a lower bound; it can never
prove a supremum over a continuum, and no claim of exhaustiveness is made here. Two
distinct statements follow, and they should not be conflated: that **`K` > 1 for this
class is established** — that is a lower-bound claim, and a lower bound above 1 settles
it — while **the numeric value 1.0010 remains a lower bound** on the true supremum. All 69
probes of the dense walk itself (92 runs) — Stage A onward, `d` ∈ [0.118, 0.182] mm — lie
within [0.9623, 1.0010], with no sign of a second branch like the sphere's ~2.07 tread,
but that is an observation about where these samples fell, not a bound on where others
might. (The block's three reproduction-gate runs sit outside that window by construction:
the 20 mm rung reads 0.8565, deep in the coarse regime.)

**The display-precision wall — and why this result clears it.** The achieved deviation is
formatted `{achieved:.3e}` at `crates/reify-eval/src/tolerance_combine.rs:460`, which is
the **only** site under `crates/` that emits the sampled facet deviation —
`grep -rn 'sampled facet deviation' crates/*/src/` returns exactly that line plus two
comments in the same file (:401, :444). (`.3e` itself is common under `crates/` and proves
nothing — `crates/reify-constraints/src/solver.rs:2531` even binds its own `achieved`; the
discriminator is the message, not the format spec. And `reify check`'s usage line offers no
`--json` or `--verbose` alternative: `reify check [--strict] [--purpose
<name>=<binding>]... [--cfg <key=value|flag>]... <file>`.) Four significant figures is
therefore the whole apparatus, and it is a hard floor, not a convention this task could
dial up.

*Derived.* A reported `a` of `X.XXXe-4` bounds the true value to ± 0.5e-7 m, so a ratio
carries ± 0.5e-7/`d` — about ± 4.2e-4 at `d` = 0.12 mm, ± 2.8e-4 at 0.18 mm. **Every ratio
in §1 of this note inherits that bound.** Its sharpest consequence is that a ratio *read
as 1.0000 cannot by itself settle anything*: Stage B's 0.144 mm and 0.145 mm rungs are
each consistent with a true ratio anywhere in [0.99965, 1.00035), spanning 1. Had the walk
stopped at Stage B, its two 1.0000 readings would have been exactly the ambiguous
non-result this wall predicts, and reporting them as `K = 1` would have been an artifact
of the formatter.

*What breaks the tie is an asymmetry.* `d` carries **no** uncertainty — it is the
*request*, an exact input, not a measurement — so only one side of the ratio is fuzzy. On
a plateau `a` is fixed while `d` moves freely, so driving `d` down inside a plateau raises
the ratio by an amount set by the **plateau's width**, which is exact, rather than by the
display precision, which is not. That is why Stage C's bisection could reach a verdict
where Stage B's grid could not: it converts a display-precision problem into a
`d`-resolution problem, and `d` resolves arbitrarily.

**Verdict: outcome (b) — the class's true `K` exceeds 1.** At `d` = 0.14386 mm the printed
ratio is **1.0010**, and the display bound puts the true ratio in [1.000626, 1.001321), an
interval lying entirely above 1 with its lower end 6 quanta clear of the boundary. This is
not a marginal reading at the precision floor; it is above the ~1.0005 threshold at which
the wall stops mattering, and it is corroborated by five further probes above 1 at two
independent plateau edges (§ Stage C). *Measured*, and reproduced byte-identically on a
second repetition.

*Derived consequence.* `n` = ⌈log₂ `K`⌉ = **1** for this class, where §3.1 previously
recorded 0 — the first class in this note for which the achieved deviation is shown to
exceed the request at all. §1.1 and §3.1 are corrected accordingly.

*And what remains open.* Outcome (b) settles the **direction** — `K` > 1 — because a lower
bound above 1 settles it. It does not make 1.0010 a proven supremum: that number is still
the best value found over the plateaus walked, and the true supremum can only be higher.
The distinction matters downstream and is carried into §3.1 rather than rounded away.
**Determinism and datum gates.** Every probe in this block passed the §0 Caveat-2 datum
gate: the harness extracts `a` only from the `deviation <X> m` capture and emits a literal
`NO-DATUM` token when that capture is empty, so a non-realization cannot enter a table as a
number. That token did not separate a non-realization from a `timeout` kill — both leave
the capture empty — which is immaterial here because **no** `NO-DATUM` occurred at all, and
that excludes both causes at once; §4's published recipe splits them anyway, so a walk that
does hit one can tell which. The sentinel was checked against two live failures before use
and reported `NO-DATUM` for both rather than an empty field. One is §0 Caveat 2's shape
exactly — this fixture with its `RepresentationWithin` bound loosened to 50 mm prints
`OK PnrgNurbsSurfaceCheck#constraint[0]` / `All constraints satisfied.` and **exits 0**,
with no deviation line anywhere. The other is loud: a scratch file whose basename does not
match its `module` declaration fails with `E_MODULE_PATH_MISMATCH` on **exit 1**. Both
leave the capture empty, which is the point — the gate keys on the deviation line being
present, not on the exit code, so a quiet failure and a loud one are caught alike. (An
earlier draft of this paragraph credited the module-path mismatch with exiting 0; it was
re-measured on this lane's binary and exits 1.)

*Stage A:* all six new rungs were re-run for a second repetition — matching the rep count
§1.5 records for 6545's own sub-0.3 mm rungs — and returned **byte-identical achieved
strings**, including the new 1.0000 leader at 0.145 mm. Zero divergence. Each repetition
regenerates its scratch `.ri` from the committed fixture rather than re-running a cached
file, so the rep exercises the whole path, not just the kernel. Wall clocks varied
substantially with load (the sweeps below ran between 122 and 424 1-min loadavg); no
achieved value did, which is §0 Caveat 1's standing distinction holding at this
resolution too.

*Stage B:* the eight leading rungs were re-run for a second repetition — the top five by
ratio (0.144, 0.145, 0.143, 0.162, 0.147 mm), unconditionally every rung whose ratio
rounds to ≥ 0.999, and 0.151, 0.120 and 0.180 mm besides — and all eight returned
**byte-identical achieved strings**. Zero divergence. This gate carries more weight than
Stage A's: these are the rungs the block's conclusions rest on, and the two at δ = 0 sit
exactly on the K = 1 boundary, where a single unreproducible digit in the last printed
place would flip the verdict rather than perturb it. Both returned 1.440e-4 and 1.450e-4
again.

*Stage C:* every probe defining a pinned edge was re-run for a second repetition — **both
sides** of all three brackets (0.14385/0.14386, 0.14319/0.1432, 0.1449/0.14495 mm), P1's
upper bracket (0.144/0.1442 mm), and the highest-ratio probe overall — and all nine
returned **byte-identical achieved strings**. Zero divergence. A plateau edge is precisely
where the tessellator's facet count changes, so it is the one place a non-deterministic
tie-break would surface if one existed; pinning an edge without re-running both of its
sides would have been the weakest link in the chain, and `d_lo` = 0.14386 mm returned
1.440e-4 both times.

*Totals across the block:* **95 runs over 72 probes** — 3 reproduction-gate runs, 19 in
Stage A (13 rungs, 6 re-run), 33 in Stage B (25 rungs, 8 leaders re-run) and 40 in Stage C
(31 probes, 9 edge probes re-run); the dense walk alone is 69 probes over 92 runs. Probes
are not distinct `d` either: several recur across stages — 0.12, 0.145, 0.15, 0.16 and
0.18 mm between the gate, Stage A and Stage B, and 0.144, 0.145, 0.179 and 0.180 mm between
Stage B and Stage C — so the distinct-`d` count is smaller again, and neither tally above
should be read as one. Every run emitted the datum line; not a single `OK`,
`INDETERMINATE` or `NO-DATUM` occurred. No achieved value differed between repetitions
anywhere in the block, at any stage or resolution. Nothing timed out, so this class remains
**not budget-limited** at these `d` — the finest probe in the block, and so by §2.1's
1/deflection scaling its most expensive, is Stage B's 0.118 mm, well inside the regime 6545
already showed to be affordable.

### 1.6 Loft — measured (task #6318)

Task **#6188** made `loft(…)` realizable from source (merge `fca4a9ad5f`); the History paragraph at the
end of this section records what this section said before, and what is retracted. That is what made this
block possible, and the first thing it measured is that **the committed subject is the wrong probe**: the
*coaxial* spelling measures as a **cone**. A loft whose top section is **offset sideways** is a different
object, and it is the **worst class in this note by a wide margin** — and chaotic in `d`.

**Headline (measured).** `loft(circle(500mm), translate(circle(250mm), 500mm, 0mm, 800mm))` at
`#precision(0.06694mm)` returns `a` = **4.736e-3 m**, so **`a/d` = 70.7499** — about 34×
the sphere's 2.079, until now the worst class. The display wall (§1.5) puts the true ratio in
**[70.742, 70.757)**, entirely above 16. This is a **lower bound**, and a stronger kind of one than
any other row in this note (see "What this is, and is not" below): the ratio is not a smooth function of `d`,
the highest readings sit at the fine end of the walked range, where each rung costs minutes, and one regime
coordinate of many was walked.

**Provenance for this block** — own stamp; deliberately *not* §0's identity table, and not any §1.5 block's:

| | |
|---|---|
| binary | `target/release/reify`, built 2026-10-06 16:59 — newer than every `crates/` commit reachable from the measured HEAD (the last is the task/6188 merge `fca4a9ad5f`, 15:32 the same day); not rebuilt afterwards |
| HEAD | `8e81ea8339` (branch `task/6318`) when measured. The branch was later rebased onto `18d86776e7`; the only `crates/` paths that changed (`reify-test-support`'s helpers and one of its tests) are not in `reify-cli`'s normal dependency graph (`cargo tree --offline -p reify-cli -e normal -i reify-test-support` reports `nothing to print`), so the binary is unaffected and every datum below comes from this one binary |
| kernel | OCCT 7.8 (26 `libTK*.so.7.8` ldd lines, 26 distinct sonames; `has_occt` live, confirmed functionally — every probe below realized and passed the §0 Caveat-2 datum gate). 27 `libTK*.so.7.9` lines are also linked, via gmsh; reify's own calls bind 7.8 |
| machine | AMD Ryzen 9 3950X, 16C/32T (same box as §0), Linux 7.0.0-31 |
| load | **45 – 541** 1-min loadavg at probe start across the session; per §0 Caveat 1 this moves wall clocks only |

**Reproduction gate.** Five readings were taken at plan time, earlier the same day, on this same binary.
They were re-measured before any new datum was trusted, and all five match **exactly**:

| subject | d | plan-time reading (a, m) | measured here (a, m) | a/d |
|---|---|---|---|---|
| coaxial | 20 mm | 1.156e-2 | 1.156e-2 | 0.5780 |
| coaxial | 10 mm | 7.642e-3 | 7.642e-3 | 0.7642 |
| coaxial | 1 mm | 8.745e-4 | 8.745e-4 | 0.8745 |
| off-axis, x = 200 mm | 10 mm | 1.885e-2 | 1.885e-2 | 1.8850 |
| off-axis, x = 200 mm | 5 mm | 2.175e-2 | 2.175e-2 | 4.3500 |

This validates the apparatus (harness, fixture snapshot, subject rewrite) — a wrong binary, a stale
fixture, the `E_MODULE_PATH_MISMATCH` trap (§4) or a silent non-realization each fail it loudly. It is a
weaker gate than #7128's: same binary and same day, so it says nothing about a rebuilt binary.

**The committed coaxial spelling is the cone class.** The committed subject,
`loft(circle(500mm), translate(circle(250mm), 0mm, 0mm, 800mm))`, walked from 400 mm to 0.05 mm:

| d | a (m) | a/d | `cone(500mm, 250mm, 800mm)` |
|---|---|---|---|
| 400 mm | 1.156e-2 | 0.0289 | identical |
| 200 mm | 1.156e-2 | 0.0578 | identical |
| 100 mm | 1.156e-2 | 0.1156 | identical |
| 50 mm | 1.156e-2 | 0.2312 | identical |
| 20 mm | 1.156e-2 | 0.5780 | identical |
| 10 mm | 7.642e-3 | 0.7642 | identical |
| 5 mm | 3.889e-3 | 0.7778 | identical |
| 3 mm | 2.518e-3 | 0.8393 | identical |
| 2 mm | 1.665e-3 | 0.8325 | identical |
| 1 mm | 8.745e-4 | 0.8745 | identical |
| 0.8 mm | 6.972e-4 | 0.8715 | identical |
| 0.6 mm | 5.225e-4 | 0.8708 | identical |
| 0.5 mm | 4.379e-4 | 0.8758 | identical |
| 0.4 mm | 3.564e-4 | 0.8910 | identical |
| 0.3 mm | 2.702e-4 | 0.9007 | identical |
| 0.2 mm | 1.792e-4 | 0.8960 | identical |
| 0.15 mm | 1.356e-4 | 0.9040 | identical |
| 0.1 mm | 9.098e-5 | 0.9098 | identical |
| 0.05 mm | 4.605e-5 | 0.9210 | identical |

`cone(500mm, 250mm, 800mm)` — `pnrg_envelope_cone.ri`'s subject with its height set to 800 mm to match —
returned a **byte-identical `a` at all 19 rungs** (last column; **measured**). The coaxial loft therefore
contributes no loft-class datum: it *is* the cone class, and its K — 0.9210 at 0.05 mm, the highest on the
ladder and still creeping up as `d` falls — is a cone number, comparable to §1.3's. It stays committed only
as a control. *Hypothesis (not established by these runs):* ThruSections between two coaxial circles reduces
to an analytic conical face, so the tessellator sees the same surface class as `cone(…)`. Nothing here
distinguishes that from any other reason two code paths could return identical strings.

**Four loft-specific spellings, and why one spine is not enough.** Each swapped into the fixture's
`let g = …` line; all four realized at every rung of every ladder below:

| label | subject |
|---|---|
| S1 off-axis | `loft(circle(500mm), translate(circle(250mm), 200mm, 0mm, 800mm))` |
| S2 circle→ellipse | `loft(circle(500mm), translate(ellipse(400mm, 200mm), 0mm, 0mm, 800mm))` |
| S3 three sections | `loft(circle(500mm), translate(circle(300mm), 0mm, 0mm, 400mm), translate(circle(150mm), 0mm, 0mm, 800mm))` |
| S4 rectangle→circle | `loft(rectangle(800mm, 800mm), translate(circle(250mm), 0mm, 0mm, 800mm))` |

`a` (m), with `a/d` in parentheses:

| d | S1 off-axis | S2 circle→ellipse | S3 three sections | S4 rectangle→circle |
|---|---|---|---|---|
| 100 mm | 2.910e-3 (0.0291) | 1.016e-2 (0.1016) | 2.293e-2 (0.2293) | 1.827e-2 (0.1827) |
| 50 mm | 2.910e-3 (0.0582) | 8.066e-3 (0.1613) | 2.293e-2 (0.4586) | 1.632e-2 (0.3264) |
| 20 mm | 2.910e-3 (0.1455) | 8.901e-3 (0.4451) | 8.152e-3 (0.4076) | 1.254e-2 (0.6270) |
| 10 mm | 1.885e-2 (1.8850) | 8.946e-3 (0.8946) | 8.313e-3 (0.8313) | 7.446e-3 (0.7446) |
| 5 mm | 2.175e-2 (4.3500) | 4.896e-3 (0.9792) | 7.146e-3 (1.4292) | 4.315e-3 (0.8630) |
| 3 mm | 2.955e-3 (0.9850) | 2.917e-3 (0.9723) | 4.844e-3 (1.6147) | 2.735e-3 (0.9117) |
| 2 mm | 5.566e-3 (2.7830) | 4.032e-3 (2.0160) | 1.780e-3 (0.8900) | 1.623e-3 (0.8115) |
| 1 mm | 9.913e-4 (0.9913) | 9.820e-4 (0.9820) | 1.292e-3 (1.2920) | 9.724e-4 (0.9724) |
| 0.5 mm | 1.122e-3 (2.2440) | 4.895e-4 (0.9790) | 7.076e-4 (1.4152) | 4.608e-4 (0.9216) |
| 0.3 mm | 2.830e-4 (0.9433) | 2.976e-4 (0.9920) | 3.680e-4 (1.2267) | 2.914e-4 (0.9713) |

On the spine S1 peaks at 4.3500 (5 mm), S2 at 2.0160 (2 mm), S3 at 1.6147 (3 mm), S4 at 0.9724 (1 mm). A
0.5 mm grid over [1, 20] mm, run for all four (S4 included although its spine never exceeded 1, because the
spine aliases), moves two of them: S3 to **1.8707** at 4.5 mm and S4 to 0.9764 at 14 mm; S1 stays at 4.3500
and S2 at 2.0160. S2's 2 mm rung is a lone spike — its grid neighbours at 1.5 and 2.5 mm read 0.9787 and
0.9640 — and S3's best rung was missed by the spine entirely: §1.2's aliasing trap, again. **S4 never
exceeds 1** on any rung measured; S1–S3 do.

**The offset regime.** S1's coordinate is the top section's x-offset over the bottom radius (`x/r`, with
`r` = 500 mm). Every offset was run at the three `d` the walk required (10, 5, 3 mm) and over the full
0.5 mm grid [1, 20] mm — three fixed `d` per offset would have aliased here exactly as the spine does,
because the position of each offset's peak moves with the offset — and, near the peaks, over a 0.1 mm grid
[3, 7] mm. 600, 750 and 1000 mm go beyond the planned range (`x/r` ≤ 1) and are labelled as an extension:

| x-offset (mm) | x/r | a/d @ 10 mm | @ 5 mm | @ 3 mm | max, 0.5 mm grid [1, 20] (at d) | max, 0.1 mm grid [3, 7] (at d) | lowest d with a/d > 1 in [3, 7] | a/d @ 0.1 mm |
|---|---|---|---|---|---|---|---|---|
| 0 | 0.00 | 0.7642 | 0.7778 | 0.8393 | 0.8745 (1) | — | — | 0.9098 |
| 25 | 0.05 | 2.0020 | 4.5660 | 0.9577 | 4.5867 (4.5) | 4.6592 (4.9) | 4.4 | 10.2300 |
| 50 | 0.10 | 1.9870 | 4.0480 | 0.9623 | 4.5511 (4.5) | 4.5511 (4.5) | 4.4 | 10.3100 |
| 100 | 0.20 | 1.9550 | 4.4780 | 0.9710 | 4.4780 (5) | 4.5694 (4.9) | 4.4 | 10.4200 |
| 150 | 0.30 | 1.9210 | 4.4160 | 0.9787 | 4.4160 (5) | 4.5061 (4.9) | 4.4 | 10.5200 |
| 200 | 0.40 | 1.8850 | 4.3500 | 0.9850 | 4.3500 (5) | 4.4388 (4.9) | 4.3 | 10.3200 |
| 300 | 0.60 | 1.8090 | 4.2100 | 0.9290 | 5.4850 (4) | 5.4850 (4) | 4 | 11.5600 |
| 400 | 0.80 | 1.7300 | 4.0640 | 0.9353 | 5.6575 (4) | 5.8026 (3.9) | 3.9 | 12.3400 |
| 500 | 1.00 | 2.3840 | 3.6100 | 0.9397 | 5.2975 (4) | 5.4333 (3.9) | 3.9 | 13.0000 |
| 600 | 1.20 | 1.8840 | 4.3540 | 0.9417 | 6.1775 (4) | 6.3359 (3.9) | 3.9 | 13.4500 |
| 750 | 1.50 | 3.5360 | 7.3180 | 0.9863 | 7.3180 (5) | 7.4673 (4.9) | 3.9 | 7.4360 |
| 1000 | 2.00 | 1.1350 | 0.9216 | 0.9147 | 7.8400 (4) | 8.8848 (4.6) | 3.9 | 6.8760 |

* **Offset 0 reproduces the coaxial values.** Fresh re-runs at 5, 10 and 3 mm returned the coaxial `a`
  byte-identically (3.889e-3, 7.642e-3, 2.518e-3). This is the walk's built-in control.
* **Any nonzero offset breaks the cone identity.** Even 25 mm (`x/r` = 0.05) reaches 4.5867 at 4.5 mm on the
  0.5 mm grid. The transition lies somewhere in (0, 25] mm; it was not walked.
* **Three regimes in `d`** on every nonzero offset (all **measured**): a **floor** at coarse `d`
  (`a/d` ≪ 1 trivially; e.g. S1 at 100, 50 and 20 mm all return 2.910e-3); a **coarse-mesh regime** from a
  cliff at `d` ≈ 3.9–4.4 mm (the "lowest `d` with `a/d` > 1" column) up to ≈ 13 mm, in which `a` stays in
  ≈ 15–26 mm (up to 41 mm at 750 and 1000 mm) while `d` varies from 4 to 13 mm, so the ratio peaks at the
  cliff's lower edge (4.4–8.9 across the walked offsets); and a **fine-`d` regime** below ≈ 1 mm, next.
* **The `d` = 0.1 mm column is the largest in the table**: 10.23, 10.31, 10.42, 10.52, 10.32, 11.56, 12.34,
  13.00, 13.45 from 25 to 600 mm (one dip, 150 → 200 mm), then 7.436 and 6.876 at 750 and 1000 mm — against
  0.9098 at offset 0. The fine-`d` problem therefore **starts at the smallest offset walked**, an
  order-of-magnitude jump between offset 0 and 25 mm (`x/r` = 0.05), and the offset thereafter moves the
  *size* of the problem (`a` ≈ 1.0–1.35e-3 m throughout 25–600 mm), not its existence. One `d` only — no
  claim is made about where the true peak in offset lies. The leader below sits at `x/r` = 1.0, on the
  boundary of the planned range, not at an interior maximum.
* **The spellings without an offset do not show it.** At 0.1 and 0.08 mm S2 reads 1.3620 and 1.4888, S3
  1.9410 and 1.2900, S4 0.9707 and 0.9695 — no fine-`d` spike on these rungs. Two rungs each are not
  a walk, and S2 and S3 already exceed 1.

**Fine `d` is not a regime but a sawtooth with isolated spikes.** Offsets 400 and 500 were walked to the
finest rungs the budget allows. Offset 400:

| d | a (m) | a/d | wall |
|---|---|---|---|
| 1 mm | 9.923e-4 | 0.9923 | 12 s |
| 0.95 mm | 1.511e-3 | 1.5905 | 14 s |
| 0.9 mm | 8.843e-4 | 0.9826 | 15 s |
| 0.85 mm | 1.246e-3 | 1.4659 | 18 s |
| 0.8 mm | 1.748e-3 | 2.1850 | 14 s |
| 0.75 mm | 1.624e-3 | 2.1653 | 14 s |
| 0.7 mm | 1.597e-3 | 2.2814 | 16 s |
| 0.65 mm | 1.903e-3 | 2.9277 | 18 s |
| 0.6 mm | 1.412e-3 | 2.3533 | 16 s |
| 0.55 mm | 1.313e-3 | 2.3873 | 18 s |
| 0.5 mm | 1.176e-3 | 2.3520 | 18 s |
| 0.45 mm | 1.973e-3 | 4.3844 | 25 s |
| 0.4 mm | 8.441e-4 | 2.1103 | 25 s |
| 0.35 mm | 1.158e-3 | 3.3086 | 29 s |
| 0.3 mm | 2.352e-3 | 7.8400 | 29 s |
| 0.25 mm | 8.132e-4 | 3.2528 | 36 s |
| 0.2 mm | 4.615e-4 | 2.3075 | 53 s |
| 0.15 mm | 4.470e-4 | 2.9800 | 66 s |
| 0.14 mm | 8.095e-4 | 5.7821 | 86 s |
| 0.13 mm | 1.297e-4 | 0.9977 | 98 s |
| 0.12 mm | 3.199e-4 | 2.6658 | 141 s |
| 0.11 mm | 1.007e-3 | 9.1545 | 180 s |
| 0.1 mm | 1.234e-3 | 12.3400 | 92 s |
| 0.09 mm | 1.267e-3 | 14.0778 | 225 s |
| 0.08 mm | 1.219e-3 | 15.2375 | 142 s |
| 0.079 mm | 1.165e-3 | 14.7468 | 117 s |
| 0.078 mm | 1.189e-3 | 15.2436 | 113 s |
| 0.077 mm | 1.264e-3 | 16.4156 | 116 s |
| 0.076 mm | 1.232e-3 | 16.2105 | 118 s |
| 0.075 mm | 1.203e-3 | 16.0400 | 124 s |
| 0.074 mm | 1.203e-3 | 16.2568 | 130 s |
| 0.073 mm | 1.183e-3 | 16.2055 | 128 s |
| 0.072 mm | 1.147e-3 | 15.9306 | 130 s |
| 0.071 mm | 1.165e-3 | 16.4085 | 128 s |
| 0.07 mm | 5.174e-4 | 7.3914 | 256 s |
| 0.06 mm | 4.517e-4 | 7.5283 | 353 s |
| 0.05 mm | 3.449e-4 | 6.8980 | 375 s |

and offset 500 from 0.1 mm down:

| d | a (m) | a/d | wall |
|---|---|---|---|
| 0.1 mm | 1.300e-3 | 13.0000 | 186 s |
| 0.09 mm | 1.328e-3 | 14.7556 | 120 s |
| 0.08 mm | 1.216e-3 | 15.2000 | 113 s |
| 0.07 mm | 1.226e-3 | 17.5143 | 133 s |
| 0.069 mm | 1.364e-3 | 19.7681 | 147 s |
| 0.068 mm | 1.348e-3 | 19.8235 | 157 s |
| 0.067 mm | 4.736e-3 | 70.6866 | 166 s |
| 0.066 mm | 1.339e-3 | 20.2879 | 162 s |
| 0.065 mm | 9.145e-4 | 14.0692 | 242 s |
| 0.064 mm | 3.995e-3 | 62.4219 | 237 s |
| 0.062 mm | 7.216e-4 | 11.6387 | 240 s |
| 0.06 mm | 4.128e-4 | 6.8800 | 273 s |

What the rows show (**measured**):

* **No trend toward 1 as `d` falls.** At offset 400 every one of the fifteen 0.05 mm rungs from 0.1 to
  0.8 mm reads ≥ 2.1; `a` hovers at 0.4–2.4 mm over that whole span while `d` falls eightfold.
* **The ratio rises through the K = 16 limit of §3.1 before the spike.** Offset 400 reads above 16 (16.04–16.42)
  at six of the seven `d` in [0.071, 0.077] mm (0.072 mm reads 15.93); offset 500 reads 17.5 at 0.07 mm and
  19.8 at 0.068 mm.
* **Spikes.** Offset 500 returns `a` = 3.995e-3 at 0.064 mm (62.42) and `a` = 4.736e-3 at 0.067 mm (70.69)
  against 0.7–1.4e-3 at the neighbouring rungs; 0.0664 mm returns 2.997e-3 (45.14). A spike is a plateau of `a` a
  few 1e-4 mm wide, pinned next.
* **Isolated lows too.** Offset 400 reads 0.9977 at 0.13 mm, between 2.67 (0.12) and 5.78 (0.14).
* **It is expensive.** The finest rungs cost minutes each under load — offset 400 at 0.05 mm took
  375 s, offset 500 at 0.06 mm took 273 s and at 0.025 mm (a halving-chain member, §3.1) 602 s — against 3.9 s for the *coaxial* loft at 0.1 mm and
  92 s for offset 400 at the same 0.1 mm. (Two contended wall clocks: indicative, per §0 Caveat 1.)

*Hypothesis (not established by these samples):* the offset spelling's lateral face is a non-analytic
(B-spline) surface for which the tessellator's refinement decision flips at isolated `d`; when it declines
to refine, one coarse patch sets the global maximum. Distinguishing that from any other reason `a` is
piecewise-constant would need per-face facet counts, which `reify check` does not expose.

**The headline datum: pinning the spike's edge.** Where `a` is constant on a plateau, `d` falling inside it
raises `a/d`, so the maximum sits at the plateau's **lower** edge (§1.5, #7128). The 4.736e-3 plateau was
walked at 0.0002, 0.00004 and 0.00001 mm steps, bracketed on both sides by probes returning a *different*
`a` (§4's rule: `a` is not monotone in `d`):

| d | a (m) | a/d | runs (byte-identical) | note |
|---|---|---|---|---|
| 0.066 mm | 1.339e-3 | 20.2879 | 2 |  |
| 0.0662 mm | 1.339e-3 | 20.2266 | 2 |  |
| 0.0664 mm | 2.997e-3 | 45.1355 | 2 |  |
| 0.0666 mm | 1.291e-3 | 19.3844 | 2 |  |
| 0.0668 mm | 1.329e-3 | 19.8952 | 2 |  |
| 0.06684 mm | 1.329e-3 | 19.8833 | 2 |  |
| 0.06688 mm | 1.329e-3 | 19.8714 | 2 |  |
| 0.06692 mm | 1.329e-3 | 19.8595 | 2 |  |
| 0.06693 mm | 1.329e-3 | 19.8566 | 2 |  |
| 0.06694 mm | 4.736e-3 | 70.7499 | 2 | **spike plateau** |
| 0.06695 mm | 4.736e-3 | 70.7394 | 2 | **spike plateau** |
| 0.06696 mm | 4.736e-3 | 70.7288 | 2 | **spike plateau** |
| 0.067 mm | 4.736e-3 | 70.6866 | 3 | **spike plateau** |
| 0.06704 mm | 4.736e-3 | 70.6444 | 2 | **spike plateau** |
| 0.06708 mm | 4.736e-3 | 70.6023 | 2 | **spike plateau** |
| 0.06712 mm | 4.736e-3 | 70.5602 | 2 | **spike plateau** |
| 0.06716 mm | 4.736e-3 | 70.5182 | 2 | **spike plateau** |
| 0.0672 mm | 1.361e-3 | 20.2530 | 2 |  |
| 0.0674 mm | 1.361e-3 | 20.1929 | 2 |  |
| 0.0676 mm | 1.361e-3 | 20.1331 | 2 |  |
| 0.0678 mm | 1.348e-3 | 19.8820 | 2 |  |
| 0.068 mm | 1.348e-3 | 19.8235 | 2 |  |

The plateau's lower edge lies in **(0.06693, 0.06694] mm**, pinned to 1e-5 mm — below it `a` = 1.329e-3, at and above it
4.736e-3 — and its upper edge in (0.06716, 0.0672) mm, so its width lies in (0.00022, 0.00027) mm. Across
the plateau the ratio falls monotonically with `d`, from 70.7499 to 70.5182, exactly as the plateau
model predicts. The headline is the lowest `d` that was *measured* on the plateau, **0.06694 mm**, where
`a/d` = **70.7499**; the edge is pinned to 1e-5 mm, and by the printed `a` alone
the ratio at the true edge lies in [70.7499, 70.7605).

*Derived.* `a` prints as `4.736e-3`, so the true value lies in [4.7355e-3, 4.7365e-3) m; `d` is exact (it
is the *request*). The true ratio at 0.06694 mm therefore lies in **[70.742, 70.757)**. No
display-wall argument is needed to establish the finding: even the lower end is 4.4× the K = 16 limit.
n = ⌈log₂ K⌉ = ⌈6.14⌉ = **7** (derived; §3.1).

**What this is, and is not.** 70.7499 is the largest `a/d` found anywhere, and it is a **lower bound**:

* *No turnover.* §1.5's criterion for a `supremum` (from #6545's review) is a fall that exceeds the
  oscillation amplitude seen elsewhere in the same ladder. Here that amplitude is itself 3.5× across a
  single 0.00004 mm step (4.736e-3 at 0.06716 mm, 1.361e-3 at 0.0672 mm), and the offset-500 ladder runs
  6.88 → 62.42 across 0.06 → 0.064 mm. Low readings at the finest rungs (6.90 at 0.05 mm on offset 400; 8.20 at 0.05 mm and 15.52 at 0.025 mm on
  offset 500) are therefore not a turnover, and none is, or could be, demonstrated.
* *Budget wall.* The finest rung measured, 0.025 mm on offset 500 (a halving-chain member, §3.1), cost
  602 s and read 15.52 — below the spikes above it; the next halving would cost ≈ 1200 s
  (**derived**, §2.1's 1/deflection scaling). This class is budget-limited at fine `d`, like sweep, pipe and
  spline and unlike nurbs_surface, and what lies below 0.025 mm is unknown.
* *One coordinate of many.* Walked: five spellings and one offset coordinate. **Not** walked: top radius
  (250 mm throughout), height (800 mm), a y-offset or rotation of the top section, section count beyond
  three, section shape beyond circle/ellipse/rectangle. The leader sits on the planned range's boundary.
* *Dense search cannot prove a supremum over a continuum* (§1.5), and this class is not a plateau ladder
  but a plateau *forest*: the spike at 0.067 mm was found by a 0.001 mm walk and does not appear on a
  0.01 mm grid.

The coaxial and the offset spellings share one fixture and nothing else; §3.1 carries the offset spelling's
number, because it is the one the PRD's loop would meet on an author's first non-trivial loft.

**Determinism and datum gates.** The harness extracts `a` only from the full line `sampled facet deviation
<X> m exceeds bound 1.000e-6 m` and emits distinct `NO-DATUM` / `TIMEOUT` / `SED-FAILED` tokens (§4's
recipe, rewritten in Python so the subject swap can assert exactly-once; each scratch `.ri` is regenerated
from a `git show HEAD:` snapshot of the committed fixture, one parent directory per probe, basename kept).
It was checked against live failures before any datum was trusted, and every arm fired: the same-plane
subject `loft(circle(500mm), circle(250mm))` — §0 Caveat 2's shape exactly: `OK PnrgLoftCheck#constraint[0]`,
`All constraints satisfied.`, **exit 0** — and a scratch file whose basename does not match its `module`
(`E_MODULE_PATH_MISMATCH`, **exit 1**) both returned `NO-DATUM`; an anchor not present exactly once returned
`SED-FAILED` without running `reify`; `timeout 1` returned `TIMEOUT` (rc 124). The positive control — the
committed subject at 20 mm — read 1.156e-2. **No measurement run produced a token**: every one of the
1778 runs below emitted the datum line, so neither a non-realization nor a timeout is hiding in any
table.

Re-runs: the five gate rungs, the three offset-0 control rungs, **every rung whose ratio is ≥ 2** and each
spelling's highest rung were run again — 507 probes compared (any probe with two or more runs),
**507 byte-identical, 0 divergent**. Not all 507 are determinism picks: for 470 every repeat is a
determinism re-run, for 20 the only repeat is a halving-chain re-reach (§3.1) of a probe an earlier walk
had already measured, and 17 carry both. The headline plateau was run three times at 0.067 mm and the 0.064 mm spike twice; both sides
of the pinned edge were re-run. A plateau edge is where a facet count changes, so it is where a
non-deterministic tie-break would surface if one existed; none did. Wall clocks varied with load, no
achieved value did (§0 Caveat 1).

**Deviation from §4, disclosed.** §4 prescribes `timeout 240`. The finest off-axis rungs would have been
killed by it (offset 500 at 0.025 mm took 602 s, offset 400 at 0.05 mm 375 s), turning a datum into a `TIMEOUT` — a cost result,
which is exactly what this class's budget-limited status should not be allowed to hide. `timeout 240`
was used for the spines, the grids and the 0.05 mm-step walk. **Every off-axis rung finer than 0.1 mm**, and
every halving-chain member (§3.1), ran under `timeout 600` (the lazily evaluated deep chains under 1500 s).
None hit its limit.

**Totals across the block:** **1778 runs over 1236 probes** (a probe is one subject at one
`d`), covering 274 distinct `d`; 542 of the runs repeat a probe already run — 505 as determinism
re-runs and 37 as halving-chain members (§3.1) that an earlier walk had already measured. Neither
tally should be read as the other, and several `d` recur across stages and spellings. The 542 repeats
fall on the 507 probes compared above: 489 ran twice, one three times and 17 four times
(489 + 2 + 3 × 17 = 542). Every run emitted the datum line;
not a single `OK`, `INDETERMINATE`, `NO-DATUM` or `TIMEOUT` occurred in a measurement run (the four
non-datum results in the raw log are the deliberate validation runs above). No achieved value differed
between repetitions anywhere. Finest `d` probed: 0.025 mm; most expensive run: 602 s. The raw log records **completed** runs only: probes
in flight when the orchestrator restarted mid-session were killed and are not counted; every one was re-run.

**History.** Until task **#6188** this section read "Loft is unreachable from the source language" and
recorded two failure modes. That account is superseded by #6188 (ticket `tkt_0RS9VJ0K316S7TBYJBDMPVTCY0` → task 6188, merge
`fca4a9ad5f`):

* The compile-time rejection of a `translate`d profile (`geometry argument 'profile' must be a 2D Surface
  profile (Closed, Planar)`), measured as of §0's HEAD `5db884e30b`, is fixed by #6188. Realizability of the
  translate spelling is now pinned by `crates/reify-eval/tests/harness_sweep/loft_e2e.rs`. Profile
  constructors still take no plane argument; sections are placed with `translate()`.
* The **same-plane** failure's cause, as measured 2026-08-18 and recorded in #6318's description and in
  #6188's commit `164b5d2818`, was `OperationFailed("OCCT loft_profiles: TopoDS::Wire")` — a shape-type
  mismatch (a face section where the OCCT call requires a wire; #6188's commit `2fb3eceb48` routes loft
  sections through the shared reducer `section_profile_to_wire`, via `add_loft_section`) that **never
  reached geometric evaluation**. (Quoted as recorded; the
  current binary no longer reproduces it, so it was not re-measured.) The earlier text's claim that
  coincident profiles "bound a degenerate zero-height solid, so this is the expected geometric outcome, not
  a kernel defect" is **retracted: it was measured wrong**, and must not be cited.
* What the same-plane subject does **on this session's binary** (**measured**, verbatim). `reify check`
  prints, and **exits 0**:

      warning: constraint expression has type PnrgLoft, expected Bool
        OK PnrgLoftCheck#constraint[0]
      All constraints satisfied.

  and `reify build --verbose` prints, also exiting 0:

      warning: constraint expression has type PnrgLoft, expected Bool
        INDETERMINATE PnrgLoftCheck#constraint[0]: undefined inputs: PnrgLoftCheck.subject
      warning: constraint PnrgLoftCheck#constraint[0] indeterminate: undefined inputs: PnrgLoftCheck.subject
        PnrgLoft#realization[0]: kernel: occt, repr: BRep
      …
      No constraints violated (1 indeterminate).

  That is a live instance of §0 Caveat 2's `OK` trap — no datum, no error diagnostic — and the reason every
  probe above gates on the deviation line rather than on the exit code. *Hypothesis (not established; the
  cause was not investigated):* the kernel call still fails for coincident sections and the failure is
  swallowed into an undefined `subject` rather than reported; the `realization[0]` line shows only that a
  realization was attempted.

---

## 2. Cost: tessellate vs measure

### 2.1 PRD §2.2 re-baselined on this machine

§2.2's table came from a different machine and era, so it was re-measured here before
any cost claim was built on it. Method: 5 reps **interleaved** across the ladder (one rep
of every rung, then the next) so a load excursion hits all rungs alike.

| row | §2.2 | here (median) | min–max | norm |
|---|---|---|---|---|
| sphere+bound `check` @0.6 mm | 4.92 s | 5.45 s | 4.79–6.75 | 1.11× |
| sphere+bound `check` @0.3 mm | 10.64 s | 11.66 s | 9.72–12.28 | 1.10× |
| sphere+bound `check` @0.15 mm | 19.96 s | 21.42 s | 19.71–23.58 | 1.07× |
| same module `build --verbose` | 0.37 s | 0.36 s | 0.25–0.49 | 0.97× |
| sphere, **no** bound, `check` | 0.35 s | 0.26 s | 0.22–0.27 | 0.74× |
| two spheres, **one** bound @0.3 mm | 19.21 s | 20.12 s | 18.50–20.97 | 1.05× |

* **1/deflection scaling reproduced**: 0.6→0.3 mm costs 2.03× (min) / 2.14× (med);
  0.3→0.15 mm costs 2.03× (min) / 1.84× (med).
* **Per-`Engine` `capture_repr_tol` waste reproduced**: two spheres with only *one*
  bound cost 1.90× (min) / 1.73× (med) the single-sphere pass — ~8.5 s of pure waste at
  0.3 mm. (§2.2 saw 1.8×.) Task **δ** reclaims it.
* **The metric dominates the rest of the pipeline**: 11.66 s vs 0.36 s = **32×**.

**Normalization: ~1.1×.** On the measurement-dominated rows this machine is 1.05–1.11×
slower than §2.2's — the *same machine class*. §2.2's numbers are quotable here after
that factor.

> A pre-session premise held that §2.2 could not be quoted at all, resting on a sphere
> `build --verbose` measured at 1.7 s against §2.2's 0.37 s. **That 1.7 s was a cold
> run.** Warm best-of-3 gives 0.25–0.49 s, reproducing §2.2 within 3%. The divergence was
> cold-start, not machine. Corrected here because the note is supposed to be measured.

### 2.2 Method: the three-vector differential

```
T_build = reify build --verbose <f>    parse + compile + eval + B-rep
T_stl   = reify build -o x.stl  <f>    + tessellate + STL write
T_check = reify check           <f>    + tessellate + measure
```

`build` never calls `set_capture_repr_tol`, so `achieved_repr_tol` stays empty and no
measurement runs on either build vector (PRD §3.2). That makes `T_stl` a clean
tessellate-only vector.

`build -o *.stl` ignores `#precision` and tessellates at a hardcoded 0.1 m
(`DEFAULT_STL_TESSELLATION_TOLERANCE` — that defect belongs to task 6085). Useless as a
*precision* probe; sound as a *cost* vector, because it tessellates at a **known fixed**
deflection. `pnrg_cost_split_sphere.ri` therefore pins `#precision(100mm)` = 0.1 m to
match that constant exactly and sweeps only the **radius**, so both vectors sit at the
same `d/R`.

### 2.3 Mesh identity — checked, not assumed

The subtraction is meaningless unless both vectors tessellate the same mesh. Facet count
is not observable through the check vector, so the test used the mesh itself: the max
4-point sampled chord deviation was **recomputed independently from the exported STL's
own triangles** and compared against the engine's reported achieved deviation.

| R | tris | STL-derived | engine reported |
|---|---|---|---|
| 500 mm | 304 | 3.0032e-2 | 3.003e-2 |
| 1000 mm | 304 | 6.0064e-2 | 6.006e-2 |
| 2000 mm | 304 | 1.2013e-1 | 1.201e-1 |
| 4000 mm | 434 | 1.1741e-1 | 1.174e-1 |

Agreement to 4 s.f. at every radius — **including across the 304→434 facet-count
staircase step, where the deviation drops non-monotonically and both vectors show the
drop**. The premise holds; the subtraction in §2.4 is licensed. (This also independently
reproduces PRD §2.1's 4-interior-sample-point metric, and re-verifies §2's exact `d/R`
scale invariance: 3.0032 : 6.0064 : 1.2013e-1 = 1 : 2 : 4.)

### 2.4 The split

Measured, three reps at the two largest points:

| R (mm) | tris | T_build | T_stl | T_check |
|---|---|---|---|---|
| 4000 | 434 | 0.32 | 0.30 | 0.41 |
| 16000 | 1640 | 0.33 | 0.28 | 0.71 |
| 64000 | 6484 | 0.30 | 0.35 | 2.11 |
| 256000 | 25774 | 0.30 / 0.45 / 0.48 | 2.42 / 0.81 / 1.52 | 10.75 / 9.17 / 11.39 |
| 1000000 | 100518 | 0.97 / 0.40 / 0.34 | 4.96 / 7.76 / 5.76 | 41.67 / 31.82 / 42.86 |

**STL-write confound, bounded not ignored** (measured): writing 5.03 MB — exactly 100518
binary STL triangles — to tmpfs took 0.010–0.063 s, median 0.014 s. So `T_write ≤ 0.06 s`
at the largest point: under 2% of `T_stl`, under 0.2% of `T_check`. It is carried as an
explicit term and changes no conclusion.

**Derived** (arithmetic on the rows above, not separate observations):

```
tessellate ≈ T_stl   - T_build - T_write
measure    ≈ T_check - T_stl   + T_write
```

| point | tessellate | measure | ratio |
|---|---|---|---|
| 100518 facets, min-of-3 | 4.56 s | 26.9 s | 5.9× |
| 25774 facets, min-of-3 | 0.50 s | 8.4 s | 17× |
| 25774 facets, max `T_stl` | 2.11 s | 6.8 s | 3.2× |

Per-facet (derived): **measure ~0.27–0.33 ms/facet** (consistent across both large
points); **tessellate ~0.02–0.08 ms/facet** (noisy — `T_stl` varied 0.81–2.42 s at one
point under load).

> **Measurement dominates tessellation by roughly one order of magnitude** — bracket
> 3×–17× across reps, best estimate 6–10×. Equivalently, of the tessellate+measure block
> (`T_check − T_build` = 31.5 s at 100518 facets) roughly **85–90% is measurement**.
> Quoted to the precision the data supports and no further.

> **Correction to a prior expectation.** Session evidence suggested *two* orders of
> magnitude, from a sweep whose tessellation took 0.79 s while measurement exceeded 60 s.
> Those figures were **not mesh-matched** — the STL vector tessellated at a hardcoded
> 0.1 m while the check vector tessellated at the requested `#precision`, a far finer
> mesh. On a properly mesh-matched subject the gap is ~1 order of magnitude, not 2.

---

## 3. `REFINE_ATTEMPT_CAP`

### 3.1 Convergence

With `achieved ≤ K·d`, reaching `achieved ≤ B` from `d0` needs `n ≥ log2(K·d0/B)`; for
the natural authoring case `B ≈ d0`, `n ≥ log2(K)`. **Derived** from the §1 suprema:

| class | sup K | required n |
|---|---|---|
| loft | ≥ 70.75 ‡ | ≥ **7** ‡ |
| sphere | 2.079 | **2** |
| torus | 0.978 | 0 |
| cone | 0.970 | 0 |
| fillet blend | 0.925 | 0 |
| nurbs surface | ≥ 1.0010 † | 1 † |
| sweep / pipe / spline | ≤ 0.598 * | 0 * |

**One measured class exceeds K = 16: loft.** Its off-axis spelling reads **≥ 70.75** (§1.1,
§1.6), so `n ≥ ⌈log2 70.75⌉ = ⌈6.14⌉ =` **7** (derived) — above the cap of 4, and a lower
bound, so the true `n` can only be higher. Every other class stays at or below the sphere's
2.079, needing `n = 2`.

Cap 4 covers K up to 16 at `B = d0`. Against the sphere — the worst class before loft had a
datum — that was **7.7× headroom** (16 / 2.079). Against loft it is a **shortfall**:
16 / 70.75 = 0.23, i.e. K exceeds what cap 4 covers by at least **4.4×** (derived), and K is
a floor. §3.3 is where that lands.

Of the other classes, nurbs_surface is the one measured **above** the K = 1 boundary, and so
the one whose `required n` is not 0: its best pinned value is 1.0010 (§1.1, §1.5), and the
true ratio at that `d` is bounded in [1.000626, 1.001321) — an interval lying entirely above
1, so the crossing is established rather than merely not excluded. Per the † note below that
value is still a lower bound, so the true supremum can only be higher. Even so this changes
nothing at the cap level: `n = 1` is nowhere near the cap-4 budget, and no plausible reading
of this class's data approaches K ≈ 16 — all 69 probes of its dense walk lie within
[0.9623, 1.0010], with no second branch like the sphere's ~2.07 tread.

\* Lower bounds only. The fine-`d` regime where the sphere reached its supremum was
unaffordable for these three classes (§1.5, §2.1 caveat 1). The cap is justified by
**headroom**, not by a claim of exhaustive coverage.

† Lower bound for a different reason than the row above: nurbs_surface is not
budget-limited (§1.5) — every rung tried completed well under the 90 s wall: down to
0.1 mm on 6545's ladder, and across all 95 runs of #7128's block. Task #7128
resolved the oscillation an earlier amendment left open (no period; `a` piecewise-constant
on plateaus ~1e-4 mm wide; the ratio peaking at each plateau's lower edge) and pinned
**1.0010 at `d` = 0.14386 mm**, which crosses the K = 1 boundary and is what moves
`required n` from 0 to 1. The value stays a lower bound — three plateau edges of very many
were pinned (a fourth plateau was walked only as a control), and a dense search cannot
prove a supremum over a continuum — so the true K can only be *higher* than 1.0010. That
does not disturb the cap: `n = 1` is nowhere near the cap-4 budget, and no plausible
reading of this class's data approaches K ≈ 16.

‡ Lower bound for the reasons §1.6 gives: `a` is piecewise-constant in `d` with isolated
spikes, the walk stopped where each rung costs minutes, and one regime coordinate of many was
walked. `n = ⌈log2 70.75⌉ = 7` inherits the floor. The coaxial spelling is not this row: it
is the cone class (§1.6), `n = 0`.

**Cross-check: the K-bound model against halving chains (task #6318, measured).** §3.1's
`n ≥ log2 K` says what a loop needs *if* `K` bounds `a/d` everywhere, so it is a worst case over `d` — and
loft is the one class whose `a` is wildly non-monotone in `d`. The model was therefore tested against the loop
it models. For each starting request `d0`, with `B = d0` (the natural authoring case), the chain
`d0, d0/2, d0/4, …` was measured at the two leading x-offsets of §1.6 (500 and 400 mm): `n` is the first
attempt `i` with `a_i ≤ B`, and cap 4 allows `i` ≤ 4. A cell is `a_i / B` — above 1 is a failed attempt — and
a chain stops at its first pass, as the loop would. *Derived:* `a_i ≤ B` is the same as `a_i / d_i ≤ 2^i`, so
attempt `i` passes exactly when the measured ratio at `d0/2^i` is at most 1, 2, 4, 8 or 16; the cap is
exhausted only if the ratio exceeds **every** one of them, and the last threshold is §3.1's K = 16. 46
chains (23 starting points × 2 offsets):

| d0 (= B) | x = 500: n | a_i / B | x = 400: n | a_i / B |
|---|---|---|---|---|
| 20 mm | **0** | 0.15 | **0** | 0.15 |
| 16 mm | **0** | 0.18 | **0** | 0.18 |
| 12 mm | **2** | 1.99 → 2.17 → 0.23 | **2** | 1.35 → 2.09 → 0.23 |
| 10 mm | **2** | 2.38 → 1.81 → 0.25 | **2** | 1.73 → 2.03 → 0.24 |
| 8 mm | **2** | 2.98 → 2.65 → 0.44 | **2** | 2.16 → 2.83 → 0.25 |
| 6.4 mm | **1** | 3.96 → 0.47 | **1** | 2.44 → 0.47 |
| 5 mm | **1** | 3.61 → 0.49 | **1** | 4.06 → 0.49 |
| 4 mm | **1** | 5.30 → 0.88 | **1** | 5.66 → 0.51 |
| 3.2 mm | **0** | 0.94 | **0** | 0.93 |
| 2.5 mm | **0** | 0.99 | **0** | 0.97 |
| 2 mm | **1** | 1.76 → 0.50 | **1** | 1.02 → 0.50 |
| 1.3 mm | **0** | 0.98 | **0** | 0.99 |
| 1.2 mm | **3** | 3.38 → 1.63 → 1.69 → 0.37 | **3** | 3.66 → 1.18 → 1.96 → 0.37 |
| 1.1 mm | **2** | 4.00 → 1.75 → 0.76 | **2** | 3.45 → 1.19 → 0.78 |
| 1.05 mm | **2** | 5.97 → 1.34 → 0.98 | **2** | 4.34 → 1.39 → 0.37 |
| 1 mm | **0** | 0.99 | **0** | 0.99 |
| 0.9 mm | **4** | 1.05 → 1.65 → 1.98 → 1.11 → 0.52 | **0** | 0.98 |
| 0.8 mm | **4** | 1.52 → 1.20 → 5.48 → 1.63 → 0.51 | **2** | 2.19 → 1.06 → 0.58 |
| 0.7 mm | **1** | 2.40 → 0.50 | **2** | 2.28 → 1.65 → 0.58 |
| 0.64 mm | **1** | 3.61 → 0.49 | **2** | 1.53 → 1.27 → 0.64 |
| 0.6 mm | **2** | 3.26 → 3.39 → 0.74 | **2** | 2.35 → 3.92 → 0.75 |
| 0.5 mm | **2** | 3.01 → 1.99 → 0.31 | **2** | 2.35 → 1.63 → 0.72 |
| 0.4 mm | **4** | 2.40 → 10.97 → 3.25 → 1.03 → 0.97 | **3** | 2.11 → 1.15 → 3.09 → 0.86 |

*Measured:* 46 of 46 chains converged within the cap (n = 0: 13, n = 1: 10, n = 2: 17, n = 3: 3, n = 4: 3; worst n = 4).

* **The cap held on every chain measured — and is tight, not slack.** No chain exhausted the cap: every chain measured to completion converged, 3 of them only at the last attempt the cap allows (x = 500 mm, d0 = 0.9 mm; x = 500 mm, d0 = 0.8 mm; x = 500 mm, d0 = 0.4 mm), and the tightest passed its last attempt with `a/d` = 15.5 against the limit of 16 (x = 500 mm, d0 = 0.4 mm). 6 of the
  46 chains needed 3 or 4 attempts, and every one of them starts between 0.4 and
  1.2 mm.
* **Those are the chains whose last members land in §1.6's fine regime.** Attempt 4 runs at `d0/16`, which for
  `d0` = 0.4–1.2 mm is 0.025–0.075 mm — where §1.6 measured `a/d` between 6.88 and 70.75 and the chains' own last
  members read 8.2, 8.3 and 15.5. The threshold there is 16, so each of these chains asks whether the
  ratio at `d0/16` is above or below 16, in the regime where it is least predictable.
* **`n(d0)` is erratic in `d0`, not a trend.** At x = 500 mm, `d0` = 0.9, 0.8, 0.7, 0.64 and 0.6 mm give
  `n` = 4, 4, 1, 1 and 2: whether a spike lands on one of a chain's five members changes the verdict, so no
  interpolation between the measured `d0` is licensed.
* **What the chains do not show.** They sample 23 starting points at two offsets of one spelling; they
  neither prove nor bound a supremum. Chains from `d0` < 0.4 mm are **unmeasured**: their last members sit below
  0.025 mm, where no ratio has been measured. The 0.025 mm member cost 602 s under load, so the last
  member of a `d0` = 0.2 mm chain, at 0.0125 mm, would cost ≈ 1200 s (**derived**, 1/`d` scaling, §2.1).

### 3.2 Cost

Cost scales as 1/deflection (§2.1, re-measured: 2.03× per halving) and bisection halves
`d` per attempt, so attempts cost 2×, 4×, 8×, 16× the base pass and a capped run pays
**all** of them — **derived**:

```
Σ_{i=1..N} 2^i  =  2^(N+1) − 2  =  30× base at N = 4,  31× including the initial pass
```

> PRD §4.2's "≤ ~16× the measured per-pass cost" is arithmetically the **final pass
> alone**. The cost of a capped **run** is ~2× that. This corrects the figure; it does
> not challenge §4.2's reasoning, which is unchanged.

Worst-case wall clock on the re-baselined sphere (11.66 s at 0.3 mm, median), both rows
counting the initial pass so they are comparable:
**N = 3 → 15× → ~2.9 min**; **N = 4 → 31× → ~6.0 min**.

### 3.3 Decision — keep the cap at 4: provisional, with an open question for loft

Task #6318's loft datum (§1.6) breaks the arithmetic this section was built on. This note does **not**
replace the decision with a new one: it records the finding, escalates it, and leaves the cap, PRD §4.2 and
§3.4 as they were.

*What changed* (**derived** from §1.6's measured lower bound). Cap 4 covers `K` up to 16 at `B = d0`. Loft's
off-axis spelling measures `K` ≥ 70.75, so `n ≥ 7` and the coverage falls short of the worst measured class
by at least 70.75 / 16 = **4.4×** — where this section used to report **7.7× headroom** (16 / 2.079, against
the sphere). `K` is a lower bound, so the shortfall can only be larger. Under §3.1's model, cap 4 **cannot be
shown to reach `B = d0`** for that class.

*The earlier argument, and what became of it.* "Buys over 3: K headroom 16 vs 8, i.e. 7.7× vs 3.8× over the
worst measured class" was arithmetically right for the classes then measured. The insurance it argued for —
"the extra doubling is still cheap insurance against classes this session could not fully pin down" — named
the risk that materialised: loft was the one class not yet measured, and its first datum is 34× the
previous worst. The insurance was too small, not misplaced. (nurbs_surface, the other worked example, moved
from 0.9975 to 1.0010 on denser walking, and that still stands.)

*What the measurements say about the actual loop, and what they do not* (**measured**; §3.1's chains).
No chain exhausted the cap: every chain measured to completion converged, 3 of them only at the last attempt the cap allows (x = 500 mm, d0 = 0.9 mm; x = 500 mm, d0 = 0.8 mm; x = 500 mm, d0 = 0.4 mm), and the tightest passed its last attempt with `a/d` = 15.5 against the limit of 16 (x = 500 mm, d0 = 0.4 mm). So the cap held on every starting request sampled — but with no slack in the band
where `d0/16` lands in the fine regime, and the model's `n ≥ 7` is a bound over every `d` while a chain visits
five of them. Whether a real loop exceeds 4 attempts on a loft requested at `d0` < 0.4 mm, or at a `d0` the
walk did not sample, is **unmeasured, not shown safe**.

*What buying the model's guarantee would cost* (**derived**; §3.2's arithmetic is class-independent). A cap of
`N` costs 2^(N+1) − 1 base passes including the initial one: `N` = 5 → 63×, `N` = 6 → 127×, `N` = 7 → 255×,
about 12, 25 and 50 minutes on the re-baselined sphere's 11.66 s base pass (`N` = 4: 31×, ~6.0 min). Whether to
pay that, to scope the guarantee away from classes whose `a/d` no small K bounds, or to read the cap as a cost
bound only, is a design question for PRD §4.2's owner and task γ1. No measurement decides it; it is escalated
(`design_concern`) with this section's evidence.

*Costs at cap 4* (unchanged arithmetic): worst case ~6.0 min instead of ~2.9 min on the re-baselined sphere — a
worst case reached only when **every** attempt fails. Every measured class except loft converges at
`n = 0…2`; loft converged at `n ≤ 4` on every chain measured and has model `n ≥ 7`.

Framing is PRD §4.2's and is unchanged: the loop is a **safety net, not a search engine**. Neither the halving
factor nor the cap is a soundness constant — the verdict is always the measured one, and the cap bounds
**cost**, not correctness. That framing is why this is an open question about cost and coverage, not a defect
in a verdict.

### 3.4 The per-iteration cost sentence

For task **γ1** to quote directly:

> Each refinement attempt halves the requested deflection and therefore roughly **doubles
> the pass cost** (measured: 2.03× per halving), and about **85–90% of that cost is
> deviation measurement, not tessellation** (measured: ~0.3 ms per facet to measure
> against ~0.02–0.08 ms to tessellate); a run that exhausts `REFINE_ATTEMPT_CAP = 4`
> therefore costs **~31× a single pass** cumulatively — not 16×, which is the final
> attempt alone — or roughly **6 minutes** on a 1 m sphere requested at 0.3 mm.

---

## 4. Reproduction

Fixtures (committed as reproducibility artifacts; **deliberately not registered in any
probe set or auto-run gate** — runtimes range from 0.3 s to >90 s timeouts, and their
expected values are continuous measurements that drift with OCCT and machine):

| fixture | class |
|---|---|
| `tests/prd-gate/fixtures/pnrg_envelope_sphere.ri` | sphere — **anchor**, read its header first |
| `tests/prd-gate/fixtures/pnrg_envelope_cone.ri` | cone |
| `tests/prd-gate/fixtures/pnrg_envelope_torus.ri` | torus |
| `tests/prd-gate/fixtures/pnrg_envelope_fillet_blend.ri` | fillet blend |
| `tests/prd-gate/fixtures/pnrg_envelope_sweep.ri` | sweep |
| `tests/prd-gate/fixtures/pnrg_envelope_pipe.ri` | pipe |
| `tests/prd-gate/fixtures/pnrg_envelope_spline.ri` | spline |
| `tests/prd-gate/fixtures/pnrg_envelope_nurbs_surface.ri` | nurbs surface |
| `tests/prd-gate/fixtures/pnrg_envelope_loft.ri` | loft — off-axis spelling, pinned at the §1.6 headline rung; minutes per run, use `timeout 600` |
| `tests/prd-gate/fixtures/pnrg_cost_split_sphere.ri` | cost split |

A single probe:

```bash
timeout 90 ./target/release/reify check tests/prd-gate/fixtures/pnrg_envelope_sphere.ri
# valid ⟺ output contains: sampled facet deviation <X> m exceeds bound 1.000e-6 m
```

**A `d` ladder** (the `module` declaration must match the file's basename, so rewrite in
place under a same-named temp file — a bare `sed > /tmp/x.ri` fails with
`E_MODULE_PATH_MISMATCH`):

```bash
F=tests/prd-gate/fixtures/pnrg_envelope_sphere.ri
mkdir -p /tmp/pnrg && for d in 3mm 1mm 0.6mm 0.312mm 0.3mm; do
  sed -E "s/#precision\([^)]*\)/#precision($d)/" "$F" > "/tmp/pnrg/$(basename $F)"
  a=$(timeout 90 ./target/release/reify check "/tmp/pnrg/$(basename $F)" 2>&1 \
      | grep -oE 'deviation [0-9.e+-]+ m' | awk '{print $2}')
  echo "$d -> ${a:-NO-DATUM}"
done
```

**A regime walk** — same loop, with a second `sed` expression rewriting the constructor,
e.g. `s/torus\(1000mm, 100mm\)/torus(1000mm, 20mm)/`.

**A dense parallel walk** (task #7128) — what the plain ladder above does not cover. Three
things differ once probes run concurrently and the ratios are read to 4 dp:

```bash
F=tests/prd-gate/fixtures/pnrg_envelope_nurbs_surface.ri
# (1) Each d needs its own PARENT dir.  The basename must stay pnrg_envelope_<class>.ri
#     for the module-path rule above, so concurrent probes sharing one path clobber each
#     other.  Per §0 Caveat 1 ratios are load-invariant and exact — only wall clocks are
#     contended — so parallelism cannot corrupt the data, only its timings.  P=4; do not
#     raise it on a box already oversubscribed.
probe() {                            # probe <d>  ->  "<d> <a|NO-DATUM|TIMEOUT>"
  local d=$1 dir=/tmp/pnrg7128/$1 b; b=$(basename "$F")
  mkdir -p "$dir"
  sed -E "s/#precision\([^)]*\)/#precision($d)/" "$F" > "$dir/$b"
  grep -q "^#precision($d)\$" "$dir/$b" || { echo "$d SED-FAILED"; return 1; }
  local out rc a
  out=$(timeout 240 ./target/release/reify check "$dir/$b" 2>&1); rc=$?
  # (2) The failure tokens are FATAL for the row — never a number, never 0 — and they
  #     must not be merged, because a kill and a non-realization both leave the grep
  #     empty.  A §0 Caveat-2 non-realization exits 0 and prints no deviation line, so a
  #     harness that records the empty grep builds a table of confident false near-zero
  #     ratios.  A `timeout` kill (rc 124) is instead a COST result, and this class's
  #     "not budget-limited" claim (§1.5) is exactly what a merged token would hide.
  #     Capturing `out` first is what makes rc readable: inside a pipeline $? is the
  #     grep's, not reify's.
  [ "$rc" -eq 124 ] && { echo "$d TIMEOUT"; return 0; }
  a=$(printf '%s\n' "$out" | grep -oE 'deviation [0-9.e+-]+ m' | head -1 | awk '{print $2}')
  echo "$d ${a:-NO-DATUM}"
}
export -f probe; export F
printf '%s\n' 0.143mm 0.14386mm 0.144mm 0.145mm | xargs -P4 -I{} bash -c 'probe {}' \
  | sort -g | python3 ratio.py
```

```python
# ratio.py — (3) ratios via decimal.Decimal with ROUND_HALF_UP at 4 dp.
# awk '%.4f' is WRONG here: it rounds exact ties DOWN.  The 0.8 mm rung is a live
# example — 7.658e-4 / 8e-4 = 0.95725 exactly, which awk prints 0.9572 and this
# prints 0.9573.  One such cell needed its own fix commit in task 6545.
import sys
from decimal import Decimal, ROUND_HALF_UP
for line in sys.stdin:
    d, a = line.split()
    if a in ("NO-DATUM", "TIMEOUT", "SED-FAILED"):
        print(f"{d}\t{a}")
        continue
    ratio = Decimal(a) / (Decimal(d.rstrip("m")) / 1000)
    print(f"{d}\t{a}\t{ratio.quantize(Decimal('0.0001'), rounding=ROUND_HALF_UP)}")
```

To pin a plateau edge rather than grid the interval, bisect `d` downward from a candidate
to the largest `d` still returning the plateau's `a`, bracketing each edge with a probe on
*both* sides that returns a different `a` — `a` is not monotone in `d` (§1.5), so an
unbracketed bisection is unsound. ~10 probes pin one edge; a 1e-4 mm grid over
[0.12, 0.18] mm would need ~600.

*Checked.* The `probe()` block above was extracted from this file and run verbatim after
the fact — same lane, same binary, loadavg 149 — and printed `0.143mm 1.429e-4 0.9993`,
`0.14386mm 1.440e-4 1.0010`, `0.144mm 1.440e-4 1.0000`, `0.145mm 1.450e-4 1.0000` in 26.6 s
wall at P=4. §1.5's Stage C values therefore reproduce from the *published* recipe in a
later session, not merely from whatever was typed at the time. The `TIMEOUT` arm was
exercised by lowering `timeout 240` to `timeout 1` (prints `0.12mm TIMEOUT`, returns 0 so
`xargs` does not abort) and the `NO-DATUM` arm by the two failures §1.5 records.

**A loft subject swap** (task #6318) — sed is fragile for a subject full of parentheses and
commas, so the spellings and offsets of §1.6 are produced by an exactly-once Python rewrite of
the `let g = …` line. Anchoring on that line rather than on a spelling keeps the recipe valid
whichever spelling the fixture has committed (it is pinned at the offset-500 leader). The fixture
text comes from `git show HEAD:`, not the working tree, so an uncommitted edit cannot change the
apparatus — §1.6's harness likewise rewrote a snapshot of the committed fixture. Same
conventions as the dense walk above — one parent dir per probe, basename kept, `out` captured
before the exit code is read, distinct failure tokens:

```python
# loft_probe.py — run from the repo root.  probe(d_mm [, rhs]) -> "<a>" | NO-DATUM | TIMEOUT | SED-FAILED
import re, subprocess, tempfile
from pathlib import Path
F = "tests/prd-gate/fixtures/pnrg_envelope_loft.ri"
DATUM = re.compile(r"sampled facet deviation (\S+) m exceeds bound 1\.000e-6 m")

def probe(d_mm, rhs=None):
    src = subprocess.run(["git", "show", f"HEAD:{F}"],     # committed text, never the working tree
                         capture_output=True, text=True, check=True).stdout
    if rhs is not None:                      # replace the whole `let g =` line, exactly once
        src, n = re.subn(r"^(    let g = ).*$", lambda m: m.group(1) + rhs, src, flags=re.M)
        if n != 1: return "SED-FAILED"
    src, n = re.subn(r"^#precision\([^)]*\)$", f"#precision({d_mm}mm)", src, flags=re.M)
    if n != 1: return "SED-FAILED"
    path = Path(tempfile.mkdtemp(prefix="pnrg_loft_")) / Path(F).name
    path.write_text(src)
    p = subprocess.run(["timeout", "600", "./target/release/reify", "check", str(path)],
                       capture_output=True, text=True)
    if p.returncode == 124: return "TIMEOUT"
    m = DATUM.search(p.stdout + p.stderr)
    return m.group(1) if m else "NO-DATUM"      # `OK` and E_MODULE_PATH_MISMATCH land here

S1 = lambda x: f"loft(circle(500mm), translate(circle(250mm), {x}mm, 0mm, 800mm))"  # x = offset, mm
# probe(20, S1(0))      -> 1.156e-2   coaxial control (== cone(500mm, 250mm, 800mm))
# probe(5,  S1(200))    -> 2.175e-2   §1.6 reproduction gate
# probe(0.06694)        -> 4.736e-3   the committed fixture: the headline
```

The other spellings (S2–S4) are in the §1.6 table; pass each as `rhs`.

*Checked.* The block above was extracted from this file and run verbatim: `probe(20, S1(0))`
printed `1.156e-2`, `probe(5, S1(200))` printed `2.175e-2` and `probe(0.06694)` printed `4.736e-3`
(the last in 199 s, 1-min loadavg 125 at launch). The `HEAD:` read was checked with the working-tree
fixture deliberately dirty: each scratch copy carried the committed header, not the edited one. The
committed fixture, run
directly as `timeout 700 ./target/release/reify check
tests/prd-gate/fixtures/pnrg_envelope_loft.ri`, printed `error: RepresentationWithin: sampled
facet deviation 4.736e-3 m exceeds bound 1.000e-6 m for PnrgLoftCheck` and exited 1, in 249 s
wall at a 1-min loadavg of ≈ 150 — the §1.6 headline, string for string.

**The cost split** (three vectors; the STL path must go to tmpfs to keep the write term
bounded):

```bash
F=tests/prd-gate/fixtures/pnrg_cost_split_sphere.ri     # radius is the facet-count knob
time ./target/release/reify build --verbose      "$F"   # T_build
time ./target/release/reify build -o /dev/shm/t.stl "$F" # T_stl
time ./target/release/reify check                "$F"   # T_check
tris=$(( ($(stat -c %s /dev/shm/t.stl) - 84) / 50 ))    # binary STL triangle count
```

Re-run the §2.3 mesh-identity check before trusting any new split: recompute the max
4-point sampled deviation from the STL triangles and compare against the engine's
reported achieved value. If they diverge, the subtraction is invalid and the only
defensible statement is the bracket `measure ≥ T_check − T_stl`.
