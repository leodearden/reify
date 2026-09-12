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
construct that compiles but never realizes produces (loft, degenerate cone — §1.5,
§2.3).

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
| sphere | `d/R` | **2.079** | `d/R` = 3.12e-4 | supremum |
| torus | `minor/major`, `d/minor` | 0.978 | 0.02, 0.015 | supremum |
| cone | `top_r/bottom_r` | 0.970 | 0.8, `d/R` = 6e-4 | supremum |
| fillet blend | `fillet_r/feature` | 0.925 | 0.49, `d/R` = 6e-4 | supremum |
| nurbs surface | `d/span` † | **1.0010** | `d/span` = 1.4386e-4 | **lower bound**, `K` > 1 |
| pipe | pipe_r / path curvature | 0.598 | `d/R` = 5e-2 | **lower bound** |
| sweep | profile / path curvature | 0.534 | `d/R` = 1e-2 | **lower bound** |
| spline | profile / path curvature | 0.013 | `d/R` = 2e-2 | **lower bound** |
| loft | — | **no datum** | — | blocked at realization |

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
continuum, and only four plateau edges of the very many in [0.12, 0.18] mm were pinned.
So `lower bound` no longer means the structure is un-understood, and never meant a wall
was hit — it means 1.0010 is a floor that further walking can only raise.

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
sits inside the floor as well.
In the floor regime `a/d < 1` **trivially**, so a coarse sweep reports a falsely
comfortable envelope. This is the trap the non-analytic classes could not escape (§2.4).

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
* **loft** — blocked at realization, both failure modes below.

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
| **0.12 mm** | 1.197e-4 | **0.9975** | ← highest measured |
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
between adjacent 0.005 mm rungs reaches 0.0333 (0.9667 at 0.135 mm to 1.0000 at 0.145 mm).
Meanwhile the five leading values — 0.9967, 0.9969, 0.9972, 0.9975 and 1.0000 — are
separated from one another by as little as 0.0002. **The between-rung swing is two orders
of magnitude larger than the gaps between the candidates the sweep is trying to rank**, so
a 0.005 mm grid cannot establish which of them is the true local maximum, nor that any of
them is a local maximum at all: each is simply the largest value on whichever grid
happened to be sampled. This is the §1.2 aliasing trap in its exact form — the sphere's
branches alternate over ~0.006 mm, and a grid at that same order lands on one branch and
misses the other.

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

**P4 — `a` = 1.428e-4**, walked as a control. It is a *low* plateau, and it shows the
mechanism cleanly in the direction that does not flatter the result — `a` byte-identical
across six probes spanning 0.00014 mm while the ratio falls monotonically with rising `d`:

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
it — while **the numeric value 1.0010 remains a lower bound** on the true supremum. All 92
probes of the dense walk itself — Stage A onward, `d` ∈ [0.118, 0.182] mm — lie within
[0.9623, 1.0010], with no sign of a second branch like the sphere's ~2.07 tread, but that
is an observation about where these samples fell, not a bound on where others might. (The
block's three reproduction-gate runs sit outside that window by construction: the 20 mm
rung reads 0.8565, deep in the coarse regime.)

**The display-precision wall — and why this result clears it.** The achieved deviation is
formatted `{achieved:.3e}` at `crates/reify-eval/src/tolerance_combine.rs:460`, which is
the **only** production site that emits it (the other `.3e` occurrences under `crates/`
are test assertions, and `reify check`'s usage line offers no `--json` or `--verbose`
alternative: `reify check [--strict] [--purpose <name>=<binding>]... [--cfg
<key=value|flag>]... <file>`). Four significant figures is therefore the whole apparatus,
and it is a hard floor, not a convention this task could dial up.

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
number. That sentinel was checked against a live failure before use — a scratch file whose
basename does not match its `module` declaration exits **0** with `E_MODULE_PATH_MISMATCH`
and no deviation line, which is exactly the shape §0 Caveat 2 warns about, and the harness
reported `NO-DATUM` for it rather than an empty field.

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

*Totals across the block:* **95 runs over 65 distinct `d` values** — 3 reproduction-gate
runs, 19 in Stage A (13 rungs, 6 re-run), 33 in Stage B (25 rungs, 8 leaders re-run) and
40 in Stage C (31 probes, 9 edge probes re-run). Every one emitted the datum line; not a
single `OK`, `INDETERMINATE` or `NO-DATUM` occurred. No achieved value differed between
repetitions anywhere in the block, at any stage or resolution. Nothing timed out, so this
class remains **not budget-limited** at these `d` — the finest probe, 0.14305 mm, is well
inside the regime 6545 already showed to be affordable.
### 1.6 Loft is unreachable from the source language

Two mutually exclusive failure modes with no path between them:

1. Profiles in the **same plane** — the only thing the language can express — compile
   but never realize (`INDETERMINATE`). Reproduced for `loft(circle, circle)`,
   `loft(rectangle, circle)`, `loft(circle, ellipse)`, three-profile loft,
   `loft(rectangle, polygon)`, `loft_guided(…)`, and `translate(loft(…))`. Coincident
   profiles bound a degenerate zero-height solid, so this is the expected geometric
   outcome, not a kernel defect.
2. Separating the planes with `translate` is rejected at compile time:
   `error: geometry argument 'profile' must be a 2D Surface profile (Closed, Planar)`
   (`crates/reify-compiler/src/geometry.rs:729`, dispatched via
   `crates/reify-compiler/src/conformance/mod.rs:6158`).

**Root cause is structural, not a spelling problem**: every profile constructor is fixed
arity with no plane or offset argument — `circle(r)`, `rectangle(w,h)`, `ellipse(a,b)`,
`polygon(coords…)` at `crates/reify-compiler/src/geometry.rs:1598-1660`. No profile can
be authored at non-zero z, and the one operator that could move it degrades the kind
loft requires.

