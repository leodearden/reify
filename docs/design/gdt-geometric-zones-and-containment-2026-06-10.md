# Design — GD&T geometric zones & containment vertical

**Provenance:** single-session design (2026-06-10), interactive with Leo, spawned from the
deferred-task triage of #4269 ("tolerancing — geometric zone-REGION construction for
nominal_zone"). Grounded in a four-strand agent survey of (A) the implemented tolerancing
surface, (B) the task graph, (C) kernel/engine/GUI capabilities, (D) ASME Y14.5-2018 / ISO GPS
semantics, all with file:line / task-id anchors. All decisions below were ratified by Leo this
session.

**Companions (do not duplicate):**
- `docs/prds/v0_6/tolerancing-gdt-surface-completion.md` — the COMPLETE scalar GD&T surface
  this builds on (tasks 4265–4268, 3116 all done). This design is its geometric successor.
- `docs/prds/v0_6/geometric-relations.md` (tasks 4381–4388, mostly pending) — owns the typed
  datum lattice (Direction/Point/Axis/Plane/Frame), feature→datum projections (ε=4385), and the
  DOF ledger (θ=4388). GD&T datum-referenced zones CONSUME that lattice (rung 4 dep), never
  re-implement it.
- `docs/prds/v0_6/process-dfm-overhang-draft.md` (γ=4408 pending) — owns the
  `Engine::measure_dfm_rules` realize→measure→diagnose check-pass pattern this design's
  conformance pass mirrors as a sibling arm.
- `docs/prds/v0_6/process-dfm-thickness-metrology.md` (4421–4427) — owns the solid→SDF wire;
  a future alternative deviation mechanism, NOT a dependency of this slice.

---

## 1. Problem

Reify's GD&T support is a **typed vocabulary, not an engine**. The shipped surface
(`crates/reify-compiler/stdlib/tolerancing.ri`, 19 templates) is scalar end to end:

- `nominal_zone` is a scalar effective-zone **width** (`tolerancing.ri:46-51`), not a region.
- `Conforms` is the one-line predicate `effective_tolerance_zone(...) >= measured_deviation`
  where `measured_deviation : Length = 0mm` is **hand-typed by the author**
  (`tolerancing.ri:227-232`) — nothing ever measures geometry.
