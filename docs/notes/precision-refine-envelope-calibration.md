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
| pipe | pipe_r / path curvature | 0.598 | `d/R` = 5e-2 | **lower bound** |
| sweep | profile / path curvature | 0.534 | `d/R` = 1e-2 | **lower bound** |
| spline | profile / path curvature | 0.013 | `d/R` = 2e-2 | **lower bound** |
| loft | — | **no datum** | — | blocked at realization |

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
`d ≥ 50 mm`; cone is identical at 100 mm and 20 mm; torus identical at 40 mm and 20 mm.
In the floor regime `a/d < 1` **trivially**, so a coarse sweep reports a falsely
comfortable envelope. This is the trap the non-analytic classes could not escape (§2.4).

### 1.5 Non-analytic classes

| class | subject | finest affordable rung | a/d |
|---|---|---|---|
| sweep | `sweep(circle(100mm), interp(…))` | 10 mm | 0.534 |
| pipe | `pipe(helix(100mm,80mm,300mm), 20mm)` | 5 mm | 0.598 |
| spline | `sweep(circle(100mm), bezier(…))` | 20 mm | 0.013 |

sweep: 100 mm 0.148 (floor) · 50 mm 0.296 (floor) · 20 mm 0.379 · **10 mm 0.534**.
pipe (shape-shrunk): 20 mm 0.135 · 10 mm 0.258 · **5 mm 0.598**; 2 mm timed out.
pipe (full size `pipe(helix(300mm,200mm,900mm), 60mm)`): 100 mm 0.123, 20 mm 0.580.
spline: 100 mm 0.003 · 50 mm 0.007 · **20 mm 0.013**; 10 mm timed out.

All three were still **rising** at their finest rung. Their entries in §1.1 are lower
bounds, not suprema — see §2.4.

Not measurable, recorded honestly:

* **sweep along a helix**, either profile — `TIMEOUT > 90 s` at every `d` tried.
* **`nurbs(…)`** is excluded on semantics, not behaviour: it returns a **Wire**, which
  has no facets and therefore no chord deviation.
* **loft** — blocked at realization, both failure modes below.

**`nurbs_surface(…)` is measurable and does not belong in the list above.** The
`INDETERMINATE` in 0.22 s originally recorded here was **not** a capability gap: it was
an artifact of a flat, bare-`[x,y,z]`-literal control-point/weight encoding rejected at
eval decode. `control_points`/`weights` are NESTED (u-major × v) grids
(`NurbsSurface` in `crates/reify-ir/src/geometry.rs`), and every pole must be a
`point3(…)` — `point3_components` (`crates/reify-eval/src/geometry_ops.rs:1209`)
accepts only `Value::Point`/`Value::Vector` with 3 components, so a flat list of bare
`[x,y,z]` literals decodes to `Value::List` and is rejected, leaving the subject
undefined. Re-measured 2026-08-24 at HEAD=`2306e029ec` with the corrected nested
encoding (3x3 u-major control net of `point3(…)` poles, nested unit weights, the same
clamped knots `[0,0,0,1,1,1]` in both directions and `u_degree = v_degree = 2` as
before):

    error: RepresentationWithin: sampled facet deviation 1.713e-2 m exceeds bound 1.000e-6 m

`Operation::SurfaceNurbs` remains genuinely absent from `occt_capability_descriptor()`
(`crates/reify-kernel-occt/src/register.rs:101-163`) — that fact still holds. What was
wrong was the inference that the absence prevents realization: it resolves instead via
the `DEFAULT_KERNEL_NAME` fallback, which is exactly how the corrected call above
realizes. No d-ladder row exists yet for this class — one measurement is not a ladder —
tracked as follow-up ticket `tkt_0RSV7JNW3WXWDSFJGRDMHDT63T`.

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
| sweep / pipe / spline | ≤ 0.598 * | 0 * |
| loft | no datum | — |

**No measured class exceeds K ≈ 16.** The worst is the sphere at 2.079, needing `n = 2`.
Cap 4 covers K up to 16 at `B = d0` — **7.7× headroom** over the worst thing measured.

\* Lower bounds only. The fine-`d` regime where the sphere reached its supremum was
unaffordable for these three classes (§1.5, §2.1 caveat 1). The cap is justified by
**headroom**, not by a claim of exhaustive coverage.

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

*Buys over 3*: K headroom 16 vs 8, i.e. 7.7× vs 3.8× over the worst measured class. With
three classes known only as lower bounds and one (loft) with no datum at all, the extra
doubling is cheap insurance against a class this session could not reach.

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
| `tests/prd-gate/fixtures/pnrg_envelope_spline.ri` | spline (+ a commented, measurable `nurbs_surface` alternative) |
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