`crates/reify-compiler/tests/fixtures/stdlib_geometry_ops_smoke.ri` contains
`loft(prof, prof2)` and does **not** contradict this: that harness asserts arity and
registry membership only, explicitly not argument type/dimension/order (its own header
says so). It is a spelling reference, never evidence of realizability.

Filed as follow-up ticket `tkt_0RS9VJ0K316S7TBYJBDMPVTCY0`; the evidence is
`tests/prd-gate/fixtures/pnrg_envelope_loft.ri`.

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
| sphere | 2.079 | **2** |
| torus | 0.978 | 0 |
| cone | 0.970 | 0 |
| fillet blend | 0.925 | 0 |
| nurbs surface | ≥ 1.0010 † | 1 † |
| sweep / pipe / spline | ≤ 0.598 * | 0 * |
| loft | no datum | — |

**No measured class exceeds K ≈ 16.** The worst is the sphere at 2.079, needing `n = 2`.
nurbs_surface is the one class measured **above** the K = 1 boundary, and so the one whose
`required n` is not 0: its best pinned value is 1.0010 (§1.1, §1.5), and the true ratio at
that `d` is bounded in [1.000626, 1.001321) — an interval lying entirely above 1, so the
crossing is established rather than merely not excluded. Per the † note below that value is
still a lower bound, so the true supremum can only be higher. Even so this changes nothing
at the cap level: `n = 1` is nowhere near the cap-4 budget, and no plausible reading of this
class's data approaches K ≈ 16 — all 92 probes of its dense walk lie within [0.9623, 1.0010],
with no second branch like the sphere's ~2.07 tread. Cap 4
covers K up to 16 at `B = d0` — **7.7× headroom** over the worst thing measured.

\* Lower bounds only. The fine-`d` regime where the sphere reached its supremum was
unaffordable for these three classes (§1.5, §2.1 caveat 1). The cap is justified by
**headroom**, not by a claim of exhaustive coverage.

† Lower bound for a different reason than the row above: nurbs_surface is not
budget-limited (§1.5) — every rung tried completed well under the 90 s wall: down to
0.1 mm on 6545's ladder, and across all 95 runs of #7128's dense walk. Task #7128
resolved the oscillation an earlier amendment left open (no period; `a` piecewise-constant
on plateaus ~1e-4 mm wide; the ratio peaking at each plateau's lower edge) and pinned **1.0010 at `d` = 0.14386 mm**, which crosses the K = 1 boundary and is
what moves `required n` from 0 to 1. The value stays a lower bound — four plateau edges of
very many were pinned, and a dense search cannot prove a supremum over a continuum — so the
true K can only be *higher* than 1.0010. That does not disturb the cap: `n = 1` is nowhere
near the cap-4 budget, and no plausible reading of this class's data approaches K ≈ 16.

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

### 3.3 Decision — keep the cap at 4

*Buys over 3*: K headroom 16 vs 8, i.e. 7.7× vs 3.8× over the worst measured class. That
ratio is unchanged by task #7128: nurbs_surface rose from 0.9975 to 1.0010, but the worst
measured class is still the sphere at 2.079, so 16/2.079 = 7.7× stands as derived.

The *argument* around it does move, and is re-derived rather than left standing. Four
classes are still known only as lower bounds (sweep, pipe, spline and nurbs_surface) and
one (loft) still has no datum at all, so the extra doubling is still cheap insurance
against classes this session could not fully pin down — but the reasons now differ: three
by the 90 s budget wall, and nurbs_surface because a dense search cannot prove a supremum
over a continuum. Its oscillation is no longer unresolved (§1.5, task #7128); four pinned
plateau edges simply are not exhaustiveness. That class has also become the **worked
example** for the insurance rather than merely a claimant on it: believed to peak at
0.9975, it was found on denser walking to exceed 1 (1.0010, `n` = 1). A lower-bound row
moving upward once walked properly is precisely the risk the extra doubling covers, and it
has now happened once, measured — which strengthens the case for 4 rather than weakening
it.

*Costs*: worst-case ~6.0 min instead of ~2.9 min on the re-baselined sphere — a worst
case reached only when **every** attempt fails. The measured classes converge at
`n = 0…2`.

Framing is PRD §4.2's and is unchanged: the loop is a **safety net, not a search
engine**. Neither the halving factor nor the cap is a soundness constant — the verdict is
always the measured one, and the cap bounds **cost**, not correctness.

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
| `tests/prd-gate/fixtures/pnrg_envelope_loft.ri` | loft — **evidence only**, does not realize |
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
probe() {                                       # probe <d>  ->  "<d> <a|NO-DATUM>"
  local d=$1 dir=/tmp/pnrg7128/$1 b; b=$(basename "$F")
  mkdir -p "$dir"
  sed -E "s/#precision\([^)]*\)/#precision($d)/" "$F" > "$dir/$b"
  grep -q "^#precision($d)\$" "$dir/$b" || { echo "$d SED-FAILED"; return 1; }
  local a
  a=$(timeout 240 ./target/release/reify check "$dir/$b" 2>&1 \
      | grep -oE 'deviation [0-9.e+-]+ m' | head -1 | awk '{print $2}')
  # (2) NO-DATUM is a sentinel, and it is FATAL for the row — never a number, never 0.
  #     A §0 Caveat-2 non-realization exits 0 and prints no deviation line, so a harness
  #     that records the empty grep builds a table of confident false near-zero ratios.
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
    if a in ("NO-DATUM", "SED-FAILED"):
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
