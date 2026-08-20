# PRD: Hex and Wedge Meshing for Swept Geometries

Status: deferred — candidate v0.3.x. Partial answer to thin-body FEA before shells (`structural-analysis-shells.md`) ship in v0.4. Filed 2026-05-02 from FEA PRD spillover.
Design resolved 2026-05-04 — see "Resolved design decisions" below.
Addendum 2026-07-04 — Phase A activation under the v0.6 realization read-back architecture; see the final section "## Addendum (2026-07-04)". The original "Cache-key handling" / "swept_kind is light-touch metadata" decisions are partially superseded there.

## Goal

Add hexahedral (8-node) and wedge / triangular-prism (6-node) elements for swept geometries — extrudes, revolves, single-profile lofts. These element types handle thin features dramatically better than tets without requiring 2D shell formulation.

## Background

The v0.3 FEA PRD (`structural-analysis-fea.md`) ships tet-only FEA. Tets are universal but inefficient for swept geometries:

- A simple extruded thin plate (say 100×100×2 mm) needs hundreds of thousands of tets to mesh well, but only a few thousand prisms or hexes — 50–100× element-count reduction.
- Tets in swept geometry suffer the same shear-locking issues as tets anywhere; hexes / prisms have better bending behavior even at P1 order.
- Mesh quality is naturally controlled when sweeping a 2D base mesh along an axis — no aspect-ratio surprises.

Most CAD-FEA tools have specialized swept-feature recognition that triggers prismatic meshing. Reify can do the same: detect swept features, generate a 2D mesh on the cross-section, sweep it along the axis to produce wedge or hex elements.

This is a smaller scope than shell elements — no new formulation surface, no mid-surface extraction problem — but addresses a meaningful subset of the thin-body pain. Shells remain the proper general fix.

## Why deferred (and why separate from FEA PRD)

- v0.3 FEA PRD is already 22 tasks; pulling hex/wedge in would expand it materially.
- Detection heuristic for "swept" features is a non-trivial geometry-kernel addition.
- Element kernel work is small but distinct from tet kernel — better to land tet-only first, validate, then add hex/wedge as a focused addition.
- Some user pain may be addressable by P2 tets + thin-body diagnostic in v0.3; need to see how much before committing to hex/wedge work.

## Sketch of approach

1. **Sweep detection** — geometry-kernel pass walks the construction history of each body and tags it with `swept_kind = Extrude { axis, length } | Revolve { axis, angle } | SweepLinear { profile, path }` when the body is the result of exactly one extrude/revolve/single-profile-loft op with no subsequent modifications. (`SweepLinear` covers non-twisted single-profile lofts specifically; twisted lofts fall back to tet meshing.) The tag persists on the realized body so other systems (mesh morphing, GUI) can read it.
2. **Sweep meshing** — for a tagged swept body, generate 2D mesh on cross-section via Gmsh, then sweep that 2D mesh along the axis to produce wedge (from triangle base) or hex (from quad base) elements ourselves. Element count = base_mesh_count × sweep_subdivisions.
3. **Element kernel** — implement P1 hex (8-node) and P1 wedge (6-node) reference elements + stiffness assembly in `reify-solver-elastic`. P2 variants (20-node hex, 15-node wedge) deferred unless demand surfaces.
4. **Mixed-element assembly** — global assembly path needs to handle hex and wedge alongside tet, since multi-body assemblies routinely combine swept thin parts with tet-meshed blocky parts. Within a single body, only one element type is used (no within-body mixing in v0.3.x).
5. **Body-level only, no within-body coupling** — when a body qualifies as fully swept, mesh it as hex/wedge; otherwise tet-mesh the whole body. Body-to-body coupling in assemblies uses the existing kinematic-constraints / shared-face mechanism, not new pyramid or mortar infrastructure.

