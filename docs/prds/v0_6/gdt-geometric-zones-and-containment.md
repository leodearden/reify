# PRD — GD&T geometric zones & containment (v0_6)

**Status:** active — authored 2026-06-10 (interactive design session with Leo; all design forks
ratified). Approach **B+H** (contract + two-way boundary tests). Successor to the COMPLETE
`tolerancing-gdt-surface-completion.md` (4265–4268, 3116 done); consumer that promotes deferred
**#4269**. Design doc: `docs/design/gdt-geometric-zones-and-containment-2026-06-10.md`
(decisions D1–D8 with declined alternatives and substrate corrections; read it before
implementing).

**Rungs 0–3 delivered (2026-06-15):** α/4474 · β/4475 · γ/4476 · δ/4477 · ε/4478 · ζ/4479 ·
η/4480 · **θ/4481** (critical leaf) all done. §9 boundary-test matrix gated as one CI suite
(`crates/reify-cli/tests/cli_gdt_integration_gate.rs`: B4 regression + B5 oracle cross-check +
B9 pass-weave); engine-integration-norm §3.11 entry committed (cites §3.8, DFM walk, 4408
landed first). Remaining rungs: ι (datum-anchored zones, dep 4382/4385 pending) · κ (DRF
diagnostics, dep 4388 pending).

## 1. Goal

Take Reify's GD&T surface from *typed scalar vocabulary* to *geometric engine*, design-side
first. A user can:

1. Write standards-legal GD&T and get **check-time legality diagnostics** for spec-invalid
   callouts (`Flatness(material_condition: MMC)` → `E_GdtIllegalModifier`; Concentricity →
   `W_GdtRemoved2018`).
2. Construct **real tolerance-zone solids** from a callout's effective zone width (Ø-cylinder,
   annulus, slab, per-face profile slab) and use them as ordinary geometry.
3. Derive **virtual/resultant condition** scalars on feature-of-size callouts and realize the
   **VC boundary solid**, proving worst-case assembleability against a mating part via the
   shipped clearance machinery — compile-time functional gauging, no measured data needed.
4. Check **geometric conformance**: `Conforms(tolerance: t, actual: deviated_copy)` measures
   max deviation of an actual feature against the nominal and reports
   Violated-with-magnitude / Satisfied / Indeterminate (kernel-less) in engine check output —
   exercised honestly via synthetic deviation (a transformed copy as `actual`).
5. (Once the datum lattice lands) anchor datum-referenced zones to projected datums and get
   DRF completeness diagnostics.

## 2. Background

The shipped surface is scalar end-to-end: `nominal_zone` is a width
(`crates/reify-compiler/stdlib/tolerancing.ri:46-51`), `Conforms` compares a hand-typed
`measured_deviation` against it (`:227-232`), `feature`/`datum_refs` are never dereferenced,
and the hierarchy admits spec-invalid callouts (form+MMC compiles; runout has no datum slot —
mandatory per Y14.5). Full survey evidence (4-strand: code, task graph, capabilities,
standards) is condensed in the design doc §7. The core parametric-tool tension — containment
is trivially-passing when actual == nominal — is resolved by the two design-side consumers
(VC boundary clearance proofs; synthetic deviation), with external metrology import explicitly
bookmarked (§6, companion stub PRD).

## 3. Pre-conditions for activating

**None blocking rungs 0–3** — all assumed substrate verified on main 2026-06-10:

- Zone primitives + booleans + sweep/pipe: `reify-ir/src/geometry.rs:529-839` (OCCT-backed).
- Geometry-builtin extension recipe: registry lists + lowering arms
  (`reify-compiler/src/geometry.rs:1730-1790`, `compile_geometry_call`), `GeometryOp` enum,
  eval dispatch (`reify-eval/src/geometry_ops.rs`). Nested composition proven
  (`examples/m5_geometry*.ri`).
- Sampled max-deviation primitive: `measure_mesh_deviation`
  (`reify-kernel-occt/src/ffi.rs:1068`, tess-QA-only today; this PRD promotes it).
- Engine measured-constraint pattern + C1 invariant: `RepresentationWithin` interception
  (`reify-eval/src/engine_constraints.rs:42-125`).
- Interference/clearance: `min_clearance` (task 2530, done). Point-contains (3611, done).
- Grammar: no novel syntax — both candidate fragments parse (`tree-sitter parse --quiet`
  exit 0, 2026-06-10): constraint param with expression default; trait/structure additions.
  `grammar_confirmed=true` for every task.

