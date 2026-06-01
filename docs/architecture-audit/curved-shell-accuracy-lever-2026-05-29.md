# Curved-Shell MacNeal–Harder Accuracy — Which Lever, Which Substrate

**Date:** 2026-05-29
**Author:** FE-domain design session (analysis + task reconciliation — escalation untouched; esc-3392-360 is owned by the escalation-watcher)
**Decision owner:** task 4065 reconciliation
**Inputs:** `crates/reify-solver-elastic/src/shell_assembly.rs`, `…/src/shell_kinematics.rs`, `…/tests/shell_benchmarks.rs`, `docs/prds/v0_4/structural-analysis-shells.md`, `docs/architecture-audit/mitc3plus-formulation-review-2026-05-29.md`, fused-memory tasks 3392 / 4065 / 3513.

---

## TL;DR — the lever and the substrate

**Chosen lever: BOTH — per-node directors *and* ANS-membrane — as a stack, not as alternatives.** They are layered on a **degenerated/continuum-shell substrate** (per-node directors → varying element Jacobian), carrying task 3392's MITC3+ assumed-transverse-shear treatment onto the curved geometry.

- **The flat-facet ANS-membrane premise that originally created 4065 is rejected.** A flat facet has zero curvature in its membrane strain measure, so it exhibits **no element-level membrane locking** — ANS-membrane on a flat triangle corrects a pathology the substrate does not have. (The FE review §3 is correct here.)
- **But "per-node directors *instead of* ANS-membrane" (the FE review's lean) is also wrong.** Per-node directors *are* the varying-Jacobian curved substrate; once you have it, the curvature re-enters the membrane strain and **membrane locking becomes real** — so ANS-membrane becomes *necessary*, not optional. The two are a stack: directors enable curvature; ANS-membrane is the locking cure that curvature then requires.
- **Reaching ~50%/4×4 REQUIRES leaving flat facets.** At a *fixed* 4×4 mesh, no element-local trick on flat facets closes a 21–2200× gap; the gap is geometric. Mesh refinement is a convergence-rate lever, not a 4×4 lever, so it cannot meet a 4×4 target by itself.
- **3392 (flat-facet MITC3+) buys the transverse-shear cure only.** It does not touch faceting, directors, curved geometry, or membrane. 4065 must not re-claim shear relief; it inherits it and *carries it onto* the curved substrate.

**This synthesis vindicates the *physics* of 4065's original premise (varying-J element + ANS-membrane) while correcting *where the substrate comes from*:** the varying-J / per-node-director substrate is no longer a free input from 3392 — 4065 must build it. The "varying-Jacobian element" and "per-node directors" are the same thing described from the geometry side vs. the kinematics side.

**4065 reconciliation:** re-specced to own the degenerated-shell substrate + ANS-membrane stack; split into two gated sub-pieces (substrate A, ANS-membrane B); ~50%/4×4 target kept; 3513's gating preserved and re-pointed. Details in §8.

---

## 1. What the substrate is today (on-substrate facts, not theory)

From `shell_assembly.rs` / `shell_kinematics.rs`:

- **Bare MITC3** (Bathe & Dvorkin 1985): CST membrane + plate bending, with MITC assumed transverse shear sampled at the **three edge-midpoint** tying points (`shell_kinematics.rs:95–117`).
- **Single element normal**, built once from the first two edges (`build_shell_frame`, `shell_assembly.rs:66–114`). **No per-node directors.**
- **Constant in-plane Jacobian** per element (`det2`, `jac2_inv_t` computed once, `shell_kinematics.rs:82–93`). **Flat facet.**
- 6 DOF/node; drilling (θ about the single normal) carries zero stiffness.
- `elements/mitc3_plus.rs` exists with **unused** bubble functions — the misnamed shell task 3392 will turn into genuine flat-facet MITC3+.

So the substrate is a **flat-facet facet-shell**: each element is a flat plate+membrane in a local frame; the structure's curvature is represented *only* by the dihedral kinks between facets at shared edges/nodes. Within any one element, membrane and bending are **exactly decoupled** (κ_element = 0).

---

## 2. The error-decomposition framework (defined precisely for a flat-facet triangle)

Three candidate error sources for a curved-shell benchmark on this substrate:

1. **Faceting / geometric error (+ lost intra-element membrane–bending coupling).** A flat facet cannot represent a curved mid-surface; the discrete model is a coarse polyhedron. Critically, because each facet has zero internal curvature, the membrane–bending coupling that a real curved shell carries *inside* the material is carried **only across element boundaries** (facet kinks). At coarse mesh this under-represents the true coupling and makes the structure spuriously **stiff** → displacement **under**-prediction. This is an **assembly-level / geometric** phenomenon, not an element-local one.

2. **Transverse-shear locking.** Spurious shear stiffness as t→0. **MITC3 is precisely the cure**, and it works on this substrate: the regression test `mitc3_thin_shell_pinched_cylinder_does_not_lock_under_decreasing_thickness` shows the normalized response *increases* as t→0 (ratio n(0.01)/n(1.0) = 4.73 > 1, `shell_benchmarks.rs:1239,1266–1353`). A locking element would collapse (ratio < 1). So shear locking is **already substantially relieved**; 3392's MITC3+ improves it further.

3. **Element-level membrane locking.** Parasitic membrane strain generated by a bending mode because **curvature couples w into the membrane strain measure**. This is, by definition, a property of **curved** elements (curved quads, higher-order, degenerated shells). **On a flat facet κ_element = 0, so the membrane strain has no curvature term and this locking mode cannot occur.** A flat-facet triangle does **not** membrane-lock. (The test-file comments saying "membrane locking on curved geometry" use the term colloquially for the §2.1 geometric over-stiffness; the precise attribution is faceting, not element-level membrane locking. Worth correcting those comments when 4065/3513 touch the file.)

### The control experiment that settles which dominates

The **twisted-beam** benchmark is the control. Its helicoid is so gently curved that the facets are **nearly planar** (the test notes this explicitly: "*the helicoid's elements are nearly planar*", `shell_benchmarks.rs:1074–1075`). Same element, same MITC3 shear, same CST membrane — but with faceting ≈ 0 it is only **~1.7×** off.

| Benchmark | Curvature | R/t (thinness) | Under-prediction (4×4) | Facets |
|---|---|---|---|---|
| Twisted beam | ~none (helicoid) | ~37 | **~1.7×** | ~planar |
| Scordelis-Lo roof | single | 100 | **~21×** | curved |
| Pinched cylinder | single | 100 | **~76×** | curved |
| Hemisphere | double | **250** | **~2200×** | curved |

The element-local error (shear + CST-membrane + bending interpolation) is bounded by the planar-facet case at **~1.7×**. Everything above that — ~12× for Scordelis-Lo, ~45× for the pinched cylinder, ~1300× for the hemisphere after dividing out the ~1.7× element-local ceiling — appears **only when the facets become curved**, and scales with curvature topology (single → double) and thinness. That delta is, by construction, the thing that is *absent* when facets are planar: **faceting + lost curvature coupling.** This is direct on-substrate evidence, not theory.

**Conclusion of §2: on Reify's flat-facet triangular substrate, none of the three curved benchmarks is dominated by element-level membrane locking (impossible on a flat facet) or by transverse-shear locking (MITC3 handles it, capped at ~1.7× by the twisted-beam control). All three are dominated by faceting / geometric error + the flat facet's inability to carry membrane–bending coupling inside the element.**

---

## 3. Per-benchmark decomposition

### Hemisphere — R/t = 250, ~2200× (worst)
- **Character:** doubly-curved, **nearly-inextensional bending** (alternating in/out point loads; deformation is almost pure bending with ~zero membrane strain).
- **Decomposition:** faceting/coupling **dominant and extreme** — double curvature + extreme thinness make 32 flat triangles maximally deficient at representing the smooth doubly-curved inextensional-bending field. Transverse-shear: minor (MITC3), possibly some residual at R/t=250 that 3392 trims. Element-level membrane locking: **zero** (flat facet).
- **Why it's the hardest:** double curvature is the case a flat facet approximates worst, and inextensional bending is the mode most sensitive to the missing intra-element coupling.

### Pinched cylinder — R/t = 100, ~76×
- **Character:** singly-curved, **bending-dominated / near-inextensional**, concentrated loads. The classic membrane-locking benchmark *once an element is curved*.
- **Decomposition:** faceting/coupling **dominant** (single curvature → less than the hemisphere; near-inextensional + point load → more than Scordelis-Lo). Shear: minor. Element-level membrane locking: **zero on flat facets** — but this is exactly the benchmark where it will reappear *the moment we curve the substrate*, so ANS-membrane is load-bearing for this one post-upgrade.

### Scordelis-Lo roof — R/t = 100, ~21× (mildest of the curved three)
- **Character:** singly-curved cylinder under self-weight; **membrane-dominated** with a bending boundary layer near the free edges.
- **Decomposition:** faceting/coupling dominant but smallest, because single curvature + membrane-dominated + R/t=100 is the least demanding of the curved geometry. Shear: minor. Element-level membrane locking: zero on flat facets; reappears (and is the key locking mode for a membrane-dominated curved problem) once curved → ANS-membrane is the cure on the curved substrate.

**Common thread:** the ordering 2200× ≫ 76× > 21× tracks (double vs single curvature) × (thinness) × (inextensional-bending character) — i.e. it tracks **geometric** difficulty, exactly as a faceting-dominated story predicts, and **not** an element-locking story (which would track t alone and would be capped near the twisted-beam ceiling).

---

## 4. The cure and the substrate — why both, and why it means leaving flat facets

### Step 1 — recover the geometry: per-node directors (= the varying-Jacobian degenerate shell)
The fix for faceting + lost coupling is the **standard degenerated/continuum-shell** construction: each node carries its own **director** `V_i` (≈ true surface normal at that node), and geometry is interpolated as
`X(ξ,η,ζ) = Σ N_i(ξ,η)·x_i + (ζ/2)·Σ N_i(ξ,η)·t_i·V_i`.

For a 3-node (linear) element the mid-surface (ζ=0) is still the flat plane through the three nodes — but because the **directors tilt across the element**, the full Jacobian `J(ξ,η,ζ)` **varies**, and that director-tilt term is exactly the mechanism by which the element represents curvature and recovers the intra-element membrane–bending coupling a flat facet lacks.

**This is the key identity:** *per-node directors* (kinematics view) and *varying Jacobian / curved element* (geometry view) **are the same substrate upgrade**. The FE review treated them as alternatives ("directors, not varying-J"); they are not — you cannot have genuinely varying directors and a constant Jacobian. This also reconciles 4065's original wording ("the varying-Jacobian element"): that element is precisely the per-node-director degenerate shell — it was never 3392's to provide once 3392 became flat-facet.

This step needs the per-node normals to come from somewhere — either supplied per-vertex by the mid-surface extraction pipeline (the v0.4 shells PRD already notes voxel extraction produces per-vertex data) or estimated by averaging adjacent facet normals (neighbour-finding). **This is the "substantial mesh-layer plumbing" the 3392 record flagged for the curved direction — it lands here, in 4065, which is where it belongs.**

### Step 2 — the curvature reintroduces membrane locking → ANS-membrane (now necessary)
Once the element is curved, curvature couples `w` into the membrane strain, and the linear (CST-level) membrane field will develop parasitic membrane strain under bending → **classical membrane locking**, severe at high R/t and exactly on the pinched-cylinder / hemisphere inextensional modes. The standard cure is an **assumed-natural-strain membrane field (ANS-membrane)** — re-interpolate the covariant membrane strains from tying samples so the inextensional mode is representable with zero parasitic membrane energy. **ANS-membrane is therefore *required as a companion to* the curved substrate — not an alternative to directors, and not applicable to a flat facet.**

### Step 3 — carry the transverse-shear cure onto the curved geometry (from 3392)
3392 delivers the MITC3+ assumed-transverse-shear field on the **flat reference** triangle. On the curved substrate the covariant shear tying must be **re-expressed/re-validated against the varying Jacobian** (the assumed-shear construction is in natural coordinates, but the covariant→physical map now varies). This is a **carry-over / re-derivation**, not a new shear deliverable — the no-double-count boundary.

### Why refinement is not the lever for *this* target
The target is fixed at **4×4**. Refinement = changing the mesh, which a 4×4 target forbids. The substrate upgrade *also* improves the convergence rate (so finer meshes converge faster), but it is the **substrate**, not refinement, that meets a 4×4 target. Refinement stays a complementary quality lever, out of scope for the 4×4 deliverable.

---

## 5. What 3392 buys vs. what 4065 owns (the no-double-count boundary)

| | 3392 (flat-facet MITC3+) | 4065 (this task) |
|---|---|---|
| Transverse-shear locking | **Owns** the cure (interior-tying assumed shear + rotation bubble + condensation), on flat facets | **Carries** 3392's shear onto curved geometry (re-express vs varying J); does **not** re-claim it |
| Per-node directors / curved geometry / varying J | Explicitly **out of scope** (single normal, constant J) | **Owns** (the degenerate substrate) |
| Faceting / geometric error | Untouched | **Owns** (directors + curved geometry remove it) |
| Membrane locking / ANS-membrane | N/A (no curvature on flat facet) | **Owns** (ANS-membrane on the curved substrate) |
| Benchmark signal | "measurable improvement vs bare MITC3 on a bending/shear benchmark (e.g. twisted beam)"; keeps the *wide* smoke envelopes on the curved three | Tightens the curved three to **~50%/4×4** |

3392's expected effect on the **curved** benchmarks is **small** (the twisted-beam control caps element-local error at ~1.7×, and the no-lock test shows shear is already mostly handled) — so 4065 cannot lean on 3392 for the curved gaps. They are 4065's to close, via the substrate, not via shear.

---

## 6. Reachability of ~50%/4×4 — honest per-benchmark

With the full stack (degenerate substrate + per-node directors + ANS-membrane + carried MITC-shear), reaching within ~2× of reference at coarse meshes is what the published Bathe & Lee degenerate MITC3/MITC3+ results demonstrate. Per-benchmark risk:

- **Scordelis-Lo (21×):** **Most reachable.** Membrane-dominated single-curvature; ANS-membrane on the curved substrate is squarely the right instrument. (This is 3513's GREEN.)
- **Pinched cylinder (76×):** **Reachable.** The canonical membrane-locking benchmark for curved elements; ANS-membrane + directors is the textbook cure.
- **Hemisphere (2200×, R/t=250):** **Highest risk; keep the target but flag it.** Double curvature + extreme thinness is the most demanding case, and **triangular** degenerate shells lag quads at coarse mesh. Published MITC3+ does well here, but landing *exactly* ≤2× at 4×4 (= 32 triangles on the quadrant) is marginal. Recommend the implementer be allowed to report an honest hemisphere result (e.g. if it lands at ~3× after the full stack) for review rather than pre-conceding or band-gaming — the target stays, the risk is named.

---

## 7. Costs / risks

- **Cost: HIGH** (this is the curved-element work the 3392 record priced as "substantial mesh-layer plumbing + FE-domain review required"). It now lands in 4065, correctly. Comprises: per-vertex director provenance/plumbing, varying-J degenerate geometry + strain-displacement, ANS-membrane tying derivation, carrying MITC-shear onto curved J, and condensation bookkeeping if the rotation bubble is retained on the curved element.
- **Correctness risk: MODERATE–HIGH but bounded by literature.** A degenerate MITC3-family shell is a **published, citable** construction with known acceptance tests (patch test, no spurious zero-energy modes, isotropy) and known reference convergence — validation is "match the paper," not "invent and prove." Must reduce to 3392's flat-facet behaviour when the surface is flat (directors parallel).
- **Cross-PRD seam (G4):** per-vertex director provenance touches the mid-surface extraction pipeline (v0.4 shells PRD). Name this seam in the substrate sub-task — estimated normals vs extraction-supplied normals is a design decision with accuracy consequences.
- **Grammar note (GR-040):** this is Rust kernel work, so the no-method-call-syntax rule is N/A to the implementation; any Reify-side stress helpers stay free-function form (e.g. `to_global(stress, frame)`), never `stress.to_global(frame)`.

---

## 8. Task 4065 reconciliation (what was changed)

**4065 re-specced** (title + scope + approach) to the chosen lever/substrate, **~50%/4×4 target kept**:

- **New title:** *Curved-shell MacNeal-Harder accuracy to ~50%/4×4 — degenerated-shell substrate (per-node directors + varying-J) + ANS-membrane, carrying MITC3+ shear.*
- **Lever:** degenerate substrate (per-node directors = varying Jacobian) **+** ANS-membrane, **stacked**; carries 3392's MITC3+ assumed-shear onto the curved geometry. Flat-facet ANS-membrane premise rejected; refinement is not the 4×4 lever.
- **Split into two gated sub-pieces** (sibling top-level tasks, so the orchestrator schedules and FE-review-gates each):
  - **A — degenerate substrate:** per-node directors + varying-J curved geometry + carry MITC3+ assumed-shear onto curved J. Depends on **3392**. (Correctness-sensitive; FE review of the curvature/director representation, like 3392.)
  - **B — ANS-membrane:** assumed-natural-strain membrane field on the curved substrate (the membrane-locking cure curvature reintroduces). Depends on **A**.
- **4065 itself** = integration + GREEN the **hemisphere + pinched cylinder** to ~50%/4×4 (Scordelis-Lo is 3513's GREEN). Depends on **3392, A, B**.
- **3513 preserved and re-pointed:** still gated on **[3392, 4065]** (gating unchanged); its description is corrected so the curved substrate + ANS-membrane are attributed to the **4065 chain** (not to 3392, which is now flat-facet shear only).

*(Sub-task ids are recorded in the reconciliation summary returned to the escalation-watcher.)*

---

## 9. Where this diverges from the 2026-05-29 FE review

The FE review (§3) is right that (a) flat facets don't element-level membrane-lock, (b) the gap is faceting + shear, (c) the curved-element work belongs in 4065. It **under-states** one thing and draws **one false dichotomy**:

1. **Per-node directors ARE the varying-Jacobian substrate** — the review framed "directors" and "varying-J" as different options; they are the same upgrade (director tilt *is* the varying Jacobian).
2. **ANS-membrane is necessary, not optional** — the review leaned toward "directors + refinement, maybe skip ANS-membrane." But the moment directors curve the element, membrane locking is real and ANS-membrane is the required companion cure (sharpest on the pinched cylinder and Scordelis-Lo). Hence **both**, stacked — which also vindicates the *physics* of 4065's original "varying-J + ANS-membrane" premise while fixing its substrate-ownership error.

---

## Evidence trail
- `shell_assembly.rs:1–44,66–114,221–223` — bare MITC3, single normal, constant Jacobian, edge-midpoint MITC tying.
- `shell_kinematics.rs:82–93,95–117` — constant Jacobian; MITC3 covariant shear at three edge midpoints.
- `shell_benchmarks.rs` — hemisphere ~2200× (`917–918,1037–1043`); pinched ~76× (`380–381,414`); Scordelis-Lo ~21× (`710–711,883–887`); twisted beam ~1.7×, "nearly planar" (`1074–1075`); no-shear-lock regression ratio 4.73 (`1239,1266–1353`).
- `docs/prds/v0_4/structural-analysis-shells.md` §2,§3,§21 — bare-MITC3 flat-facet contract; widened bands deferred to 3392; per-vertex extraction data.
- `docs/architecture-audit/mitc3plus-formulation-review-2026-05-29.md` §1,§3 — 3392 redirected to flat-facet MITC3+; membrane-on-flat-facet is moot; curved work relocates to 4065.
- Literature: Lee, Lee & Bathe (2014), *Comput. Struct.* 138:12–23 (MITC3+); degenerated-shell director kinematics (Ahmad/Bathe lineage); ANS membrane for curved shells.