User-visible: hex/wedge promotion is automatic when detection succeeds. `ElasticOptions.force_tet = true` disables it (debugging/comparison); `ElasticOptions.require_hex_wedge = true` makes any fall-back a hard error (user wants to know if their geometry doesn't qualify).

## Pre-conditions for activating

- v0.3 FEA kernel (tet path) shipped and validated.
- v0.2 multi-kernel mesher landing (so the meshing pipeline is in a place to extend).
- Some user signal that thin-feature pain is biting hard enough to justify the work before shells are ready.

## Resolved design decisions (2026-05-04)

**Detection scope: phased, narrow first cut.** The driving question was how strict to be — pure-sweep bodies are rare in practice, but anything broader buys into within-body region coupling, which is shells-PRD-scale work.

| Phase | What qualifies | Coupling work | Ships in |
|---|---|---|---|
| **A. Pure-sweep body** | Whole body = exactly one extrude/revolve/single-profile-loft op, no further mods | None | v0.3.x |
| **B. Sweep-with-axial-finishing** | Sweep + finishing ops that preserve sweep direction (through-holes parallel to sweep axis, end-face fillets/chamfers) | None — 2D cross-section accommodates holes | v0.3.x follow-on |
| **C. Region-level swept** | Body has swept region + non-swept region | Heavy (pyramid transitions or mortar/MPC) | Out of scope; waits for shells |

Phase A is the v0.3.x first cut. Detection is unambiguous; meshing produces a guaranteed-clean result. Most real CAD parts won't qualify, and that's stated honestly. Phase B follows once Phase A is validated and is where most user value lives. Phase C is explicitly deferred to the shells-PRD timeframe — within-body region coupling needs the same multi-region infrastructure shells will need anyway.

**Coupling at non-swept boundaries: only between bodies, never within a body.** Under Phase A/B, a body is either fully swept or fully tet-meshed; coupling at non-swept boundaries only arises in multi-body assemblies and is handled by the existing kinematic-constraints / shared-face mechanism. No new pyramid kernel, no MPC infrastructure, no interface tri-splitting.

**Meshing implementation: base-mesh-then-sweep, full stop.** Use Gmsh only for the 2D cross-section mesh; perform the sweep step ourselves. Reasoning: Gmsh's transfinite mode requires us to set up input topology in a way Gmsh recognizes as transfinite-eligible, which after we've already done sweep detection ourselves is duplicate work; the sweep step is mechanical (~100 lines for connectivity construction); we control mesh-density-along-sweep directly, which is the key user-facing knob; for revolves we sweep along an arc, for Phase B's perforated cross-sections the same path works without modification. The PRD's earlier "Gmsh transfinite where applicable, fall back otherwise" lean was the worst of both worlds — two paths to maintain plus a classifier to decide between them.

For hex generation, the 2D mesh is produced as a quad mesh (Gmsh's recombine algorithm combines triangles into quads); for wedge generation, the 2D mesh is left as triangles. Selection is automatic per body — hex preferred, wedge fallback when the cross-section doesn't recombine cleanly.

**P1 only for v0.3.x.** P1 hex (8-node) is genuinely competitive with P2 tet for thin-body bending — not a downgrade. P1 wedge is weaker than P1 hex but still better than P1 tet on swept geometry. P2 hex/wedge waits until concrete demand surfaces (mostly stress-concentration accuracy, rare in genuinely thin parts).

**`ElasticOptions.element_order = P2` interaction.** When the user sets P2 globally and a body qualifies for hex/wedge promotion, produce P1 hex/wedge anyway and emit a one-shot info diagnostic: "Body X qualified for hex/wedge meshing; P1 hex used despite `element_order = P2` (P2 hex deferred). Accuracy for thin geometry is comparable to P2 tet." Honors the auto-detect contract while being honest about the substitution.

**Failure-mode policy: silent fall-back to tet, not refusal.** Hex/wedge promotion is a transparent optimization. When detection or meshing fails, fall back to tet with an info-level diagnostic — never an error from this path by default.

Distinct fall-back causes worth naming separately in the diagnostic:

- Twisted linear sweep / non-orthogonal sweep path — body construction qualifies syntactically, but the sweep is geometrically ill-defined.
- Cross-section 2D meshing failed — degenerate corners, slivers in the profile.
- Body has post-sweep modifications (Phase A only) — drilled holes, fillets, etc. Diagnostic notes Phase B as the path that will eventually handle this case.

**Two opposing escape hatches in `ElasticOptions`:**

- `force_tet: bool` (default false) — disables hex/wedge promotion entirely. Debugging, validation comparisons, A/B accuracy studies.
- `require_hex_wedge: bool` (default false) — fall-back becomes a hard error. "Tell me if my geometry doesn't qualify" mode for users tuning thin-body performance.

**`swept_kind` tag persists on the realized body.** Not just consumed locally by meshing — surfaced as metadata so the GUI can display "hex/wedge meshed" status, and so mesh-morphing can preserve swept topology across small parameter changes. Light-touch metadata, not a new `ReprKind` variant.

**Cache-key handling.** No new `ReprKind` variant; volume meshes remain `VolumeMesh`. Element-type composition (hex/wedge vs. tet) is determined by the body's construction-history hash (which feeds the geometry hash) plus `force_tet` (already an `ElasticOptions` field). The geometry+options hash uniquely determines mesh element type, so no separate cache entries for "tet vs. hex/wedge of the same body" are needed.

**Mesh-morphing assumption acknowledged.** The morph PRD treats swept-mesh preservation as a load-bearing benefit. This only works if a small parameter change preserves swept topology — i.e., the construction-history pass re-classifies the morphed body the same way and the 2D cross-section mesh can be morphed in lockstep with the 3D sweep. Validating this claim is on the morph PRD, not this one, but flagged here so it isn't silently assumed.

**Validation strategy.** Re-run the FEA PRD's analytical-validation cases that *are* swept bodies (cantilever beam if extruded, pressurized thick-walled cylinder) with hex/wedge meshing and assert the convergence-vs-DOF curve is materially steeper than tet at equal DOF. Adds "hex/wedge converges faster than tet at equal DOF on swept geometry" as a regression-tested claim — gives concrete confidence the optimization is doing real work, beyond just "doesn't crash."

## Open design questions

- **Diagnostic phrasing for the Phase A → Phase B transition.** When a body has post-sweep modifications and falls back to tet, the diagnostic should mention that Phase B will eventually handle this case. Exact phrasing decided at task implementation time, not up-front.

## Decomposition plan

Thirteen tasks for the v0.3.x first cut (Phase A); one further task for the Phase B follow-on. Gates on FEA PRD tasks (tet kernel, mesher, ElasticOptions infrastructure) and on v0.2 multi-kernel.

**Detection (geometry-kernel side, independent of element kernel):**

1. Sweep classifier pass: walks the construction history of a `Body` and returns `Option<SweptKind>` where `SweptKind = Extrude { axis, length } | Revolve { axis, angle } | SweepLinear { profile, path }`. Phase A only — single extrude/revolve/single-profile-loft op with no subsequent modifications. Includes geometric validity check (orthogonal sweep direction for extrude, non-degenerate revolve angle, non-twisted linear sweep). Tag persists as metadata on the realized body so morphing and GUI can read it.

**Element kernel (extends `reify-solver-elastic`, depends on FEA tet kernel landed):**

2. P1 hex (8-node) reference element: shape functions, gradients, Gauss quadrature (2×2×2). Constitutive integration under isotropic linear elastic.
3. P1 wedge (6-node) reference element: shape functions, gradients, quadrature (triangle × line). Constitutive integration under isotropic linear elastic.
4. Element-level stiffness assembly for hex and wedge under isotropic linear-elastic constitutive law (engineering strain, Voigt notation). Shares the constitutive evaluator with the existing tet path (FEA task 8); the difference is the element-shape integration.
5. Mixed-element global assembly: extend the FEA assembly path (FEA task 9) to dispatch over element type per body. Includes Neumann BC integrals over quad faces (hex/wedge surfaces) in addition to the existing triangle-face path (tets).

**Meshing (depends on classifier + Gmsh integration from FEA task 17):**

6. 2D cross-section extraction + Gmsh 2D meshing: given a `swept_kind`-tagged body, extract the cross-section profile (curve set) and feed to Gmsh as a 2D meshing request. Triangle output for wedge target; quad output via Gmsh recombine algorithm for hex target. Hex preferred; wedge fallback when recombine doesn't produce a clean quad mesh.
7. Sweep step: given a 2D mesh and sweep parameters (axis+length for extrude, axis+angle for revolve, profile+path for linear sweep), generate the 3D node grid and wedge/hex connectivity. K layers controlled by `ElasticOptions.mesh_size` derivation (same auto-from-feature-size logic as FEA task 17) or explicit `sweep_subdivisions` override. Through-thickness check: at least 2 layers across the sweep direction by default; warn otherwise.
8. Volume-mesh integration: hook the swept-mesh path into the realization pipeline. When sweep classifier returns `Some(_)` AND `force_tet = false` AND 2D mesh succeeds AND sweep step succeeds, return swept hex/wedge mesh; else fall back to tet path. Cache key composition unchanged — element-type composition is determined by geometry hash + `force_tet`.

**Options + diagnostics:**

9. Extend `ElasticOptions` with `force_tet: bool` (default false) and `require_hex_wedge: bool` (default false). Wire into mesh-selection logic in task 8. Mutually exclusive — setting both is an `ElasticOptions` validation error.
10. P2 element-order interaction: when `element_order = P2` is set and a body qualifies for hex/wedge promotion, produce P1 hex/wedge and emit one-shot info diagnostic per body. Document the substitution in the PRD-derived stdlib doc.
11. Fall-back diagnostic mapping: distinct diagnostics for each fall-back cause — twisted linear sweep / non-orthogonal sweep path, cross-section 2D meshing failure, post-sweep modifications detected (Phase B note), `force_tet = true` (debug-only suppression of promotion), and the success path ("Body X meshed as N hex / M wedge"). Info-level by default; `require_hex_wedge = true` upgrades fall-backs to errors.

**Validation:**

12. Extend FEA validation suite: re-run the swept analytical cases (cantilever beam as extruded body, pressurized thick-walled cylinder as revolved body) with hex/wedge meshing. Assert (a) numerical agreement with the analytical reference within tolerance, and (b) convergence-vs-DOF curve is materially steeper than the same case with tet meshing. Regression test enforces both. **Gate:** FEA PRD validation suite (FEA task 20) shipped first.
13. Mesh-quality and structural test: synthetic swept-body fixtures (extruded plate, revolved disc, simple non-twisted linear sweep). Assert hex/wedge generation succeeds, element count matches `base_mesh_count × sweep_subdivisions`, through-thickness check is satisfied at default `mesh_size`, and `force_tet`/`require_hex_wedge` produce expected diagnostics.

**Phase B follow-on (deferred, recorded but not in v0.3.x first cut):**

14. Sweep classifier extension: recognize axial-finishing operations (through-holes parallel to sweep axis, end-face fillets, sweep-axis-aligned chamfers) as topology-preserving and continue to tag the body as `swept_kind`. The 2D cross-section extracted in task 6 now reflects holes (a multiply-connected 2D region); sweep step in task 7 is unchanged. **Gate:** Phase A (tasks 1–13) shipped and validated.

## Out of scope for this PRD

- Hex meshing of arbitrary geometry (not swept) — much harder problem; deferred indefinitely.
- Region-level swept meshing within a single body (Phase C above) — needs pyramid transitions or mortar/MPC infrastructure; waits for shells PRD timeframe.
- Pyramid elements (5-node transition between tet and hex) — useful for region-level mixing; defer until concrete need.
- Within-body mixed-element meshing — bodies are fully swept or fully tet-meshed in v0.3.x.
- Hex meshing of swept-with-twist features (lofts with rotation) — sweep can't handle cleanly; remains tet.
- Shell elements — sibling PRD (`structural-analysis-shells.md`, v0.4), proper general fix for thin bodies.
- P2 hex/wedge elements — defer until P1 is shipped and demand surfaces.

## Relationship to other PRDs and tasks

- **Companion to `structural-analysis-fea.md`** — extends the same kernel and pipeline; only adds new element types and a sweep-detection geometry pass. Reuses BCs, materials, options, validation framework. Gates on FEA tet kernel + mesher + `ElasticOptions` infrastructure being in place.
- **Partial alternative to `structural-analysis-shells.md` (v0.4)** — addresses a subset of thin-body cases (those backed by swept geometry) with much smaller engineering scope. Shells still required for non-swept thin features (general sheet metal, casings, region-level mixed thin/blocky bodies). The shells PRD already lists hex/wedge as a "partial overlap" — the cross-reference resolves both ways.
- **Carries a load-bearing assumption for `mesh-morphing.md`** — the morph PRD treats swept-mesh preservation as a primary motivator. That assumption is only valid if a small parameter change preserves the body's `swept_kind` classification and the 2D cross-section morphs in lockstep with the 3D sweep. Validation of that bi-directional claim lives in the morph PRD's task list, not here.
- **Touches v0.2 multi-kernel** — extends the mesher capability descriptor (`multi-kernel.md`) with new `(operation, repr_kind)` tuples for hex/wedge volume meshing.

## Addendum (2026-07-04): Phase A activation under the v0.6 realization read-back architecture

**Approach: B + H** (contract + boundary-test sketch below). Trigger: touches two G5 load-bearing seams — FEA and realization-kind dispatch (`engine-integration-norm.md` §3.2).

### Why this addendum

Phase A's 13 tasks (1–13 above → tasks 2982–2994) all landed `done`. But **task 8** ("Volume-mesh integration", → task 2989) shipped only the `dispatch_volume_mesh` gate/truth-table (`engine_build.rs`) plus `sweep_2d_mesh_to_3d` (`reify-solver-elastic/src/sweep.rs`) — **with no live production caller**. It predates the **v0.6 realization-read-api** batch (tasks 4507/4508/4509/4743, all `done`), which is the architecture this PRD never saw:

- v0.6 inserted a **typed content read-back layer** between mesh producer and FEA consumer — `RealizedContent` (`crates/reify-compute-contract/src/realization.rs`) + `reify_ir::VolumeMesh` (`crates/reify-ir/src/geometry.rs`), both delivered **tet-only**. `volume_mesh()` projection (`crates/reify-eval/src/realization_content.rs`) reads back tet only.
- Task **4091** made the elastic solve consume the **realized** `VolumeMesh` (replacing the synthetic box). So a swept mesh must now flow **through** the read-back layer to reach the solver — it can no longer be produced-and-consumed transiently in one place.
- Realization **demand** (`compute_demanded_reprs` / `realization_indices_where`, `engine_build.rs`) is computed at build time, **before** compute-node dispatch and before the runtime `ElasticOptions` value exists.

Net: the leaf Phase A signal ("a pure-swept body under an elastic FEA solve realizes as a hex/wedge `VolumeMesh`") is **unreachable** on main. Two substrate capabilities — never specified by this PRD because they are consequences of v0.6, and owned by no task — are missing: (1) hex/wedge **storage + read-back**, and (2) **element-type selection** at the build-time production edge. Task **4743** (which gave `dispatch_volume_mesh` its live *tet-path* caller, 2026-06-24) had to hardcode `swept_kind=None, force_tet=true` with `unreachable!()` swept closures **precisely because** capability (1) does not exist (comment at `engine_build.rs` ~7631: "a swept hex/wedge mesh has no `volume_mesh()` tet read-back projection, so it is NOT stored"). Task **4746** was filed to "activate" Phase A but its RED premise is unreachable without this substrate — the architect correctly blocked it (esc-4746-43 / esc-4746-46; provenance esc-2995-194 NO-GO). This addendum closes the gap.

### What of the original PRD stands / is superseded

- **Stands:** "No new `ReprKind` variant; volume meshes remain `VolumeMesh`" (§ Cache-key handling). We extend `VolumeMesh` **in place**. Element-type is still determined by geometry hash + `force_tet`; no separate cache entries.
- **Superseded:** "`swept_kind` is light-touch metadata, consumed locally by meshing." Under v0.6 the **mesh itself** must carry element connectivity through the read-back layer; the `swept_kind` tag remains but is no longer the transport for element type.

### Resolved design decisions (2026-07-04)

1. **Storage shape: a `VolumeConnectivity` enum inside `reify_ir::VolumeMesh`.** `RealizedContent` keeps its single `VolumeMesh` arm (honors "remain `VolumeMesh`"). One element family per mesh — no within-body mixing (matches § "Body-level only"). Illegal states unrepresentable; the solver gets a single match point. Cost accepted: existing tet readers migrate to `VolumeConnectivity::Tet`.
2. **Element-type selection: a realization-demand attribute, read statically from the demanding FEA call.** Demand carries `{HexPreferred | TetForced | HexRequired}` per demanded `VolumeMesh`, derived by statically reading `force_tet`/`require_hex_wedge` off the demanding `elastic_*` call's `ElasticOptions` argument (reusing the existing consumer→producer static match in `realization_indices_where`). The production edge honors that preference together with a `swept_kind_table` lookup (already populated at `engine_build.rs` ~7216). This is the only way to honor `force_tet` at all: the solver reads a *realized* mesh and has no geometry to re-tet from, so the tet-vs-hex/wedge choice must be made at build time.
3. **Data-dependent (non-literal) options degrade, they don't block.** When `force_tet`/`require_hex_wedge` are not compile-time constants (e.g. bound to a computed expression), demand cannot resolve them statically → default to `HexPreferred` + emit a one-shot info diagnostic ("`force_tet`/`require_hex_wedge` ignored for element selection: not a compile-time constant"). Full runtime honoring (a post-build realization redispatch) is **deferred** to a follow-up — see Out of scope. This covers the literal-option case, which is essentially all real usage.

### Contract section (B + H)

**C-1 — `VolumeConnectivity` on `reify_ir::VolumeMesh`** (owned by N1):

```rust
// crates/reify-ir/src/geometry.rs
pub enum VolumeConnectivity {
    Tet   { indices: Vec<u32>, order: ElementOrderTag }, // P1 (4/elem) or P2 (10/elem)
    Hex   { indices: Vec<u32> },                         // P1 8-node (8/elem)
    Wedge { indices: Vec<u32> },                         // P1 6-node prism (6/elem)
}
pub struct VolumeMesh {
    vertices: Vec<f32>,
    connectivity: VolumeConnectivity, // replaces flat tet_indices + element_order
    normals: Option<Vec<f32>>,
    boundary: Option<BoundaryAssociation>,
}
```
Invariants: exactly one element family per mesh; `Hex`/`Wedge` are P1-only in v0.3.x (P2 hex/wedge remain out of scope); `vertices` layout unchanged. A `From<SweptMesh3d> for VolumeMesh` (or equivalent conversion fn) maps `SweptConnectivity::{Hex,Wedge}{indices}` → `VolumeConnectivity::{Hex,Wedge}{indices}` — this is the "`SweptMesh3d → VolumeMesh` wrap" flagged unbuilt at `reify-solver-elastic/src/lib.rs` ~706. A convenience accessor (e.g. `VolumeMesh::tet_indices() -> Option<&[u32]>`) bounds tet-reader churn.

**C-2 — `volume_mesh()` projection carries connectivity** (owned by N1): `GeometryKernel::volume_mesh(handle)` / the `ReprKind::VolumeMesh` arm of `realization_content.rs` must be able to emit a `RealizedContent::VolumeMesh` whose `connectivity` is `Hex`/`Wedge`, not only `Tet`.

**C-3 — FEA consumer dispatches on connectivity** (owned by N1): the elastic assembly path (`reify-solver-elastic`, the consumer wired by 4091) matches on `VolumeConnectivity` and routes to the P1 hex / P1 wedge stiffness assembly already built by Phase A tasks 2–5 — so a *realized* hex/wedge `VolumeMesh` assembles and solves.

**C-4 — demand element-type preference** (owned by 4746): `volume_mesh_demanded_indices` returns `HashMap<usize, ElementTypePref>` (was `HashSet<usize>`), where `enum ElementTypePref { HexPreferred, TetForced, HexRequired }`. `TetForced`/`HexRequired` are mutually exclusive by the existing `!(force_tet && require_hex_wedge)` constraint (`solver_elastic.ri:385`).

**C-5 — production-edge selection truth table** (owned by 4746; at the `dispatch_volume_mesh` call, `engine_build.rs` ~7598 — replaces the hardcoded `None`/`force_tet=true`/`unreachable!()`):

| `swept_kind` | pref | outcome |
|---|---|---|
| `Some(_)` | `HexPreferred` | hex/wedge (swept path) |
| `Some(_)` | `HexRequired` | hex/wedge |
| `Some(_)` | `TetForced` | tet |
| `None` | `HexPreferred` | tet (silent fall-back, info diagnostic) |
| `None` | `TetForced` | tet |
| `None` | `HexRequired` | **hard error** (`require_hex_wedge` on non-swept body) |

Internal `dispatch_volume_mesh` fall-backs (2D-mesh-fail, sweep-fail) already degrade to tet unless `HexRequired` (→ error), per the truth table task 8 shipped. The success + per-cause fall-back diagnostics (task 2992, `diagnostics.rs` ~3246) and the P2→P1 substitution info diagnostic (`p2_substitution_diagnostic`) fire as already built.

### Boundary-test sketch (B + H)

| # | Scenario | Preconditions | Postconditions (asserts) | Faces |
|---|---|---|---|---|
| P1 | dispatch produces hex from a recombined swept quad section | `swept_kind=Some(Extrude)`, 2D quad mesh ok | `VolumeMeshOutcome::Swept(SweptConnectivity::Hex)` | producer (N1/existing) |
| P2 | Swept outcome wraps into `VolumeMesh(Hex)` | a `SweptMesh3d` with `Hex` | `RealizedContent::VolumeMesh`, `connectivity == Hex`, vertex count preserved | producer (N1) |
| C1 | solver assembles a realized hex `VolumeMesh` | `RealizedContent::VolumeMesh(Hex)` | assembly dispatches the hex path; solve completes | consumer (N1) |
| C2 | solver assembles a realized wedge `VolumeMesh` | `...(Wedge)` | assembly dispatches the wedge path | consumer (N1) |
| E1 | **leaf:** pure-swept extrude under FEA default options → hex/wedge | `.ri`: single `extrude` + `elastic_static(...)` default `ElasticOptions()` | realized `VolumeMesh` connectivity is `Hex`/`Wedge` (observed via debug MCP / `reify eval`); success info diagnostic fires | e2e (4746) |
| E2 | **leaf (rejection):** `require_hex_wedge=true` on a non-swept body → hard error | `.ri`: non-swept body + `ElasticOptions(require_hex_wedge: true)` | realization hard-errors with the `require_hex_wedge` diagnostic (observed to fire) | e2e (4746) |
| E3 | **leaf:** `force_tet=true` on a swept body → tet | `.ri`: `extrude` + `ElasticOptions(force_tet: true)` | realized `VolumeMesh` connectivity is `Tet` | e2e (4746) |

Rows E1–E3 are 4746's `user_observable_signal` (G2 integration-gate); C1–C2 face N1's consumer side; P1–P2 the producer side. E2 is the G6-branch-4 rejection binding — the capability manifest must show the `require_hex_wedge` rejection **observed to fire**, not merely declared.

### Decomposition (this addendum — completes task 8 under v0.6)

Two tasks. N1 is a foundation/intermediate roped into 4746's leaf (C-as-integration-gate); 4746 is re-scoped from a bare wiring task to the activation leaf and **re-gated** onto N1.

- **N1 — Hex/wedge storage + read-back substrate** (`reify-ir`, `reify-compute-contract`, `reify-eval`, `reify-solver-elastic`). Deliver C-1/C-2/C-3: `VolumeConnectivity` on `VolumeMesh` + `From<SweptMesh3d>`; `volume_mesh()`/`realization_content.rs` projection carries connectivity; elastic assembly dispatches on `VolumeConnectivity` so a realized hex/wedge `VolumeMesh` solves. **Signal:** intermediate — unlocks 4746; boundary rows P2/C1/C2 green. (No standalone user-observable signal; roped into 4746.)
- **4746 (re-scoped) — Phase A production-edge activation** (`reify-eval/src/engine_build.rs`, `reify-audit/tests/engine_seam_orphans_g_allow.rs`, new e2e under `reify-eval/tests/`). Deliver C-4/C-5: demand carries `ElementTypePref` (static read of `force_tet`/`require_hex_wedge`, non-literal → `HexPreferred`+diagnostic); production edge consults `swept_kind_table` + pref and calls `dispatch_volume_mesh` with real args (drop the hardcoded `None`/`force_tet=true`/`unreachable!()`); **remove `dispatch_volume_mesh` AND `sweep_2d_mesh_to_3d` from the engine-seam orphan allow-list** (concrete done-signal). **Signal (leaf, G2):** boundary rows E1/E2/E3 — a pure-swept body realizes hex/wedge under default FEA options; `require_hex_wedge=true` on a non-swept body hard-errors; `force_tet=true` yields tet. **Depends on:** N1, and (existing) 4091/4509/4508.

Downstream **task 2995** (Phase B, `pending`) stays dep-wired on 4746 — its flip condition ("a pure-swept body verifiably realizes as a hex/wedge `VolumeMesh`") is satisfied by 4746's leaf, so Phase B is not stranded.

### Cross-PRD relationship (addendum seams)

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `structural-analysis-fea.md` (task 4091) | consumes | realized `VolumeMesh` → elastic assembly | this-prd (N1 extends the consumer) | queued (N1) |
| `engine-integration-norm.md` §3.2 | plugs into | realization-kind dispatch (`dispatch_volume_mesh`) | this-prd (4746) | queued (4746) |
| task 2995 (Phase B) | produces-for | Phase-A validated hex/wedge realization | this-prd (4746 leaf) | pending, dep-wired |
| `mesh-morphing.md` | produces-for | `swept_kind` preservation across morph | mesh-morphing (validation lives there) | unchanged (flagged in original PRD) |

No new contested-ownership seam is introduced.

### Out of scope for this addendum (deferred follow-ups)

- **Full runtime honoring of data-dependent `force_tet`/`require_hex_wedge`** (post-build realization redispatch so a non-literal option value drives element type). Deferred; the `HexPreferred`+diagnostic degrade (decision 3) covers literal options, which is all real usage. File as a follow-up if a data-dependent case surfaces.
- **P2 hex/wedge**, within-body mixed elements, Phase B/C — all remain out of scope per the original PRD.

### Open questions (tactical — decided at implementation time)

1. **Exact compiled-expr traversal for the static `ElasticOptions`-arg read** (C-4). The demand walker already matches consumer→producer via compiled `UserFunctionCall` arg cells; reading a *nested* `ElasticOptions(force_tet: …)` ctor-literal sub-field depends on its real `CompiledExpr` shape. **First step for 4746's implementer:** author a fixture and probe the real shape via `reify eval` / inspection (overlay's eval-signature discipline) *before* freezing any assumption into the RED test. If the nested literal isn't cleanly readable, fall back to `HexPreferred`+diagnostic for that call (the decision-3 path) rather than guessing the shape.
2. **Where the `SweptMesh3d → VolumeMesh` wrap attaches** — at the `execute_realization_ops` store edge vs inside the `realization_content.rs` projection. N1 provides the conversion; 4746 wires whichever edge actually stores/projects the realized content. Confirm the single storage point at impl time.
3. **Diagnostic phrasing** for the non-literal-options degrade and the Phase-A→B fall-back (the latter already flagged in the original "Open design questions").