**Rung-4 tasks** (ι, κ) carry real out-of-batch dep edges on pending **4382/4385** (datum
lattice + feature→datum projections) and **4388** (DOF ledger) — filed `pending` per the
deferred-vs-pending norm; the scheduler holds them.

**Substrate corrections discovered during authoring** (bind the design):
- Param defaults compile in a **neutral scope** (`reify-compiler/src/functions.rs:95-119`) —
  no sibling-param references. `Conforms.actual` therefore uses static-binding detection + an
  inert marker default (§8 C3).
- Template-declared `.ri` constraints are **not enforced per ctor instance** (empirically:
  a `DimensionalTolerance` ctor violating its own guard passes `reify check` clean). Legality
  therefore lives in Rust-side diagnostics (β), not `.ri` guards. This per-instance gap is a
  general language finding, logged in §11 for separate triage — NOT owned by this PRD.
- Structures must re-declare inherited trait params; pinned defaults remain caller-overridable
  (`trait_requirements.rs`) — another reason legality is diagnostic-enforced.

## 4. Sketch of approach

Five rungs (design doc §3): **0** legality (.ri additive restructure + Rust check-time lint) →
**1** zone-region constructors as new *registered* geometry functions lowering to existing-op
compositions, plus ONE new kernel op (face-offset slab) → **2** VC/RC scalars + VC boundary
solids + clearance e2e → **3** max-deviation `GeometryQuery` + `measure_gdt_conformance`
engine pass + `Conforms.actual` → **4** datum-referenced zones + DRF diagnostics (gated on the
datum lattice). Conformance is **measured-deviation primary** (boolean emptiness is a test
oracle only — near-coincident booleans are the fragile case). The pass intercepts `Conforms`
instances and realizes handles itself — geometry in constraint *bodies* would evaluate in the
kernel-less P1 check phase (the structural trap that blocked #4275); pass interception is the
sanctioned shape (`RepresentationWithin`, `measure_dfm_rules` per
`process-dfm-overhang-draft.md` §2.2).

## 5. Resolved design decisions

D1–D8 in the design doc, each with declined alternatives. Headlines: engine/DSL only (no GUI —
follow-up PRD's territory, this slice is its named producer); legality via diagnostics
(substrate-forced; see §3); zone constructors registered-fn composition + one kernel op;
`nominal_zone` scalar unchanged (region constructors take it as input); VC depth = scalars +
boundary solids + clearance check; measured-deviation conformance feeding the shipped scalar
predicate; `Conforms.actual` optional param with static-binding detection; datum-free rungs
first, datum-referenced zones late-rung-gated; MMC **bonus stays hand-fed** this PRD
(`feature_departure` param unchanged — auto-computing departure from actual mating size is
follow-up scope, §11 Q4).

## 6. Out of scope

GUI/viewport GD&T rendering (zones/FCFs/datum symbols — future PRD, named consumer of rungs
1–3). Measured-data import — owned by the **deferred stub PRD**
`docs/prds/v0_6/gdt-measured-feature-import.md` (companion commit), which names
`Conforms.actual` (η) as its consumer seam and #4290 (PointCloud, deferred) as substrate; this
batch files only its bookmark task. Statistical stack-up extensions / GD&T→contributor bridge
/ Cpk. Composite FCFs, datum targets/simulators, MMB, projected zones Ⓟ, tangent plane Ⓣ,
free state Ⓕ, Rule #1 envelope vs ISO independency election. PMI/AP242 export (OCCT XDE makes
this cheap later — noted, not owned). SDF-route deviation (rides 4421–4427; this PRD's
deviation is B-rep-sampled). Per-instance `.ri` constraint enforcement (general language gap,
§11). Auto-derived stack-up chains.

## 7. Cross-PRD relationship

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| #4269 (deferred zone-REGION task) | absorbs | zone-region construction scope | **this PRD** (γ+δ) | decompose closes 4269 cancelled-superseded citing γ/δ |
| `geometric-relations.md` (4382/4385) | consumes | typed datums + feature→datum projections for ι/κ | **geometric-relations** | dep edges ι←4382,4385 (pending) |
| `geometric-relations.md` (4388) | consumes | DOF ledger for DRF-arrest diagnosis (κ) | **geometric-relations** | dep edge κ←4388 (pending) |
| `process-dfm-overhang-draft.md` (4408/4410) | sibling pattern | check-time measure-pass shape; each pass owns its OWN engine-integration-norm §3 entry | each PRD its own | **θ landed second** — §3.11 (GD&T conformance walk) authored by θ/4481, cross-references §3.8 (DFM walk, 4408 landed first); engine-integration-norm §11 cross-PRD row added |
| `gdt-measured-feature-import.md` (stub, companion) | produces | `Conforms.actual` (η) is the import PRD's consumer; #4290 named as its substrate | **stub PRD** (future) | bookmark task filed deferred |
| `tolerancing-gdt-surface-completion.md` | extends | scalar surface untouched except additive α/η edits | this PRD | shipped |
| kernel realization-tolerance subsystem; stack-up builtins | do-not-touch / read-only | — | — | firewalled (design doc §4) |

No new contested-ownership pair (checked against the breadcrumb-map §3 trio).

## 8. Contract section (H)

**C1 — Callout-instance enumeration (shared by β and η).** A *GD&T callout instance* is a
ctor-let or `sub` whose template conforms to `GeometricTolerance` (transitive refinement, the
`satisfies_trait_bound` walk). One enumerator, built in β, reused verbatim by η. Enumeration
is deterministic in declaration order; instances in unevaluated/dead branches are skipped.

**C2 — Legality rules (β).** Rule table keyed by characteristic family: MMC/LMC permitted
ONLY on FOS callouts (Position, StraightnessOfAxis, FOS orientation variants with
`zone_shape = Cylindrical`); runout/form/circularity/cylindricity/profile: RFS only →
`E_GdtIllegalModifier` (error). Concentricity/Symmetry → `W_GdtRemoved2018` (warning, names
position/profile/runout replacements). Diagnostics registered in
`reify-core/src/diagnostics.rs` with spans pointing at the instantiation site. Kernel-less:
legality needs only param values — always runs.

**C3 — `Conforms.actual` (η).** Signature: `param actual : Geometry = nominal()` where
`nominal()` is a new no-arg registered builtin returning an inert marker value; the `Conforms`
*body is unchanged* (never reads `actual`). The pass keys on the **statically-present explicit
`actual` argument binding** on the compiled constraint instance — never on the evaluated
default. No explicit binding → scalar path exactly as shipped. Explicit binding + kernel →
measure; explicit binding + no kernel → `Indeterminate` with a diagnostic naming the missing
kernel (C1 invariant: never a false `Violated`; mirror
`engine_constraints.rs:42-62`).

**C4 — Max-deviation query (ζ).** `GeometryQuery::MaxDeviation { actual, nominal, tolerance }`
→ `max_deviation(actual: Geometry, nominal: Geometry) -> Length` (SI metres): tessellate
`actual` at the engine's representation tolerance, sample ≥4 interior points/triangle, project
onto `nominal`'s exact B-rep, return the global max (promotes `measure_mesh_deviation`,
`ffi.rs:1068`). **Honest floor:** reported deviation carries ±(sample-spacing h + chord_tol);
tests assert inequalities against that stated floor, never exactness. Repr-gated to
B-rep-backed handles via the existing `QueryCapability` mechanism.