- `feature : Geometry` / `datum_refs : DatumRef` (16 + 8 sites, real types since #3116) are
  opaque type-checked slots **never dereferenced anywhere** in the surface.
- The trait hierarchy is standards-wrong in ways that block zone construction:
  `Flatness(material_condition: MMC)` compiles (illegal — form is RFS-only per Y14.5),
  `CircularRunout`/`TotalRunout` have **no datum slot** (a datum axis is mandatory), there is no
  Ø-cylinder vs parallel-planes zone-shape distinction, and `Concentricity`/`Symmetry` were
  removed from ASME Y14.5-2018 with no flag.

The core tension for a *parametric* tool: geometric containment ("actual feature ⊆ tolerance
zone") is trivially-passing when actual == nominal, which it always is in pure parametric land.
A zone/containment vertical therefore earns its keep only via consumers that don't need
external measured data. This design picks two: **virtual-condition boundary geometry**
(clearance/assembleability/gauge checks — purely design-side, the highest design-tool value) and
**synthetic deviation injection** (the user passes a perturbed copy as the actual — makes the
containment check honestly exercisable in examples/tests today). External metrology import is
explicitly bookmarked, not built (§6).

## 2. Decisions (Leo-ratified, with declined alternatives)

**D1 — Slice = zone-region + containment vertical; engine/DSL only, NO GUI.** Statistical
stack-up deepening, as-manufactured conformance, and viewport GD&T visualization are follow-ons
that build on this slice (declined as drivers now). A GD&T renderer becomes its own PRD with
this slice as named producer.

**D2 — Legality-typing restructure rides this PRD as rung 0** (declined: separate PRD; skip).
Zone constructors are per-characteristic — zone shape, datum-mandatoriness, and MMC-eligibility
live in the hierarchy — so the corrected hierarchy goes underneath first. **Mechanism
(substrate-corrected 2026-06-10):** compile-time *unrepresentability* is not deliverable on the
verified substrate — structures must re-declare inherited trait params (a pinned default is
still caller-overridable, `trait_requirements.rs` collect/override semantics; lets do NOT
satisfy param requirements), and template-declared `.ri` constraint guards are **not enforced
per ctor instance** (empirically verified: a `DimensionalTolerance` ctor violating its own
`upper_deviation >= lower_deviation` guard passes `reify check` clean — a shipped-surface gap
in its own right, noted in the PRD). Legality is therefore enforced as **Rust-side check-time
diagnostics** over enumerated callout instances (`E_GdtIllegalModifier` for MMC/LMC on
non-FOS, `W_GdtRemoved2018` for Concentricity/Symmetry, etc.), sharing the instance enumerator
the rung-3 conformance pass needs anyway. The `.ri` half is additive: runout gains a required
datum slot; `Straightness` splits surface-line vs FOS-axis variants (the axis variant is the
one form control that takes MMC); profile splits datum-less vs datum-referenced variants
(avoids an optional-Geometry-param default, which has no clean value); FOS
location/orientation callouts gain a zone-shape discriminator (Ø-cylindrical vs
width/parallel-planes). Shipped examples/tests updated in the same rung (the CI-pinned
MMC-vs-RFS flip in `std_tolerancing_surface.ri` survives — Position is a legal MMC consumer).

**D3 — Conformance = measured-deviation primary** (declined: boolean-containment primary;
sampled point-containment primary). Tessellate the actual feature, measure max deviation
against the nominal, compare against the scalar zone width — i.e. the geometric pass **feeds
the shipped scalar `Conforms` predicate** rather than replacing it. Rationale: the conformal
case (actual exactly on the zone boundary) is the numerically worst case for OCCT booleans;
deviation gives a continuous "by how much" margin; and the sampled-max-deviation primitive
already exists privately as `measure_mesh_deviation` (`reify-kernel-occt/src/ffi.rs:1068`,
4-pts/triangle projection onto exact B-rep, currently tess-QA-only) — rung 3 promotes it to a
public `GeometryQuery` (max-deviation was deferred to v0.4 at `geometry.rs:1205-1206`; this is
that promotion with a named consumer). Honest floor: measured deviation carries a
±(sample density + chord_tol) bound, same convention as DFM thickness (no fake exactness).
Boolean `Difference(actual, zone)` + volume≈0 is used **only as a test oracle** cross-checking
the deviation path on clearly-inside / clearly-outside fixtures.

**D4 — Language surface: extend `Conforms` with optional `actual : Geometry`** defaulting to
the nominal feature (declined: parallel `ConformsGeometric` constraint — forks the vocabulary;
`as_built()` occurrence — more machinery for the same effect). Additive, shipped scalar path
untouched. **Mechanics (substrate-corrected 2026-06-10):** param defaults compile in a
*neutral scope* (`reify-compiler/src/functions.rs:95-119`) so `= tolerance.feature` cannot
evaluate; instead the engine pass **statically detects the explicit `actual` argument binding**
on the compiled constraint instance (the same arg-peeling shape `RepresentationWithin` uses),
and the param carries an inert marker default (a no-arg registered builtin, e.g. `nominal()`)
solely for call-site compatibility — the `Conforms` *body* never reads `actual`, so the marker
value flows nowhere. The pass measures only when an explicit `actual` is bound; without a
kernel the result is `Indeterminate`, never a false `Violated` — the C1 invariant proven by
`RepresentationWithin` interception (`reify-eval/src/engine_constraints.rs:42-125`). Crucially
the pass-interception design (not constraint-body geometry calls) is what sidesteps the
kernel-less P1-check-phase trap that structurally blocked #4275 — geometry in constraint
*bodies* evaluates in the kernel-less phase; a dedicated pass realizes handles itself.
Synthetic deviation = the author passes a transformed/offset copy of the nominal as `actual`.

**D5 — Engine pass = sibling arm of the DFM check-pass pattern.** A
`measure_gdt_conformance` pass in `Engine::check`: enumerate `Conforms` instances whose
tolerance is a `GeometricTolerance` → realize `feature` (and `actual`) handles → construct the
zone → measure deviation → weave `ConstraintResult{satisfaction, diagnostics}` back in caller
order. Mirrors `measure_dfm_rules` (PRD process-dfm-overhang-draft §2.2, task 4408 pending) and
the live `RepresentationWithin` seam. Sequencing: this pass does NOT hard-depend on 4408
landing — both instantiate the same documented pattern — but the PRD must declare the seam so
whichever lands second cites and converges with the first (one §3 engine-integration-norm entry
each, per the 4410 convention). Geometry evaluation is engine-path-only (plain `reify eval`
leaves geometry Undef — `tolerancing.ri:254-256`), so all geometric rungs are observable via
`reify check` / engine builds, while the scalar surface stays eval-friendly.

**D6 — Zone construction = stdlib composition over existing kernel ops, plus ONE new kernel
op.** Existing ops cover most zones: position Ø-zone = `Cylinder` (`geometry.rs:535`);
cylindricity/runout annulus = `Tube` (`:542`); flatness/parallelism slab = `Box`/extrude;
whole-solid profile zone = `Thicken(+t) − Thicken(−t)` (OCCT `MakeOffsetShape`,
`occt_wrapper.cpp:2036-2046`) composed via `Difference`. The one genuinely missing op is a
**face-offset slab**: offset an arbitrary nominal *face* by ±t and cap into a solid (per-face
profile-of-surface zones). **Mechanics (substrate-corrected 2026-06-10):** user-defined `.ri`
fns returning composed geometry are NOT a recognized substrate — geometry lets are detected by
a hardcoded registry of constructor names (`reify-compiler/src/geometry.rs:1730-1790`, 44 fns
across four lists, each with a `compile_geometry_call` lowering arm). Zone constructors are
therefore **new registered geometry functions** (the documented 3-file recipe: registry list +
lowering arm + `EXPECTED_DISPATCH_COUNT` in `reify-compiler/src/geometry.rs`; `GeometryOp`
variant only for the face-offset slab in `reify-ir/src/geometry.rs`; eval dispatch in
`reify-eval/src/geometry_ops.rs`), lowering to compositions of existing op DAG nodes (nested
composition verified — `examples/m5_geometry*.ri`). The scalar `nominal_zone` (effective width
incl. MMC bonus) is **unchanged** and becomes the size input to the region constructors
(declined: making `nominal_zone` itself a region — breaks the CI-pinned scalar surface for
nothing).

**D7 — Virtual condition: scalars + boundary solids + clearance check** (declined: scalars
only — leaves zones with no design-side consumer; gauge-solid STEP/STL export — follow-up once
io-export covers it). Derived `virtual_condition` / `resultant_condition` lets on FOS callouts
(VC = MMC size ± geometric tol, direction per internal/external feature). VC **boundary
solids** (cylinder at VC diameter along the feature axis; profile boundary for irregular FOS)
as real geometry, booleaned/distance-checked against mating parts via the shipped
interference/`min_clearance` machinery (task 2530) = compile-time assembleability proof and
functional-gauge geometry. No code-first competitor has this (survey D).

**D8 — Datum phasing: datum-free rungs first, datum-referenced zones as a late rung gated on
the geometric-relations lattice** (declined: datum-free-only scope; gating the whole PRD on
4382/4385). Rungs 0–3 need no DRF: form zones, profile-vs-nominal zones, and FOS-axis zones are
anchored by the *parametric nominal* (in Reify the basic dimensions ARE the model). VC boundary
solids in rung 2 take an **explicit axis param** first (the author built the hole; they have
its axis); auto axis-extraction (Cylindrical→Axis) arrives with 4385. Rung 4 (position/runout
zones anchored to datum axes/frames, DRF precedence, DOF-arrest diagnosis) depends on
4382 (Direction type + projections) and 4385 (feature→datum projections) — tasks filed
`pending` with those dep edges per the deferred-vs-pending norm, NOT deferred.

## 3. Rung structure

| Rung | Content | New substrate | Gated on |
|---|---|---|---|
| 0 | Legality rung (D2, substrate-corrected): additive .ri restructure (mandatory runout datum, Straightness surface/axis split, Profile datum-less/Related split, zone-shape discriminator) + Rust check-time legality diagnostics (E_GdtIllegalModifier, W_GdtRemoved2018); example/test updates | none (additive .ri + diagnostics plumbing) | — |
| 1 | Zone-region constructors (consumes/promotes **#4269**): per-characteristic zone solids from `nominal_zone` width; face-offset-slab kernel op | 1 kernel op (face offset slab) | rung 0 |
| 2 | VC/RC derived scalars on FOS callouts; VC boundary solids (explicit axis); worst-case clearance/assembleability check vs mating part | none (composition + 2530) | rung 0 (scalars), rung 1 (boundary solids) |
| 3 | Geometric containment `Conforms`: max-deviation `GeometryQuery` (promote `measure_mesh_deviation`); `measure_gdt_conformance` engine pass; `Conforms.actual` param; synthetic-deviation e2e | 1 query variant + 1 engine pass | rungs 0–1 |
| 4 | Datum-referenced zones: position/orientation/runout zones anchored via datum lattice; DRF ordering + DOF-arrest diagnostics | none new (consumes lattice) | rungs 1–3 + **4382, 4385** (pending deps) |
| BM | Measured-feature import **bookmark**: stub PRD + activate #4290 (PointCloud, deferred) as its named consumer; NO implementation | — | explicit non-goal here |

## 4. Cross-PRD seams (G4)

- **#4269 (deferred)** — this PRD IS the consumer its deferral notes demand ("do NOT promote
  until a containment-Conforms or renderer is scoped"). Rung 1 promotes/absorbs it; never file a
  parallel zone task.
- **4382/4385 (geometric-relations, pending)** — rung 4 + VC auto-axis depend on them; datum
  extraction is theirs, full stop.
- **4408 (`measure_dfm_rules`, pending)** — sibling pattern, no hard dep (D5); seam declared
  both ways; each pass gets its own engine-integration-norm §3 entry.
- **#4290 (PointCloud import, deferred stub)** — the measured-import bookmark names it as
  substrate; activating it is the future PRD's job, not this one's.
- **Do-not-touch:** the kernel realization-tolerance subsystem (`tolerance_budget/combine/
  scope/promise`, `RepresentationWithin`) — representation accuracy, firewalled from GD&T; the
  stack-up builtins (read-only reuse); cancelled 4276/4277 stay cancelled.

## 5. Out of scope (named, so the PRD can say "no" cleanly)

GUI/viewport rendering (follow-up PRD; this slice is its producer); measured-data import
(bookmark only); statistical stack-up extensions / GD&T→contributor bridge / Cpk; composite
FCFs, datum targets/simulators, MMB, projected zones Ⓟ, tangent-plane Ⓣ, free-state Ⓕ;
Rule #1 envelope vs ISO independency election; PMI/AP242 export (note: OCCT XDE makes this
unusually cheap later — `XCAFDoc_DimTolTool` ships in the linked OCCT); SDF-route deviation
(rides 4421–4427 when it lands; the deviation query here is B-rep-sampled).

## 6. Verification sketch (G2)

Leaf signal: an e2e CI example (`examples/tolerancing/` + `reify-cli` test) where a flatness or
profile callout on a real face, with a synthetically deviated `actual` (transformed copy),
yields `Conforms` **Violated with the measured deviation magnitude** in `reify check` engine
output, and the undeviated twin yields Satisfied; plus a clearance example where a VC boundary
solid vs a mating part proves worst-case assembleability (and flags a deliberately
under-clearanced variant). Numeric oracles carry the honest sampling floor (±(h+chord_tol)
convention, as DFM thickness). Kernel-less runs report `Indeterminate` (C1 pinned in tests).

## 7. Key survey evidence (condensed)

- Scalar surface + opaque slots: `tolerancing.ri:46-51,227-232`; `tolerancing.rs:118-150`;
  `type_resolution.rs:563-565`; `Datum` never instantiated.
- Zone composability: `GeometryOp::{Cylinder:535,Tube:542,Box:529,Extrude:680,Sweep:692,
  Difference:569-583,Thicken:793}`; missing face-offset slab (Thicken solid-only,
  `occt lib.rs:5617`).
- Containment substrate: point-only `Contains` (`geometry.rs:1183`, #3611); min-`Distance`
  (`:959`, 2530); **no** max/Hausdorff query (deferred note `:1205`); private
  `measure_mesh_deviation` (`ffi.rs:1068`).
- Engine seam: `RepresentationWithin` interception (`engine_constraints.rs:42-125`, C1);
  `measure_dfm_rules` PRD'd not landed (4408).
- Geometry is engine-only: plain eval leaves `box()` Undef (`tolerancing.ri:254-256`).
- Standards map (survey D): zone-shape table per characteristic; MMC legality matrix; VC/RC
  formulas; Y14.5-2018 removals; competitive gap (no code-first tool has typed GD&T).