**C5 — Conformance pass (η).** `Engine::measure_gdt_conformance`, invoked from `Engine::check`
beside `check_constraints_against_templates`: enumerate `Conforms` instances with explicit
`actual` (C1+C3) → realize `tolerance.feature` and `actual` handles → `MaxDeviation(actual,
feature)` → substitute the measured value for `measured_deviation` in the shipped scalar
predicate → weave `ConstraintResult { satisfaction, diagnostics }` back **in caller order**
(the `dispatch_constraints` weave contract). Violated results carry the measured magnitude and
the zone width in the diagnostic message. The pass never mutates `achieved_repr_tol` or any
DFM state.

**C6 — Zone constructors (γ/δ).** Registered geometry fns (3-file recipe):
`zone_cylinder(axis: Geometry, width: Length)` (Ø-zone solid about an axis wire),
`zone_annulus(axis: Geometry, nominal_radius: Length, width: Length, length: Length)`,
`zone_slab(face: Geometry, width: Length)` (face-offset slab — the ONE new
`GeometryOp`/kernel op, OCCT offset-face-and-cap), `zone_profile(solid: Geometry, width:
Length)` (= `Difference(Thicken(+w/2), Thicken(−w/2))` composition). All take the callout's
`nominal_zone`-derived width as input; constructors do not read callout structs directly
(struct-arg lowering into geometry fns is unverified substrate — widths are plain `Length`
args).

## 9. Boundary-test sketch (H)

| # | Scenario | Preconditions | Postconditions |
|---|---|---|---|
| B1 | Producer→consumer: pass feeds predicate | kernel; example with explicit deviated `actual` (translate copy 0.5mm; zone 0.1mm) | `Conforms` Violated; diagnostic magnitude within ±(h+chord_tol) of 0.5mm |
| B2 | Conformant twin | same, `actual` = untransformed copy | Satisfied; no spurious Violated (near-zero deviation vs 0.1mm zone) |
| B3 | C1 invariant (consumer side) | NO kernel; explicit `actual` | `Indeterminate` + missing-kernel diagnostic; exit clean |
| B4 | Scalar path untouched (regression, both ways) | shipped `std_tolerancing_surface.ri` Test A/B unmodified semantics | MMC-vs-RFS flip + check gate green, byte-identical expectations |
| B5 | Query oracle vs boolean oracle | kernel; clearly-outside / clearly-inside fixtures | `MaxDeviation` verdict agrees with `Difference(actual, zone)+Volume≈0` on both |
| B6 | Zone volume oracles | kernel | `zone_cylinder`: V=π/4·d²·L analytic (1e-9 rel, GProp on analytic B-rep); `zone_slab` planar face: V=w·A exact-planar identity |
| B7 | Legality lint fires per instance | fixture: `Flatness(material_condition: MMC)` ctor-let | `E_GdtIllegalModifier` at the instantiation span; RFS twin silent |
| B8 | VC clearance both verdicts | kernel; bolt-pattern example, conformant + under-clearanced variants | conformant: `min_clearance(vc_boundary, mating) > 0` Satisfied; variant Violated |
| B9 | Pass ordering / weave | example mixing scalar Conforms, geometric Conforms, RepresentationWithin | results in caller order; neither pass perturbs the other's results |

θ (integration gate) owns the full matrix as its observable signal.

## 10. Decomposition plan

Intra-batch deps by letter; out-of-batch by ID. All tasks `grammar_confirmed=true`.

- **α — Restructure tolerancing.ri: legality-bearing types** (additive). Runout gains required
  `datum_refs`; `Straightness` → surface-line semantics + new `StraightnessOfAxis` (FOS,
  MMC-eligible); `ProfileOfSurface`/`ProfileOfLine` split datum-less vs `…Related` variants;
  `ZoneShape {Width, Cylindrical}` enum + `zone_shape` on FOS location/orientation callouts;
  doc-comments reconciled. Modules: reify-compiler stdlib + tests. **Signal:** updated
  `std_tolerancing_surface.ri` exercises the new types; CI Test A/B green; template-count
  assertions updated. Deps: —.
- **β — Check-time GD&T legality diagnostics** (Rust). Callout-instance enumerator (contract
  C1) + rule table (C2) + `E_GdtIllegalModifier`/`W_GdtRemoved2018` in
  `reify-core/src/diagnostics.rs`. Modules: reify-eval or reify-compiler check path,
  reify-core, reify-cli tests. **Signal:** committed fixture with `Flatness(MMC)` → `reify
  check` emits `E_GdtIllegalModifier` (B7); Concentricity fixture → `W_GdtRemoved2018`.
  Deps: α.
- **γ — Prismatic zone constructors** (`zone_cylinder`, `zone_annulus`, `zone_profile` —
  composition-only, contract C6). Modules: reify-compiler geometry registry, reify-eval
  dispatch, example. **Signal:** CI example realizes zones; B6 cylinder volume oracle.
  Deps: α.
- **δ — Face-offset-slab kernel op + `zone_slab`**. New `GeometryOp` variant + OCCT impl +
  dispatch + registry. Modules: reify-ir, reify-kernel-occt (cpp+rs), reify-eval,
  reify-compiler. **Signal:** B6 planar-slab volume identity in CI; curved-face smoke
  (non-failure + volume>0). Deps: α.
- **ε — VC/RC scalars + VC boundary + clearance e2e**. `virtual_condition`/
  `resultant_condition` fns (size `DimensionalTolerance` × geo callout → Length, exact
  arithmetic) + VC boundary solid example (axis wire + `zone_cylinder` at VC Ø) + B8
  bolt-pattern example. Uses `min_clearance` (2530, done). Modules: stdlib .ri, examples,
  reify-cli tests. **Signal:** B8 both verdicts in CI. Deps: α, γ.
- **ζ — `MaxDeviation` GeometryQuery** (contract C4; promotes `measure_mesh_deviation`).
  Modules: reify-ir, reify-kernel-occt, reify-eval, units signature table. **Signal:** CI
  test: `max_deviation(translate(box,0.5mm,0,0), box)` reports 0.5mm within stated
  ±(h+chord_tol) floor — floor documented in the test (G6: bound ≫ floor). Deps: —.
- **η — `measure_gdt_conformance` pass + `Conforms.actual` + `nominal()` marker** (contracts
  C3+C5). Modules: reify-eval engine, stdlib tolerancing.ri (one param), reify-stdlib
  (marker builtin), reify-cli tests. **Signal:** B1+B2+B3 in CI (Violated-with-magnitude /
  Satisfied / kernel-less Indeterminate). Deps: β, ζ.
- **θ — Integration gate + norm entry** (**critical leaf**). Full §9 matrix as one CI suite
  (incl. B4 regression, B5 boolean-oracle cross-check, B9 weave); authors the GD&T pass's
  engine-integration-norm §3.11 entry (citing §3.8, 4408/DFM walk, which landed first); PRD/doc
  reconcile. **Signal: DELIVERED (2026-06-15)** — `crates/reify-cli/tests/cli_gdt_integration_gate.rs`
  green in CI (B4/B5/B9 direct; B1–B3/B6–B8 mapped); §3.11 entry committed and cross-referenced
  to §3.8. Task θ/4481 done. Deps: γ, δ, ε, η.
- **ι — Datum-anchored zones**. Position/orientation/runout zones anchored to projected datum
  axis/plane (consumes 4385 projections + 4382 Direction); `datum_refs` accepts lattice
  datums. **Signal:** e2e datum-anchored position-zone example green in CI. Deps: γ, η,
  **4382, 4385** (out-of-batch, pending).
- **κ — DRF ordering + DOF-arrest diagnostics**. Ordered primary/secondary/tertiary refs;
  per-characteristic DOF-arrest completeness check consuming 4388's ledger; diagnostic for
  incomplete/redundant DRFs. **Signal:** DRF-incompleteness fixture emits the diagnostic in
  `reify check`. Deps: ι, **4388** (out-of-batch, pending).
- **BM — Bookmark: measured-feature import** (filed **deferred** — genuine
  forward-stub gate). Points at `gdt-measured-feature-import.md`; names η as consumer seam and
  #4290 as substrate. No implementation.

Manifest-binding seeds (decompose builds the manifest from these): registry recipe
`reify-compiler/src/geometry.rs:1730-1790`; `measure_mesh_deviation` `ffi.rs:1068`;
C1-invariant `engine_constraints.rs:42-125`; neutral-scope defaults `functions.rs:95-119`;
`min_clearance` ← task 2530 (done); grammar fixtures re-extractable from §8 (both passed
2026-06-10); numeric floors as stated per-signal (B6 identities, ζ floor, ε exact arithmetic).

## 11. Open questions (tactical)

1. **Exact OCCT recipe for the face-offset slab** (δ): `BRepOffsetAPI_MakeOffsetShape` on a
   face vs offset-surface + `ThruSections` capping. Decide in δ; volume identities in B6 are
   the acceptance bar either way.
2. **Marker builtin name** (`nominal()` vs `as_designed()`): pick in η; must read naturally at
   `Conforms(tolerance: t, actual: nominal())`.
3. **Diagnostic code spellings** (`E_GdtIllegalModifier` etc.): finalize against
   `reify-core/src/diagnostics.rs` naming conventions in β.
4. **Auto-computed MMC bonus** from actual mating size (replacing hand-fed
   `feature_departure`): follow-up scope once measured/as-built features exist; do not attempt
   in η.
5. **Per-instance `.ri` template-constraint enforcement** (discovered gap, §3): general
   language semantics question — surface to Leo for separate triage; this PRD neither fixes
   nor depends on it.
6. **Zone-constructor ergonomics** (callout-struct-aware sugar once struct-arg lowering into
   geometry fns is verified substrate): revisit after θ.
