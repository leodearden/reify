# PRD — Implicit solids: the `sdf_*` field algebra and `implicit()` realization

**Status:** deferred · **Milestone:** v0.6 · **Authored:** 2026-10-08
**Type:** contract PRD completing `docs/initial-design/geometry-engine-design-decisions.md` §10.6 and
`docs/reify-implementation-architecture.md` §10.6 (the field-to-geometry bridge); supersedes the
`.ri`-level signal of `docs/prds/v0_3/multi-kernel-phase-3.md` §8 **task κ** (delivered at kernel level by
#3441, never reachable from `.ri`), and supersedes `docs/prds/v0_2/multi-kernel.md` "All implemented
kernels default-on" **for Fidget only** (§11). Task **#8010** is cancelled by this PRD's authoring session.
**Approach:** B + H (contract + two-way boundary tests) — touches the multi-kernel, grammar/compiler and
FEA load-bearing seams across the workspace (reify-ir, reify-expr, reify-compiler, reify-eval, reify-core,
reify-kernel-openvdb, reify-kernel-occt, reify-geometry, reify-stdlib, reify-cli, gui).
**Code anchors** verified against main `a8d99a691b` (2026-10-08). Main moves fast — cite-by-symbol;
re-locate lines at implementation time.
Decompose amendments (2026-10-08, main 586b793377): see §12; in-place corrections in §1–§11 are listed there.
**Decomposed** 2026-10-08: α #8389 … ω #8410 (22 leaves, §8), out-of-plan milestone #8411, existing #7393 rescoped; landed via docs carrier #8388.

---

## 1. Goal

A `.ri` author builds solids from signed-distance fields, measures them as fields, and realizes them
as mesh egress (STL/3MF, GUI preview, FEA volume mesh), with every unsupported path failing closed
under a coded diagnostic.

```ri
// examples/implicit/smooth_bracket.ri
structure SmoothBracket {
    param web   : Length = 6mm
    param r     : Length = 2mm
    let plate   = sdf_box(60mm, 40mm, web)
    let boss    = sdf_transform(sdf_cylinder(8mm, 30mm), transform3(orient_identity(), vec3(0mm, 0mm, 12mm)))
    let body_f  = sdf_smooth_union(plate, boss, r)                    // Field<Point3<Length>, Length>
    let body    = implicit(body_f, vec3(80mm, 60mm, 40mm), 2mm, point3(0mm, 0mm, 12mm))   // Solid
    let mass    = sdf_volume(body_f, vec3(80mm, 60mm, 40mm), 2mm, point3(0mm, 0mm, 12mm)) * 7850kg/m^3
}
```

Arguments are `(field, extent, feature, centre = origin)`: builtins bind **by position** on main
(`lookup_named_arg_value`, `geometry_ops.rs`; label binding is #6313), so the optional argument is last.
ε ships this file **without** the `mass` line; θ appends it (θ depends on ε) with a sanity assertion. The
unit literal is `7850kg/m^3` (no space: `7850 kg/m^3` is a parse error, `kg/m3` an unknown unit). The boss
cap is coplanar with the plate bottom at z = −3 mm, so the polynomial smooth-min bulges k/4 = 0.5 mm below
it: the analytic envelope is the **smooth-union** extent, z ∈ [−3.5, 27] mm.

```
$ reify build examples/implicit/smooth_bracket.ri -o /tmp/bracket.stl --verbose
  SmoothBracket#realization[0]: kernel: openvdb, repr: Voxel
  mesh updates: …
Wrote /tmp/bracket.stl (N bytes)
Triangles: N            # N ≥ 14 000 at feature 2 mm (§7.9, ≈ 1.7 M voxels); bbox spans all three axes
$ reify build examples/implicit/smooth_bracket.ri -o /tmp/bracket.step
error: E_IMPLICIT_NO_BREP: implicit solid `body` has no B-rep; STEP export is not available …
```

The transcript order is `cmd_build`'s (provenance, `mesh updates`, `Wrote … (N bytes)`, `Triangles: N`); the
CLI prints `{severity}: {message}` and never renders a `DiagnosticCode`, so every code is also the message
prefix (D-9). The STL line is reachable once η lands; until then φ refuses it with `E_IMPLICIT_EXPORT_UNROUTED`.

Two further user-observable outcomes close the consumer chain:

- `reify check` of the committed extracted printer fixture (ToolBody + ToolDock, λ) evaluates the four
  tool-dock swept-volume gates `pen_y_lintel`, `pen_y_parked`, `pen_x_parked`, `pen_x_lintel` through
  `sdf_from_solid` + `sdf_sweep` + `sdf_penetration`: the y pair reports **Satisfied** where the
  sampled-union gate does and a seeded violation flips both; the x pair is determinate. The whole-file
  `reify check prj/printer_v01/printer.ri` SIGSEGVs today (#7383, OCCT Fuse in ToolDock's mech_x section)
  and is noted, not asserted (§8 leaf λ).
- `solve_elastic_static(material, body: <implicit solid>, loads, supports, ElasticOptions(..))` at
  **fixed parameters** runs a real plain-producer solve on the marching-cubes mesh and converges with
  populated fields; a thicker web gives a lower max displacement; `reify eval` of the example prints
  non-Undef displacement/stress (§8 leaf μ, edges #6660, #7052). Geometry-in-the-loop optimisation is §9.

**Load-bearing property.** The signal must not be satisfiable by rerouting onto the existing
BRep→Mesh→Voxel path: the terminal realization's `produced_kernel` is `openvdb` and `produced_repr` is
`Voxel` with **zero** conversion stages, and the realization's input is a dense sample grid the engine
computed from the field, never a tessellated BRep.

## 2. Background

### 2.1 Why this exists

The geometry/field bidirectionality has been a design commitment since the first design sessions
(`geometry-engine-design-decisions.md` §3.4: "A field can define geometry (SDF → implicit surface →
operations that work on implicit geometry)") and was left "needs detailed specification" at §10.6. The
multi-kernel programme assumed Fidget would deliver it (`multi-kernel.md` "Fidget unblocks `field
def`-as-geometry"), and task κ's observable signal promised "`field def sphere_sdf …` → Mesh via CLI".
What landed (#2644, #3094, #3441) is a kernel adapter with no path from `.ri`.

Investigation esc-8010-4 (2026-10-08, six-seat team + skeptic) established, on main `09dc41ec1e`:

- **Fidget is unreachable by construction.** Its descriptor declares only Booleans at `Sdf` and
  `Convert{from: Sdf} → Mesh` (`crates/reify-kernel-fidget/src/register.rs`, `fidget_capability_descriptor`);
  `demanded_reprs_for_template` (`crates/reify-eval/src/engine_build.rs`) never demands `Sdf`; no Convert
  edge targets `Sdf`; the conversion executor's `v03_conversion_projection` (`dispatcher.rs`) has exactly
  three arms, none involving `Sdf`; and ops with no parents start planning at `{BRep}`
  (the per-op loop's local `available_for_op` binding in `engine_build.rs`, design_decision 6). Linking the crate changes none of this.
- **Fidget's adapter cannot mesh a real part.** `iso_mesh` pins the domain to `[-8, 8]³` with octree
  depth clamped to `[3, 7]` (`DEFAULT_MESH_HALF_EXTENT`, `MAX_MESH_DEPTH`, `kernel.rs`), and kernel
  coordinates are SI metres (`mm` scale 0.001, `units.rs`; `Value::as_f64` = `si_value`), so the finest
  cell is 0.125 m against 20 mm parts.
- **Fidget's numerics and maintenance profile** (f32 end to end; 0.5.1 removes f64 even from
  constants; dual contouring with open non-manifold/winding issues; single maintainer; breaking API every
  0.x minor; facet dependency churn already broke `fidget-shapes` for reify) make it a poor *first*
  evaluator and a plausible *later* one (§9, §11).
- **No corpus demand for lattices or implicit modelling today**, but three fit signals: the printer
  tool-dock gate's recorded ceiling ("No hull/minkowski/solid-sweep exists … a sampled union is the
  ceiling", `prj/printer_v01/printer.ri` ToolDock O5), the PETG-printed litter tray's 4,524-hole hex
  lattice (`designs/litter_tray/dogfood-findings.md`), and the deferred FDM homogenisation PRD's gyroid
  unit cell (`docs/prds/v0_5/fdm-print-rve-homogenization-r2.md`). Leo (2026-10-08) wants FEA-driven
  topology/free-form optimisation of the printer carriage and gantry blocks, and swept-volume collision
  checks in kinematics; both want an SDF representation in the engine. The optimisation half needs the
  geometry-in-the-loop PRD that `docs/prds/v0_6/engine-unified-build-dag.md` lists as future work (§9).

### 2.2 Substrate findings that shape the design (probed 2026-10-08, `target/debug/reify`)

- **S-1 Lambdas cannot read coordinates.** `fn_field(|p| p.x …)` → `member access not yet supported: .x`;
  `Point3<Length>` `.x/.y/.z` and the lambda-parameter annotation are **#6871** (pending); `Vector3` `.x`
  (so `(p − c).x`) is **#6892** (low, not a dependency of this PRD). `sqrt`/`abs`/`max` work on Length
  today; `norm` is unresolved and `distance` refused on the value-eval surface. A user cannot write a
  closed-form SDF in `.ri` on main.
- **S-2 No stdlib namespacing.** `sdf.box(..)` → `unknown qualifier`; `std.implicit.box` is out of
  scope by `stdlib-namespace.md` §9; `import std.fields as sf` does not resolve. Field ops are the
  `FIELD_OP_NAMES` builtins (`units.rs`: fn_field, from_samples, restrict, compose, sample, gradient,
  divergence, curl, laplacian) plus `.ri` stdlib fns in `crates/reify-compiler/stdlib/fields.ri`
  (`pointwise_max`/`pointwise_min`, `compose`, `threshold`). Qualified lookup is stdlib-namespace ν
  **#5505**; `import std.*` resolution is stdlib-namespace η **#5500** (its prerequisite ζ is #5499).
- **S-3 Named arguments parse as `name: expr`** (`tree-sitter-reify/grammar.js` named-argument rule),
  but **builtins bind by position** and the label is dropped (`lookup_named_arg_value`;
  `CompiledExprKind::FunctionCall` carries no argument names; **#6313** pending). `name = expr` is a
  parse error. `atan2(x: 1.0, y: 0.0)` silently returns π/2 today.
- **S-4 No domain remap on fields.** `fn_field` types its result `Field<Real, …>`; `compose(f, g)`
  requires `g.codomain == f.domain`; no `transform_field`. **S-5 No list reduction** (`min(xs)` compiles
  and evaluates to Undef; `fold`/`reduce` unresolved).
- **S-6 Stdlib `.ri` pub fns are invisible inside user `fn` bodies.** Probed: `pointwise_min`,
  `compose`, `threshold` and the non-field `standard_bolt_lengths` all evaluate to Undef with a generic
  `OpContractViolation` when called from a user fn, while the direct call works; user→user generic fns
  work. Cause area: `compile_builder/functions_phase.rs::phase_functions` compiles fn bodies against
  `&ctx.functions` before the prelude merge (`traits_phase.rs` unverified). An INV-SF-1 violation (leaf σ).
- **S-7 `reify check` and `reify eval` never load OpenVDB** (only `cmd_build` under
  `module_has_isosurface`, `cmd_check` under thickness-DFM, after `realize_for_check`), so `reify check
  examples/multi_kernel/voxel_to_mesh.ri` errors today. Loading is necessary, not sufficient:
  `realize_for_check` demands BRep, so the isosurface terminal is dispatched to OCCT (#7393, §8).
- **S-8 Dispatch of a parentless Voxel-producing op** reaches OpenVDB only through a spurious
  `BRep→Mesh→Voxel` chain with no parents to convert (works by accident when OCCT is registered);
  `plan_output_repr` takes the first declared row regardless of demand.
- **S-9 Export and queries run on the default kernel by bare handle id**: CLI `-o` export (#6308,
  pending since 2026-08-21, root cause known, fix designed in #6366's text), declarative `: Output`,
  `distance_between_placed` and `resolve_export_body_color` (all **#6366**, which needs #6308's
  `ExportBody.kernel`), `try_eval_geometry_query` / `try_eval_kinematic_query` /
  `try_eval_conformance_query` / `realize_solid_sdf_at` (no owning task until leaf γ; the existing tests
  `geometry_ops/tests.rs::try_eval_conformance_query_resolves_via_kernel_handle_id` and
  `try_eval_kinematic_query_resolves_via_kernel_handle_id` pin that `.kernel` is ignored). OpenVDB
  `query()` and `export()` are always `Err`. Boolean/Transform/Pattern ops see `{BRep}` and a bare id for
  any parent not resolved cross-realization (only Voxel-only-input ops resolve parents that way).
- **S-10 FEA body overload**: gmsh is not linked in any shipping binary (**#6660**, pending, depends on
  **#7052**; ruled plain-producer-only, selector BCs coded-refused). The only body overload is the 5-arg
  `solve_elastic_static(ConstitutiveLaw, Geometry, List<Load>, List<Support>, ElasticOptions)`. BCs on a
  realized body are placed by **AABB-face coordinate selection** (`elastic_static.rs` `x_min`/`x_max` node
  sets), which works on any mesh. The VolumeMesh edge is entered only when the realization's demand **is**
  `VolumeMesh` (`demanded_repr == ReprKind::VolumeMesh && is_terminal_realization`) and tessellates the
  terminal via the **owning** kernel (`terminal.kernel`). A constraint whose geometry-backed inputs reach
  an auto param is declined (`engine_fixpoint.rs::run_unified_pass`, `EvalUnresolved`): realization runs
  once per build.
- **S-11 OpenVDB has no dense→grid ingest**; `read_vdb_file` densifies and drops its handle;
  `densify_grid_to_sampled` defines the layout an ingest must mirror (X-outermost row-major,
  voxel-centre = index × h, f32 storage). The C++ `GRID_DENSIFY_MAX_VOXELS` (256 Mi) is the late backstop;
  the pre-allocation check uses the Rust `DENSIFY_BUDGET_VOXELS`, hoisted into reify-ir by ε (D-7).
  `SampledField.data` is `Vec<f64>`.
- **S-12 reify-expr has forward-mode dual-number AD** over `CompiledExpr` (`dual.rs`, `dual_eval.rs`,
  solver-unification ε #6672), cluster dimension ≤ 12. `sample` is a reify-stdlib stub evaluated in
  reify-expr (`lib.rs` intercept beside calculus); reify-expr depends on reify-stdlib, not the reverse.
  `calculus.rs::validate_differentiable_field` accepts only Analytical/Composed lambdas.
- **S-13 Pass order.** `post_process_kinematic_queries` runs after `named_steps` is populated and
  **before** `run_post_processes`; `run_post_processes` is an associated fn with no `self` and the
  default kernel only; derived lets run inside it (`post_process_derived_lets`) as a fixpoint; a
  `constraint` can consume a post-processed let (printer.ri's `volume()` gate; `merge_post_build_verdicts`).
  `realize_for_check` builds with `ExportFormat::Step` and exports nothing, so every `reify check` carries
  a BRep terminal demand. `UnifiedDag` is the default scheduler with a stated SYNC REQUIREMENT binding
  only `run_post_processes`'s ladder to `hydrate_value_cell_in_loop`. No post-process `UndefCause`
  channel exists: `last_undef_causes` is written only by `Engine::eval`'s `record_op_contract_failures`.
- **S-14 `realize_solid_sdf_at` degrades silently**: five `None` paths with no `Diagnostic` and no
  `UndefCause` (one `tracing::warn!`); `Engine.realization_handles` stores bare `GeometryHandleId`s.

### 2.3 The gaps

1. **No field algebra an author can use.** S-1, S-2, S-4, S-5: neither a closed-form SDF nor a
   transformed, swept or combined one is expressible.
2. **No field-to-solid realization.** No op consumes a field and produces geometry; no dispatch tier,
   classifier arm, descriptor row or kernel ingest exists for one (S-8, S-11).
3. **No egress for a non-default-kernel body.** Export and queries go to the default kernel (S-9);
   OpenVDB cannot export; `reify check`/`eval` never load OpenVDB (S-7).
4. **No field-level measurement.** Volume, clearance and penetration over a field do not exist; kernel
   queries cannot serve an implicit body (S-9).
5. **No BRep→field bridge an author can call.** `realize_solid_sdf_at` exists but is `pub(crate)`,
   reachable only from DFM measures, and silent on failure (S-14).

## 3. Sketch of approach

**The field is the source of truth; the solid is derived egress.**

- **Gap 1 — `SdfExpr`.** A Rust-side implicit expression tree in `reify-ir`, built by bare `sdf_*`
  builtins, carried as `Value::Field { source: FieldSourceKind::Sdf, lambda: Arc<Value::SdfExpr> }`
  and typed `Field<Point3<Length>, Length>`. Evaluated in f64 by a free `reify_expr::sdf_eval` beside
  `sample`. Nodes: primitives, CSG, smooth union, offset, shell, rigid transform, mirror, gyroid (α);
  `from_samples` (β), `from_solid` (ι), `sweep` (κ) — each variant lands with the leaf that constructs
  it, with its eval arm. Constructors validate their domains and refuse with `SdfInvalidArg` so I-1
  holds by construction. Cost is node count × points; no reify-expr interpretation per point. Every node
  maps one-to-one onto a Fidget `Tree` (§9).
- **Gap 2 — `implicit(field, extent, feature, centre = origin) -> Solid`.** The engine validates
  `feature > 0`, `extent > 0`, checks the voxel budget from `extent`/`feature` **before allocating**
  through a pure grid-sizing fn hoisted by ε into reify-ir beside `VoxelResolution`, samples the field
  into a dedicated f32 carrier `DenseSdfGrid` at `h = feature / MIN_FEATURE_VOXELS_ACROSS`, and lowers to
  one op `GeometryOp::ImplicitSolid { grid, options }` declared **only** as `(ImplicitSolid, Voxel)` on
  OpenVDB, executed through one new dense-ingest FFI entry. Dispatch gains a **per-op tier** keyed on an
  OUTPUT-side predicate (the op's input class is **empty**, `NO_INPUT`): such an op is always dispatched at
  `(op, Voxel)` with `available = {Voxel}`, whatever the realization demands (the shape of the
  design_decision-3 BRep fallback). Demand is **not** clamped, so a VolumeMesh demand still reaches the
  FEA edge, which tessellates the OpenVDB terminal by owner. Sinks convert by owner: GUI preview already
  does, the FEA edge already does, STL/3MF needs #6308 plus `OpenVdbKernel::export` built from
  `tessellate` + the shared writers (η).
- **Gap 3 — owner routing or rejection.** Geometry, kinematic and conformance queries, topology
  selectors and `realize_solid_sdf_at` resolve the owning kernel by threading owner identity (γ); a body
  owned by a kernel that cannot serve an author-requested query gets `E_QUERY_KERNEL_MISMATCH`, never a
  foreign-id answer; automatic walks degrade to Warning/Indeterminate. Export colour matching and
  `distance_between_placed` are #6366's (§6). A Boolean/Transform/Pattern/Modify op with an implicit
  parent is refused in v1 with `E_IMPLICIT_UNSUPPORTED_OP` in the per-op loop before dispatch (φ). STEP
  export and `STEPOutput` refuse with `E_IMPLICIT_NO_BREP` at the **sink**, never keyed on a demand
  value (φ). CLI detector `module_requires_openvdb` (ε) calls `ensure_openvdb_kernel()` **before**
  `realize_for_check` in `cmd_check` and `cmd_eval` (and before `match output_path` in `cmd_build`) for any
  Voxel-producing op or thickness-DFM; ι adds the `sdf_from_solid` arm. The residual isosurface failure
  under check (BRep demand dispatched to OCCT) is out-of-plan task #7393 (§8).
- **Gap 4 — field-level measurement.** `sdf_volume`, `sdf_clearance`, `sdf_penetration` as reify-expr
  intercept arms over fields, grid-sampled at a declared `feature`, with stated error floors, a clipping
  check on every measurement operand (`W_IMPLICIT_CLIPPED`), coded handling of empty domains, and an
  `UndefCause` on every Undef through γ's post-process channel.
- **Gap 5 — `sdf_from_solid(solid, voxel)`.** An `Engine::post_process_sdf_from_solid(&mut self, …)`
  pass (ι) placed after the geometry-handle and redispatch passes and **before** `run_post_processes`, at
  both the `build` and `build_snapshot` sites, resolving a realized body through `realize_solid_sdf_at`
  into a `FromSolid` node with coded diagnostics on every degradation and a compile-time refusal of any
  subject that is not a named realization in scope. In v1 a `from_solid` node may feed measurement
  builtins and `sdf_sweep` but **not** `implicit()`; that is a **compile-time** rejection over the field
  argument's `ValueRef` closure (D-11, ι).

Consumers (G1): (a) `examples/implicit/*.ri` → STL and GUI preview; (b) the printer tool-dock gate on
its committed extracted fixture (λ); (c) `solve_elastic_static(body: implicit)` at fixed parameters (μ;
CLI asserted by μ with #6660/#7052 as edges); (d) an imported VDB SDF → `sdf_from_samples` → `implicit` →
STL (ξ).

## 4. Resolved design decisions

- **D-1 — Field is authoritative; solid is egress (Leo, Q-G).** Measurement happens on the field in
  Rust. Kernel queries on an implicit solid are owner-routed-or-rejected, never served. Foreclosed:
  nothing structural; serving OpenVDB query arms later sits on top of γ; Fidget later lowers the same tree.
- **D-2 — `SdfExpr` is a Rust tree built by builtins, not a lambda.** Forced by S-1/S-4/S-5; it also
  makes Fidget lowering node-for-node and keeps per-point cost out of reify-expr. `fn_field` lambdas are
  accepted by `implicit()` and the measurement builtins as `FieldSourceKind::Analytical` (leaf ν, gated
  on #6871); a Field whose domain is not `Point3<Length>` (an unannotated `fn_field(|p| ..)` types as
  `Field<Real, ..>`, which `sample` silently accepts a Point3 for today) is refused with `SdfInvalidArg`.
- **D-3 — Bare `sdf_` prelude names now; re-home later (Leo, Q-H).** Leaf ρ moves them under
  `std.implicit` when stdlib-namespace ν (#5505) and η (#5500) land; until then the bare names are the
  contract.
- **D-4 — `implicit(field, extent: Vector3<Length>, feature: Length, centre: Point3<Length> = origin)`.**
  Positional binding (S-3); the optional parameter is last. Labelled arguments on **this PRD's builtin
  rows** are refused at compile time with `SdfLabelledArgUnsupported` until #6313 (one shared check in β,
  keyed on a property of the rows; each later leaf's rows join it), because same-typed asymmetric slots
  (`sdf_clearance(a, b)`, `sdf_difference`, `sdf_box(w, h, d)`) would silently misbind. B22 pins
  distinct-typed misorders only: a `Vector3` in a registered Length slot is `ArgTypeMismatch`, while a
  Length in a Vector3 position is silently accepted on main (no `ExpectedArg::Vector`). Extent and centre
  are pure values. Extent is mandatory in v1 (Leo, Q-K); inference is §10 Q2. The extent is the
  modelling domain; a field that is ≤ 0 on the extent boundary gets `W_IMPLICIT_CLIPPED`; a grid with no
  sample ≤ 0 gets `W_IMPLICIT_EMPTY`.
- **D-5 — One descriptor row, `(ImplicitSolid, Voxel)`.** Not `(…, Mesh)` too: `plan_output_repr`
  keys on the first row (S-8) and a dual row would mis-record provenance; Voxel→Mesh is already an
  executor arm (`MarchingCubes`) and an owner-tessellate at every sink.
- **D-6 — Per-op dispatch tier, no demand clamp.** `classify_op_input_reprs(ImplicitSolid)` returns an
  **empty** input set (`NO_INPUT: &[ReprKind] = &[]`, satisfying `classify_op_all_variants_are_classified`);
  the tier is keyed on a separate OUTPUT-side predicate (`op_is_voxel_source` or a descriptor field), never
  on a `&[Voxel]` input class, which would be value-identical to `Surface`'s and dispatch isosurface at
  `(Surface, Voxel)`. The per-op loop dispatches such ops at `(op, Voxel)` with `available = {Voxel}`
  regardless of `demanded_repr` (zero-conversion plan, verified against `dispatch`'s first-pop probe).
  `demanded_reprs_for_template` and `voxel_pipeline_demand_overrides` are unchanged for every other op
  (regression oracle, C-3 shape; `crates/reify-eval/tests/voxel_to_mesh_e2e.rs` stays green); the GUI
  override maps `ImplicitSolid` realizations to Voxel so the tessellate twin agrees. `available_for_op`'s
  `{BRep}` default is untouched for every existing primitive.
- **D-7 — Resolution and budget.** `feature` is the thinnest feature that must be resolved;
  `h = feature / 4` (`MIN_FEATURE_VOXELS_ACROSS`); grid dims = ⌈extent/h⌉ + 2 band voxels per side;
  voxel count checked against `DENSIFY_BUDGET_VOXELS` (256 Mi) **before allocation**. ε hoists both
  constants and a pure grid-sizing fn `(extent, feature, band) -> Result<GridDims,
  ImplicitBudgetExceeded { implied_voxels, budget }>` from `reify-kernel-openvdb/src/mesh_to_voxel_options.rs`
  into reify-ir beside `VoxelResolution`, with reify-kernel-openvdb re-exporting (SPOT); θ reuses both.
  The refusal is `ImplicitBudget` (`E_IMPLICIT_BUDGET: …`, counts in the message text; the typed error is
  what tests assert). `feature > 0` and `extent > 0` are validated **before** the budget arithmetic
  (`SdfInvalidArg`). Samples are f32 in a dedicated `DenseSdfGrid` carrier (δ; `SampledField` is f64):
  memory basis 4 B/voxel payload + the grid, ≈ 1 GiB payload at the 256 Mi cap. Marching cubes at iso 0.
  `ImplicitOptions { voxel_size, band_voxels }` — no `adaptive` field (§10 Q3).
- **D-8 — Dense ingest mirrors `densify_grid_to_sampled`** (X-outermost row-major, linear transform
  with `postTranslate` for the centre, `LEVEL_SET` class, background = +band·h, name/units set before
  registration per the Sync audit). Activation policy: **every voxel of the padded block stays ACTIVE**
  (no prune, no tolerance deactivation); `band_voxels` is both the per-side padding and the ±band·h
  clamp. Round trip (B5): per-axis bounds == centre ± (n−1)·h/2 with n = ⌈E/h⌉ + 2·band; values ==
  clamp(±band·h, s) everywhere; voxel size == h. Pruning is §10 Q9.
- **D-9 — Fail closed, coded, one site per sink.** `DiagnosticCode` is a fieldless `Copy` enum: every
  new code is attached with `.with_code(..)` **and** its mnemonic is the message prefix
  (`E_IMPLICIT_NO_BREP: …`, the `W_MODULE_DECL_MISSING` convention), because the CLI prints
  `{severity}: {message}`. Payloads (counts, query, kernel, cause) are message text; tests assert code
  identity at engine level and the prefix at CLI; numeric payloads are asserted through typed Rust
  returns, never parsed. Variants and owners: `ImplicitBudget` (ε; θ reuses), `ImplicitClipped` W (ε; θ
  reuses), `ImplicitEmpty` W (ε), `ImplicitUndefArg` (ε), `ImplicitNoBrep` (φ; sinks: Phase-B STEP export
  arm, `STEPOutput`), `ImplicitUnsupportedOp` (φ; sink: per-op loop before dispatch),
  `ImplicitExportUnrouted` (φ; sinks: CLI `-o` export arm, `STLOutput`/`ThreeMFOutput`; CLI arm removed
  by η, declarative arm and the variant removed by υ), `ImplicitFromSolidInRealization` (ι, compile-time,
  D-11), `QueryKernelMismatch` (γ), `SdfFromSolidNoKernel` (ι), `SdfFromSolidFailed` (ι; cause in message
  text, typed enum underneath), `SdfFromSolidUnresolvableSubject` (ι, compile-time),
  `SdfFromSolidPreviewUnavailable` W (ι), `SdfInvalidArg` (β; ε/θ/ν reuse), `SdfLabelledArgUnsupported`
  (β), `SdfSweepMisuse` (κ), `SdfDeprecatedBareName` W (ρ), and (pinned at decompose, §12 R37)
  `QueryKernelUnserved` W (γ: an automatic walk or DFM rule over a body its owner cannot measure),
  `ImplicitArgAwaitingSolve` W (ε), `ImplicitNoOpenVdb` (ε, B18), `ImplicitFeaOptionUnsupported` (μ).
  θ's empty and clipped measurement domains reuse `ImplicitEmpty` / `ImplicitClipped`; a selector on a
  body its owner cannot serve is `QueryKernelMismatch`. **Mnemonic rule:** `E_` (Error) or `W_` (the
  variants marked W) + the variant name in SCREAMING_SNAKE — `ImplicitNoBrep` → `E_IMPLICIT_NO_BREP`,
  `SdfFromSolidPreviewUnavailable` → `W_SDF_FROM_SOLID_PREVIEW_UNAVAILABLE`. Length-dimension
  rejections reuse the existing `DimensionedArgRejected`.
- **D-10 — Measurement semantics.** `sdf_volume(f, extent, feature, centre?)` = h³ × #{voxels with
  f ≤ 0}, surface voxels sub-sampled at h/2 (§10 Q4); `sdf_clearance(a, b, extent, feature, centre?)`
  = min over {p : a(p) ≤ 0} of b(p) (the Layer-1 margin printer.ri wanted and could not express, C6);
  `sdf_penetration(a, b, …)` = max over {p : a(p) ≤ 0} of −b(p), clamped at 0. Error floors in §7.8.
  **Domains:** every measurement operand gets the clipping check → `W_IMPLICIT_CLIPPED` naming the
  builtin; {a ≤ 0} empty within the extent → `sdf_clearance` Undef with a coded cause; `sdf_penetration`
  over an empty or clipped `a` returns Undef with `UndefCause::OpContractFailed { code: ImplicitEmpty | ImplicitClipped }` (constraint → Indeterminate), never a value — the same disposition as `sdf_clearance`; `sdf_volume` of a clipped field →
  `W_IMPLICIT_CLIPPED`. Every Undef carries an `UndefCause` recorded through γ's post-process channel
  (`UndefCause::OpContractFailed { code, span }` merged into `last_undef_causes`), which the derived-lets
  pass drains (today it evaluates with none).
- **D-11 — `from_solid` nodes are value-level only in v1.** They resolve in the post-process pass
  after realization, so they cannot be inputs to a realization. The compiler (ι) rejects an `implicit()`
  whose field argument's transitive `ValueRef` closure reaches an `sdf_from_solid` call with
  `E_IMPLICIT_FROM_SOLID_IN_REALIZATION` (INV-SF-3); `SdfExpr::is_realizable` (ι) is a defensive runtime
  check in ε's lowering. The pass is **excluded** from the `UnifiedDag` in-loop hydration mirror for the
  same reason: `from_solid` never feeds a realization, so in-loop hydration never consumes it. The
  collision gate never needs it.
- **D-12 — `sdf_sweep(field, snapshots, body: BodyId)`** takes the `Value::List` of snapshot maps that
  the kinematic 4-arg `sweep(mech, joint, range, n)` returns and a `BodyId` (the nominal type
  `body_id_of` returns); slots 2–3 are **compile-unchecked in v1** (the 4-arg `sweep` compiles as `Real`
  today; no slot vocabulary for List/BodyId) and the eval-time coded diagnostic is the only guard. κ
  decodes each snapshot's body record — the `bodies` element whose `id` equals the BodyId
  (`snapshot.rs` `transform_of` semantics) — through reify-stdlib `geometry.rs::decompose_transform`
  (visibility change) or a local decode, never reify-eval's private `accept_transform_to_arrays`; the
  node stores the decoded inverse world transforms and evaluates min over `field(T⁻¹p)`.
  `SdfSweepMisuse` fires on a non-List value, a non-snapshot element, an **empty** list (min over ∅ would
  be +∞, I-1) and a snapshot lacking the body. Sample-pitch tunnelling is the author's responsibility, as
  today; interval-sound sweeps are a Fidget-era follow-up (§9). Typing `sweep`'s result is **#6007**
  (§10 Q7; no edge).
- **D-13 — Determinism.** Grid fill is f64 and single-threaded-deterministic. B17 compares two
  **fresh** engines (a same-engine rebuild is a `RealizationCache` hit) under a scoped
  `tbb::global_control(max_allowed_parallelism, 1)` behind a test-fixtures FFI hook (`volumeToMesh`
  partitions by tbb concurrency), owner-tessellated Mesh buffers compared bitwise; envelope test: bbox
  within h of the smooth-union extent, triangle floor per §7.9. No closed-form byte-identity claim is
  made across hosts; mesh bytes stay annex (conformance scope boundary).
- **D-14 — Fidget is the future evaluator, not the first.** Trigger and measurement in §9/§11; leaf
  π records the baseline; #8411 evaluates it.
- **D-15 — FEA v1 scope (Leo, Q-I).** Fixed-parameter plain producer, AABB-face BCs only
  (`FixedSupport(target: "root")` style), selector BCs refused by #6660's coded refusal; μ asserts the
  CLI run itself (edges #6660, #7052). Every `ElasticOptions` param on the implicit-body route is either
  honoured (site named) or coded-refused when non-default; `mesh_size` changes the DOF count and
  `require_hex_wedge: true` emits its code. The VolumeMesh edge already tessellates on the owner
  (`terminal.kernel`); μ's edge work is limited. An MC adaptivity knob is added only if μ measures a need,
  named `mc_adaptivity`, read at the VolumeMesh-edge owner tessellation via
  `GeometryKernel::realize_mesh_from_voxel(handle, iso, adaptive)`, never sourced from
  `ElasticOptions.adaptive` (§10 Q3). Meshing-service γ's `GeometrySource::Surface` swap is mechanical.
- **D-16 — Evaluation site.** `SdfExpr` (the type, hashing) lives in reify-ir, as do the hoisted grid
  constants/sizing fn, `ImplicitOptions` and `DenseSdfGrid`; `is_realizable` arrives with ι; `sdf_eval`
  and the measurement intercepts live in reify-expr beside `sample`/calculus, because reify-expr depends
  on reify-stdlib and owns field evaluation (S-12) and cannot name reify-kernel-openvdb.

## 5. Pre-conditions for activating

| Pre-condition | State | Effect if unmet |
|---|---|---|
| OpenVDB present (`cfg(has_openvdb)`) | preflighted by `scripts/check-manifold-deps.sh` (presence fatal) | not a supported configuration: `module_requires_openvdb && !ensure_openvdb_kernel()` is a coded Error (I-7, B18 waiver) |
| **#6308** export by owning kernel | pending (high) | η's CLI STL signal unreachable; φ emits `E_IMPLICIT_EXPORT_UNROUTED` on ImplicitSolid-produced bodies meanwhile (independent of #6308's landing order) |
| **#6366** declarative `: Output` owner routing (+ `distance_between_placed`, `resolve_export_body_color`) | pending (low) | `STLOutput`/`ThreeMFOutput` on an implicit body stays `E_IMPLICIT_EXPORT_UNROUTED` until υ (prereqs φ, η, #6366) |
| **#6660** gmsh in shipping binaries, **#7052** cache key | pending (high / medium) | μ's engine e2e runs (dev-dep gmsh); μ's CLI assertion waits (hard edges) |
| **#6313** label binding for builtins | pending (low) | labels on this PRD's rows are refused (`SdfLabelledArgUnsupported`, β) until #6313 lifts the refusal (lead addendum to #6313's text) |
| **#6871** lambda coordinate access | pending (medium) | ν waits (#6892 Vector3 projection is not a dependency) |
| **#5505** qualified lookup, **#5500** `import std.*` resolution (its prerequisite ζ **#5499**) | pending | ρ waits |
| **#6007** typing kinematic `sweep`'s result | pending (medium) | `sdf_sweep` slots 2–3 stay compile-unchecked; `TODO(#6007)` at the signature row |
| **#6988** / #8213 `realize_solid_sdf` returns a sparse grid | pending (low) | shares `realize_solid_sdf.rs` with γ and ι; ι's `FromSolid` payload follows whichever lands first (R33) |
| **#7383** OCCT Fuse SIGSEGV in printer.ri ToolDock mech_x | pending | observed, not gated: λ's signal is the extracted fixture; x-pair parity with the old gate is recorded as observable once #7383 lands |
| meshing-service γ **#8286** | pending | μ's edge path moves from owner-tessellate to `GeometrySource::Surface`; local to the realization edge |

## 6. Cross-PRD relationship

| Other PRD / task | Direction | Seam mechanism | Owner | Status |
|---|---|---|---|---|
| `v0_3/voxel-to-mesh-surfacing.md` | consumes | `MarchingCubes` executor arm, `voxel_pipeline_demand_overrides`, `MarchingCubesOptions` | other (shipped) | wired |
| `v0_3/multi-kernel-phase-3.md` §8 κ | supersedes | the `.ri` `field def → Mesh` signal | this PRD | queued (τ) |
| `v0_2/multi-kernel.md` "default-on" decision | supersedes (Fidget only) | linkage policy | this PRD | queued (τ) |
| `v0_3/imported-field-source` (θ #3439) | resolves contested pair 2 | `SampledField → grid handle` (voxel-to-mesh D2 gap) | **this PRD** (δ, ξ) | queued |
| #6308 (CLI export by owner) | depends on (η) | `ExportBody.kernel`, `export_kernel_for_owners`; may land the STL arm of `OpenVdbKernel::export` | other | pending |
| #6366 (declarative `: Output` owner routing, `distance_between_placed` item 1, `resolve_export_body_color` item 3) | depends on (υ); owns the two walks γ does not | `ExportBody.kernel`, `Value::GeometryHandle` provenance | other | pending |
| #6313 (builtin label binding) | depends on (ordering only); lifts β's `SdfLabelledArgUnsupported` | `lookup_named_arg_value` | other | pending |
| `v0_3/structural-analysis-fea.md`, #6660, #7052 | consumes | `solve_elastic_static(body: Solid)` plain producer; AABB-face BCs | other | pending |
| `v0_6/meshing-service.md` γ #8286 | consumes | VolumeMesh edge `GeometrySource::Surface` | other | pending |
| kinematics stdlib (`sweep`, `snapshot`, `transform_of`, `body_id_of`) | consumes | snapshot `Value::List`, `Value::Transform`, `BodyId`, `decompose_transform` | other (shipped) | wired |
| #6007 (registry τ5: type `sweep`'s result) | cited, no edge | nominal `List<Snapshot>` for `sdf_sweep`'s slot 2 (§10 Q7) | other | pending |
| `v0_6/stdlib-namespace.md` ν #5505, η #5500 (ζ #5499) | depends on (ρ only) | qualified-reference resolution, `import std.*` | other | pending |
| #6988 / #8213 (`realize_solid_sdf` sparse grid) | shares a file | `realize_solid_sdf.rs` (γ, ι) | other | pending |
| #7383 (printer.ri OCCT Fuse SIGSEGV) | observed, no edge | whole-file `reify check printer.ri` | other | pending |
| `v0_6/geometry-algebra-solver-unification.md` (dual AD) | future consumer | ∂field/∂param via `dual_eval` | other | not in v1 |
| `v0_6/engine-unified-build-dag.md` (geometry-in-the-loop) | future PRD | realization inside the solve loop | other | future |
| `v0_5/fdm-print-rve-homogenization-r2.md` | future consumer | gyroid unit cell | other | deferred stub |
| #6871 / #6892 (lambda coordinates) | depends on (ν only, #6871) | lambda param projection | other | pending |
| `process-dfm-thickness-metrology` (min_wall/min_feature/thickness measures) | shares | `realize_solid_sdf_at` owner check and typed failure (γ) | this PRD (γ) | queued |
| `crates/reify-kernel-fidget` (workspace member, unlinked) | owned by | §11 trigger evaluation | **#8411** (out-of-plan milestone, §8) | filed with this batch |

**Integration-seam sub-check** (`engine-integration-norm.md` §3): `ImplicitSolid` plugs into §3.1
(op-execute, OpenVDB) and §3.3 (multi-kernel dispatch: descriptor row, classifier arm, dispatch tier);
`sdf_from_solid` and the measurement builtins plug into the post-process/derived-lets shape that §3.8
(check-time DFM walk) and the kinematic queries already use; FEA reaches the VolumeMesh edge of §3.2.
No new seam.

**G1 consumers:** (a) `examples/implicit/*.ri` STL + viewport; (b) the committed extracted
ToolBody + ToolDock fixture under `tests/prd-gate/fixtures/` and `prj/printer_v01/printer.ri`; (c)
`examples/implicit/fea_implicit_bracket.ri` (fixed parameters) + `reify-eval` e2e + the CLI run; (d)
`examples/implicit/imported_sdf_to_stl.ri`. Every `sdf_*` combinator is exercised by
`examples/implicit/sdf_algebra.ri` (β) with a closed-form sample assertion each, asserted
`Satisfaction::Satisfied` by an engine test.

## 7. Contract (B + H)

### 7.1 `SdfExpr` (reify-ir; evaluation in reify-expr)

```rust
// crates/reify-ir/src/sdf_expr.rs
pub struct Rigid3 { pub rot: [f64; 4], pub t: [f64; 3] }   // the shape Value::Transform carries; no scale
pub enum SdfExpr {
    // α lands these variants, their sdf_eval arms, content_hash and the carriage below
    Sphere { r: f64 },                                   // SI metres throughout
    Box { half: [f64; 3] },
    Cylinder { r: f64, half_h: f64 },                    // axis +Z
    Torus { major: f64, minor: f64 },
    HalfSpace { n: [f64; 3], d: f64 },                   // n·p − d ≤ 0 inside; n unit by construction (from a Plane)
    Union(Box<SdfExpr>, Box<SdfExpr>),                   // min
    Intersect(Box<SdfExpr>, Box<SdfExpr>),               // max
    Difference(Box<SdfExpr>, Box<SdfExpr>),              // max(a, −b)
    SmoothUnion { a: Box<SdfExpr>, b: Box<SdfExpr>, k: f64 },   // polynomial smooth-min (§10 Q1), k > 0
    Offset { inner: Box<SdfExpr>, d: f64 },              // f − d
    Shell { inner: Box<SdfExpr>, t: f64 },               // |f| − t/2
    Transform { inner: Box<SdfExpr>, inv: Rigid3 },      // f(inv · p)
    Mirror { inner: Box<SdfExpr>, n: [f64; 3], d: f64 }, // from plane_xy/xz/yz(offset)
    Gyroid { cell: f64, wall: f64 },                     // (cell/2π)·|sin x cos y + sin y cos z + sin z cos x|_{x=2πp/cell} − wall/2
    // each of the following arrives with the leaf that constructs it, with its eval arm
    Samples(Arc<SampledField>),                          // β; trilinear; saturates to ±band outside bounds
    FromSolid { provenance: ContentHash, grid: /* what realize_solid_sdf_at returns, R33 */ },  // ι; built only by the post-process (D-11)
    Sweep { inner: Box<SdfExpr>, inv_poses: Vec<Rigid3> },          // κ; min over poses of inner(inv · p); never empty
}
impl SdfExpr {
    pub fn content_hash(&self) -> ContentHash;           // α; structural; Samples hashes the SampledField's content_hash; FromSolid hashes provenance
    pub fn is_realizable(&self) -> bool;                 // ι; false iff a FromSolid node is present
}
// crates/reify-expr/src/sdf_eval.rs
pub fn sdf_eval(e: &SdfExpr, p: [f64; 3]) -> f64;        // total, finite for finite p, deterministic
```

Invariants: **I-1** `sdf_eval` is total and deterministic (f64, no threads, no clocks); it holds by
construction because every constructor refuses a value outside its domain (D-9 `SdfInvalidArg`: k > 0,
cell > 0, wall ≥ 0, radii/half-extents > 0, every input finite; Sweep never empty). **I-2** every
node's value is a signed *bound* (negative inside), an exact distance only for primitives, rigid
transforms and mirrors; `Offset`/`Shell` after CSG are approximate and documented as such. **I-3**
`content_hash` is O(tree) and covers sample data through `SampledField::content_hash` (a VDB edited in
place changes the key); `FromSolid` hashes (source realization content hash, voxel size). There is no
Lipschitz accessor: the constant L used by §7.8 is test arithmetic stated per node there.

### 7.2 Value carriage

```rust
FieldSourceKind::Sdf                         // new variant (reify-ir value.rs); every exhaustive match on FieldSourceKind gains an arm (compiler-enumerated)
Value::SdfExpr(Arc<SdfExpr>)                 // new Value variant; next free content_hash tag; discriminant-rank list; uniqueness test; Eq by structural hash
Value::Field { domain_type: Point3<Length>, codomain_type: Length, source: Sdf, lambda: Arc<Value::SdfExpr(..)> }
```

`sample(f, p)` gains an `Sdf` arm in reify-expr; `pointwise_min/max` keep working on it through
`sample`. `gradient(f)` on an `Sdf` field is **out of scope** (§9): on main
`calculus.rs::validate_differentiable_field` accepts only Analytical/Composed lambdas, so α's test pins
that `gradient(sdf_sphere(..))` takes the existing refusal path, not a silent Undef (if it is silent, α
makes it coded).

### 7.3 Builtins (reify-compiler signatures; evaluation per D-16)

α chooses the registration route for the whole PRD — reify-builtins rows with eval bound by
`BuiltinId` (preferred; the I-REG-1 seed gate forbids `"name" =>` string dispatch for registered names)
or the legacy `units.rs` name slice + result-type fn + `is_known_builtin` — and every later leaf follows
it. Length slots go in `builtin_signatures.rs::builtin_arg_slots` with `NON_SELECTOR_ARG_SLOT_KEYS` /
`LOWERING_ACCEPTED_ARITIES` rows. Every leaf documents the names it registers in the relevant chunk in
the same diff (R27).

| Builtin | Signature | Node / site | Owner | Consumer |
|---|---|---|---|---|
| `sdf_sphere(r)` | `(Length) -> F` | Sphere | α | B1, B2, sdf_algebra.ri |
| `sdf_box(w, h, d)` | `(Length, Length, Length) -> F` | Box | β | Goal |
| `sdf_cylinder(r, h)` | `(Length, Length) -> F` | Cylinder | β | Goal |
| `sdf_torus(major, minor)` | `(Length, Length) -> F` | Torus | β | sdf_algebra.ri |
| `sdf_half_space(plane)` | `(Plane) -> F` (unit normal by construction, as `sdf_mirror`) | HalfSpace | β | sdf_algebra.ri |
| `sdf_union / sdf_intersect / sdf_difference(a, b)` | `(F, F) -> F` | CSG | β | sdf_algebra.ri, λ |
| `sdf_smooth_union(a, b, k)` | `(F, F, Length) -> F`, k > 0 | SmoothUnion | β | Goal |
| `sdf_offset(f, d)`, `sdf_shell(f, t)` | `(F, Length) -> F` | Offset, Shell | β | sdf_algebra.ri, λ (tool clearance offset) |
| `sdf_transform(f, t)` | `(F, Transform3) -> F` | Transform (stores inverse) | β | Goal, κ |
| `sdf_mirror(f, plane)` | `(F, Plane) -> F` | Mirror | β | sdf_algebra.ri |
| `sdf_gyroid(cell, wall)` | `(Length, Length) -> F`, cell > 0, wall ≥ 0 | Gyroid | β | sdf_algebra.ri (sample at origin = −wall/2) |
| `sdf_from_samples(s)` | `(Field<D, Length>) -> F`, D the field-def domain form (bare `Point3`, resolved today as `StructureRef` with a warning, or `Point3<Length>`); eval-time check is dimension-only and cannot see a mis-scaled length unit | Samples | β (only) | ξ |
| `sdf_from_solid(s, voxel)` | `(Solid, Length) -> F` — post-process (D-11); args[0] must be a `ValueRef` to a named realization in scope | FromSolid | ι | λ, B13 |
| `sdf_sweep(f, snaps, body)` | `(F, List, BodyId) -> F`; slots 2–3 compile-unchecked in v1 — `TODO(#6007)` | Sweep | κ | λ, κ |
| `implicit(f, extent, feature, centre?)` | `(F, Vector3<Length>, Length, Point3<Length>) -> Solid`; `length_arg(2, "feature")`, arities `Exactly(&[3, 4])` | op | ε | Goal, ζ, η, μ, ξ |
| `sdf_volume(f, extent, feature, centre?)` | `-> Volume` | reify-expr intercept | θ | Goal (mass line), B11 |
| `sdf_clearance(a, b, extent, feature, centre?)` | `-> Length` | intercept | θ | λ Layer-1 margins |
| `sdf_penetration(a, b, extent, feature, centre?)` | `-> Length` (≥ 0) | intercept | θ | λ gate, B12 |

`F` = `Field<Point3<Length>, Length>`. Length arguments are typed by the compiler and dimension-checked
at eval time. `implicit` joins `GEOMETRY_FUNCTION_NAMES`; its `feature` is gated by
`geometry_ops.rs::required_length_value`; its Field argument gets an `UNSWEPT_BUILTINS`/`RESIDUAL_SPANS`
row in `crates/reify-eval/tests/harness_geometry/units_length_closure_guard.rs` citing #7714 (the C4
tripwire classifies `&Value`s at OCCT/Fidget execute arms and has no OpenVDB call site; the guard's
`op_args` is the exhaustive match `implicit()` meets).

### 7.4 The `ImplicitSolid` op (reify-ir, reify-compiler, reify-eval)

```rust
Operation::ImplicitSolid                                   // classifier key; input class NO_INPUT (empty)
DenseSdfGrid { dims: [u32; 3], centre: [f64; 3], h: f64, data: Vec<f32> }   // δ; dedicated f32 carrier (SampledField is f64)
ImplicitOptions { voxel_size: f64, band_voxels: u32 }      // δ; content_hash with a non-zero domain tag (options-hash rule)
GeometryOp::ImplicitSolid { grid: Arc<DenseSdfGrid>, options: ImplicitOptions }   // ε; manual Debug elides data
// the compiled op carries field/extent/feature/centre in its `args`, so deps.rs::extract_realization_dependencies keys the RealizationCache on them

// reify-ir / reify-eval — one diff (strum completeness test):
one GEOMETRY_OP_DESCRIPTORS row (operation: Some(Operation::ImplicitSolid), parent_role: ParentRole::None)
  + compiled_geometry_op_to_operation arm + classify_op_input_reprs arm (NO_INPUT)
plan_output_repr(ImplicitSolid)              -> Voxel
per-op dispatch tier: op_is_voxel_source(op)  =>  dispatch(op, {Voxel}, Voxel)   // D-6; demand untouched
voxel_pipeline_demand_overrides: ImplicitSolid realization -> Voxel        // GUI tessellate twin
exhaustive-match arms: reify-kernel-occt/src/lib.rs (OcctKernel::execute), reify-eval/src/deps.rs,
  reify-compiler/src/compile_builder/entities_phase.rs

// reify-kernel-openvdb (δ) — SingleKernelHolder (reify-geometry/src/lib.rs) gains the delegating method (parity test)
GeometryKernel::ingest_dense_sdf(&mut self, grid: &DenseSdfGrid, opts: &ImplicitOptions) -> Result<GeometryHandle, GeometryError>   // trait default Err
OpenVdbKernel::execute(GeometryOp::ImplicitSolid{..}) -> ingest_dense_sdf                                                            // ε
OpenVdbKernel::export(handle, format, opts)  -> Stl | ThreeMF: tessellate(handle) + write_stl_binary / write_3mf; Obj, Step -> Err   // η
openvdb_capability_descriptor += (ImplicitSolid, Voxel)
```

Invariants: **I-4** an `ImplicitSolid` realization is always *produced* at `Voxel` with zero conversion
stages; under VolumeMesh demand the FEA edge adds a gmsh terminal after owner-tessellation; every other
graph's demand and plan is unchanged (regression oracle in the C-3 shape of voxel-to-mesh-surfacing).
**I-5** the budget is checked from `extent`/`feature` before any allocation; the FFI never allocates
beyond it. **I-6** `ingest_dense_sdf` registers a fully populated grid (name, units, transform,
background, class) before insertion — no post-registration mutation (Sync audit). **I-7** under
`cfg(not(has_openvdb))` the kernel is **not registered** (`register.rs` omits the inventory submit), so
the coded Error is sited at `module_requires_openvdb && !ensure_openvdb_kernel()` in the CLI and at the
OCCT defensive arm (B18, waived G7 `error-severity-exits-nonzero`: that configuration is unsupported).

### 7.5 Egress by owner

- GUI: `voxel_pipeline_demand_overrides` maps `ImplicitSolid` realizations to Voxel; the tessellate
  walk's owner resolution (`walk_placed_realizations`) calls `OpenVdbKernel::tessellate` (marching cubes).
- CLI `-o`: #6308's `export_kernel_for_owners` → `OpenVdbKernel::export` (η). Interim refusal (φ):
  `E_IMPLICIT_EXPORT_UNROUTED` is keyed on **ImplicitSolid-produced** product bodies only (producer-op
  identity — never on "owned by a non-default kernel", which fires on today's isosurface exports and reds
  `crates/reify-cli/tests/harness_cli/cli_build_voxel_to_mesh.rs`), defined as "an ImplicitSolid product
  body whose export would not reach an owner that can export the format": true before #6308 (bare id to
  the default kernel) and after it (`OpenVdbKernel::export` is `Err` until η), so independent of #6308's
  landing order. It fires at the export sink after the post-processes; Phase-B returns `None` for the
  whole export ahead of both the single-body and the `make_compound` arms (no file — today an Error does
  not stop the write). η removes the CLI arm by identity.
- Declarative `STLOutput`/`ThreeMFOutput` (φ): keyed on `Value::GeometryHandle.realization_ref` → an
  `ImplicitSolid` realization (precedent: gui `engine.rs::collect_display_routing`); υ (prereqs φ, η,
  #6366) removes the arm and deletes the variant.
- FEA: VolumeMesh demand reaches the edge unchanged; `kernels.get(terminal.kernel).tessellate` (owner)
  → gmsh plain producer; under meshing-service γ, the same mesh enters as `GeometrySource::Surface`.
- STEP (φ): `E_IMPLICIT_NO_BREP` at the Phase-B STEP export arm (only when `emit_geometry_output` is
  set; `None` for the whole export, no file) and at the `STEPOutput` declarative site — never keyed on a
  demand value, because `realize_for_check` demands BRep on every check.
- CLI kernel loading (ε): `module_requires_openvdb` (any Voxel-producing op, thickness-DFM; ι adds
  `sdf_from_solid`) replaces `module_has_isosurface` and the thickness-only gate, and
  `ensure_openvdb_kernel()` is called **before** `realize_for_check` in `cmd_check` and `cmd_eval` and
  before `match output_path` in `cmd_build` (the thickness arm's post-`realize_for_check` position would
  let the zero-conversion arm fall back to `default_kernel_name` and run `ImplicitSolid` on OCCT). The
  residual isosurface-under-check failure (Surface terminal demanded at BRep, dispatched to OCCT) is #7393.

### 7.6 Owner-routed queries (γ)

Sites: `try_eval_geometry_query` (four callers: `post_process_geometry_queries`,
`hydrate_value_cell_in_loop` under the UnifiedDag SYNC REQUIREMENT, `post_process_cross_sub_value_cells`,
the tessellate path), `try_eval_kinematic_query` / `post_process_kinematic_queries`,
`try_eval_conformance_query` / `Engine::post_process_conformance_queries` (is_watertight / is_manifold /
is_orientable; today default-kernel by bare id at `build_snapshot`, `build_with_geometry_output` and
`tessellate_from_values`), topology-selector evaluation on a body whose owner cannot serve it, and
`realize_solid_sdf_at` (and through it the DFM min_wall/min_feature/thickness measures).
`distance_between_placed` and `resolve_export_body_color` are **#6366**'s (items 1 and 3; they need
#6308's `ExportBody.kernel`), not γ's.

The owner check lives **inside** `try_eval_geometry_query` / `try_eval_kinematic_query` /
`try_eval_conformance_query` by threading owner identity (kernel-map keys + `default_kernel_name`;
`GeometryKernel` has no identity accessor). γ flips the pinned tests
`geometry_ops/tests.rs::try_eval_conformance_query_resolves_via_kernel_handle_id` and
`try_eval_kinematic_query_resolves_via_kernel_handle_id`, widens `Engine.realization_handles` to keep
the owner (`KernelHandle` or a sibling owner map; `post_process_geometry_handle_cells` writes `kh.id`
today), and changes `realize_solid_sdf_at` to return a typed failure enum (`OwnerMismatch`, `NoOpenVdb`,
`NoDefaultKernel`, `Unresolvable`, `RequestRejected`, `ChainFailed`) that ι maps to codes and the DFM
callers keep mapping to Indeterminate.

Per-site dispositions (G7 `error-severity-exits-nonzero`): an **author-requested** query on a body whose
owner lacks it → `E_QUERY_KERNEL_MISMATCH` (query and kernel in the message) Error + `Value::Undef`
with cause (INV-SF-1); a **DFM rule** over a body its owner cannot measure → Indeterminate with a
Warning-coded attributable reason (INV-SF-4), never Error; **kinematic walks** emit only when a query is
actually requested on that body. A foreign-id answer is never produced.

γ delivers **one** post-process `UndefCause` channel: post-process passes record
`UndefCause::OpContractFailed { code, span }` into a map merged into `last_undef_causes` after the
post-processes; ι and θ reuse it (today `cmd_eval` prints the stale pre-post-process cause).

This also fixes a latent silent-wrong for `isosurface` bodies today (B14: `volume(isosurface(..))` is
silently exactly 0 m³ under `reify build -o`). Boolean/Transform/Pattern/Modify ops with an
`ImplicitSolid` parent are φ's (`E_IMPLICIT_UNSUPPORTED_OP`, B20).

### 7.7 `sdf_from_solid` and `sdf_sweep`

`Engine::post_process_sdf_from_solid(&mut self, template, named_steps, values, diagnostics)` (ι) runs
after `post_process_geometry_handle_cells` and the redispatch passes and before the default-kernel
borrow and `run_post_processes`, at both the `build` and `build_snapshot` sites. It is **excluded** from
the `UnifiedDag` hydration mirror with the D-11 reason (`hydrate_value_cell_in_loop` is a static fn with
one kernel and the SYNC REQUIREMENT binds only `run_post_processes`'s ladder); B13 runs under the default
`UnifiedDag` scheduler to cover it. The pass is keyed on `CompiledExpr` shape (args[0] a `ValueRef` to a
named realization in scope, args[1] a Length); a call whose args[0] is anything else (an inline geometry
expression, `sub.body` member access, a fn parameter, a call inside a user fn body, a list element) is
refused at **compile time** with `SdfFromSolidUnresolvableSubject`, never left as a silent pure-path
Undef. Then: owner check (γ's enum) → `realize_solid_sdf_at(subject, VoxelResolution::TargetVoxelSize(h))`
→ `FromSolid` node with provenance = (realization content hash, h). If #6988 has landed, `FromSolid`
stores what `realize_solid_sdf_at` returns (`SparseVoxelGrid` / an `SdfSource`-like enum); otherwise ι
densifies explicitly through `densify_grid_to_sampled` with the budget stated. Every failure path gains a
code and an `UndefCause` through γ's channel: no OpenVDB kernel → `SdfFromSolidNoKernel`; everything
else → `SdfFromSolidFailed` with the cause in the message text (budget, tessellation, ingest, densify,
`RequestTooCoarse` — 4·h above the thinnest bbox extent — `FlatBody`, `NoDefaultKernel`, unresolvable)
over a typed enum underneath, one unit test per cause in the `realize_solid_sdf.rs` harness. On the GUI
preview path (`tessellate_from_values`, which lacks only `self.realization_handles`) degradation is
`SdfFromSolidPreviewUnavailable` **W** + `UndefCause`; a static helper over (`geometry_kernels`,
`default_kernel_name`, `named_steps`) may remove it (§10 Q8). ι also adds the `sdf_from_solid` arm of
ε's `module_requires_openvdb`, the B19 compile-time closure check with `ImplicitFromSolidInRealization`,
the defensive `is_realizable` check in ε's lowering, adds `sdf_from_solid` to
`geometry_ops::is_geometry_consumer_call` (so a kernel-free evaluation reports `EvalUnresolved` loudly, as
`volume()` does), and fixes the false `realize_solid_sdf.rs` doc
sentence (reify-eval "cannot name reify-kernel-openvdb" — it is a normal dependency; keep the real
chord-tolerance "Known limitation").

**Frames.** A `FromSolid` field is in the subject realization's authored (unplaced) frame; `sdf_sweep`
applies each snapshot's `world_transform` inverse, so posing a field into another body's frame is the
author's job via `sdf_transform` — the existing gate's `apply_transform(carrier_y, transform_of(s, id_car))`
shape in `prj/printer_v01/printer.ri`.

`sdf_sweep` (κ) decodes each snapshot's body record (the `bodies` element whose `id` equals the BodyId)
through reify-stdlib `geometry.rs::decompose_transform` (visibility change; reify-stdlib joins κ's
modules) or a local decode, and stores inverses; `SdfSweepMisuse` per D-12.

### 7.8 Measurement floors (G6)

- `sdf_volume`: the voxel-count error is O(A·h): at most √3·A/h² straddling cells, each misclassified
  by at most h³/2, so |ΔV| ≤ (√3/2)·A·h ≈ 0.87·A·h. Tests assert the sound bound `|V − V_analytic| ≤
  A·h` (628.3 mm³ for the sphere r = 10 mm, h = 0.5 mm; floor (√3/2)·A·h ≈ 544 mm³) **and** a bias
  ceiling B = A·h/3 ≈ 209 mm³, which lies below the inward half-voxel bias cost 4π/3·(1000 − 9.75³) =
  306.4 mm³ (an A·h/2 = 314 mm³ bound could never catch it); the e2e records the measured error and
  requires 3×measured ≤ B. If 3×measured ≥ 306 mm³ the measurement itself shows a bias.
- `sdf_clearance`, `sdf_penetration`: the builtins optimise only over samples with a(p) ≤ 0 and the true
  optimum lies on ∂a, so the error is one-sided (feasibility-restricted derivation): up to ≈ L·h along
  the normal plus O(h²/R) tangential terms, generically √1.5·L·h. Tests assert `≤ 1.5·L·h` with L = 1 for
  the sphere pair (L is stated per node as test arithmetic; no production accessor).
- `implicit` bbox: marching-cubes vertices within h of the smooth-union analytic extent on every axis
  (z_min = −3.5 mm for the Goal bracket); tests assert `≤ 2h`.

### 7.9 Triangle floor (G6)

Basis: measured `voxel_to_mesh.ri` (20 mm box, `VOXELS_PER_LONGEST_AXIS` = 64 → h = 0.3125 mm,
surface 2400 mm² → 24.6 k surface cells) produced 50,700 triangles with #6308's fix: ≈ 2.06 triangles
per surface cell on axis-aligned faces; curved surfaces only add margin. Floor rule: triangles
`N ≥ 0.5 × A / h²`; vertices `≥ 0.25 × A / h²`. For `smooth_bracket.ri` (A ≈ 7,200 mm²: plate 6,000 +
boss 1,910 − 2·201 − 302) at feature 2 mm (h = 0.5 mm; padded grid 164·124·84 ≈ 1.7 M voxels) the floor
is ≈ 14 k triangles against ≈ 58 k expected; e2e tests assert that floor and a bbox spanning three axes
(never `n > 0`). ξ's 20 mm cube fixture (A = 2400 mm², h = 0.5 mm) has floor 4,800 against ≈ 19.8 k.

### 7.10 Boundary-test sketch

| # | Scenario | Preconditions | Postconditions (asserts) | Side |
|---|---|---|---|---|
| B1 | `SdfExpr` primitives are exact distances | unit tests over Sphere/Box/Cylinder/Torus/HalfSpace | `sdf_eval` equals the closed form to 1e-12 at 50 random points (closed-form identity) | producer (α) |
| B2 | `let z = sample(sdf_sphere(5mm), point3(0mm,0mm,0mm))` in a structure let | `reify eval` | prints the line `<Struct>.z = -0.005 m` exactly | consumer (α) |
| B3 | `sdf_transform` inverse; `sdf_mirror`; `sdf_gyroid` origin; fn-wrapped combinator | sdf_algebra.ri + engine test asserting every constraint `Satisfaction::Satisfied` | `abs(eval(p + t) − untranslated eval(p)) <= 1e-12m`; mirrored sample within 1e-12m of its image (exact `==` only for the zero-offset mirror); gyroid at origin `==` −wall/2 (bit-exact); `fn rounded(f, d) = sdf_offset(f, d)` equals the direct call | producer (β) |
| B4 | Budget refusal | `feature` giving > 256 Mi voxels | `ImplicitBudget` (`E_IMPLICIT_BUDGET:` prefix, counts in message text); the typed `ImplicitBudgetExceeded { implied_voxels, budget }` asserted in a unit test; sited in ε's `ImplicitSolid` lowering before any allocation or dispatch, so `Engine::last_dispatch_count_by_realization` reads 0 for that realization; `reify check` exit ≠ 0 with the prefix on stderr (the exit code alone matches any compile error) | both (ε) |
| B5 | Dense ingest round trip | `DenseSdfGrid` s, extent E, band | per-axis bounds == centre ± (n−1)·h/2 with n = ⌈E/h⌉ + 2·band; `densify(ingest(s))` == `clamp(±band·h, s)` everywhere (every voxel active); voxel size == h; `SingleKernelHolder` delegates | producer (δ) |
| B6 | Demand unchanged | template with `implicit(..)` and STL sink | `demand[impl]` whatever the sink says; every other realization's demand identical to the pre-change oracle | producer (ε) |
| B7 | Zero-conversion plan | registry = {occt, openvdb}; `reify build examples/implicit/smooth_bracket.ri --verbose` (no `-o`) | CLI prints `<realization>: kernel: openvdb, repr: Voxel`, exit 0; zero conversions asserted by a reify-eval crate-internal test of the tier + `dispatcher::dispatch` (no CLI accessor exposes `plan.conversions`) | both (ε) |
| B8 | STEP refusal | `reify build … -o x.step` on an implicit body | `error: E_IMPLICIT_NO_BREP: …`, exit ≠ 0, no file (Phase-B returns `None` ahead of both export arms); `reify check` of the same file: exit 0, no Error-severity diagnostic, no `ImplicitNoBrep`, openvdb/Voxel provenance | consumer (φ) |
| B9 | GUI preview | `EngineSession` on `smooth_bracket.ri` | `mesh_stats_json` `face_count ≥ floor`; `bounding_box` spans 3 axes | consumer (ζ) |
| B10 | CLI STL | #6308 landed; `write_3mf` reached from reify-kernel-openvdb | `Triangles: N ≥ floor`; bbox spans 3 axes; 3MF written | consumer (η) |
| B11 | `sdf_volume` floor | sphere r = 10 mm, h = 0.5 mm | sound bound ≤ A·h and bias ceiling 3×measured ≤ A·h/3 (§7.8); a case whose measurement argument is post-process-derived (feature from `volume(box(..))` tripping the budget) shows `E_IMPLICIT_BUDGET:` in `reify check` output (derived-lets sink observed); `smooth_bracket.ri` mass line sanity | consumer (θ) |
| B12 | `sdf_penetration` / `sdf_clearance` | two spheres overlapping by δ; separated by δ | within `1.5·L·h` of δ (L = 1); non-overlapping → penetration 0 | consumer (θ) |
| B13 | `sdf_from_solid` | named `let b = box(20mm, 20mm, 20mm)`, voxel 1 mm | positive half via `reify check` (OpenVDB loaded by ι's detector arm): `sample` at origin within `h` of −10 mm (printed in SI: `-0.010 m` ± 0.001 m), under the default UnifiedDag scheduler; OpenVDB-absent half at engine level (registry without OpenVDB) → Undef with cause + `SdfFromSolidNoKernel`; one unit test per `SdfFromSolidFailed` cause (typed enum) | both (ι) |
| B14 | Owner routing | named-let fixture: `let solid = box(size, size, size)`, `let shell = isosurface(solid)`, `let v = volume(shell)`, `constraint v > 0mm^3`; `reify build <fixture> -o x.stl` (loads OpenVDB under the isosurface detector today) + engine e2e | `QueryKernelMismatch` code; constraint INDETERMINATE (not VIOLATED); cell Undef with cause; never the exit code or the STL. Today: VIOLATED with volume silently 0 m³ | both (γ) |
| B15 | Printer gate parity | committed extracted ToolBody + ToolDock fixture under `tests/prd-gate/fixtures/`; gates `pen_y_lintel`, `pen_y_parked`, `pen_x_parked`, `pen_x_lintel` | y pair: new gate Satisfied where the old one is (both Satisfied today), seeded violation (pitch = 55 mm flips `pen_y_parked`) flips the new gate too; x pair: determinate (not INDETERMINATE) in a fixture without the crashing union, old-gate parity recorded as observable once #7383 lands; semantics: Satisfied where the old gate is Satisfied with clearance > τ + method error, Violated for overlap depth > τ, sub-τ blind spot recorded; τ ≥ 2h, runtime and per-body memory recorded; Layer-1 `clr_*`/`margins_*` not covered | consumer (λ) |
| B16 | FEA on implicit | `fea_implicit_bracket.ri`, fixed params, gmsh dev-dep; two web thicknesses | converges; `displacement`/`stress` non-Undef; thicker web ⇒ lower max displacement (monotone differential); `reify eval` of the example prints non-Undef displacement/stress (CLI asserted; edges #6660, #7052); registered in the heavy-test filter in the same diff | consumer (μ) |
| B17 | Run-twice | two FRESH engines, scoped `tbb::global_control(max_allowed_parallelism, 1)` via the test-fixtures FFI hook δ delivers | owner-tessellated Mesh buffers bitwise identical | producer (ε) |
| B18 | Stub degradation | `cfg(not(has_openvdb))`: kernel not registered | one coded Error at `module_requires_openvdb && !ensure_openvdb_kernel()` (CLI) / the OCCT defensive arm, no panic, `reify check` exit ≠ 0. G7 waiver `error-severity-exits-nonzero`: unsupported configuration (`check-manifold-deps.sh` makes OpenVDB presence fatal), so a design containing `implicit()` is unrealizable there, not healthy | both (ε) |
| B19 | `from_solid` inside `implicit` | `implicit(sdf_union(sdf_from_solid(a, 1mm), sdf_sphere(1mm)), …)`, `a` a named realization | compile-time `E_IMPLICIT_FROM_SOLID_IN_REALIZATION` (`ImplicitFromSolidInRealization`); on main the unresolved name compiles with a warning (rc=0), so the assertion is on the variant and site, asserted as the `E_IMPLICIT_FROM_SOLID_IN_REALIZATION:` prefix on stderr, never the exit code | both (ι) |
| B20 | Op on implicit parent | `union(implicit(..), box(..))`, `fillet(implicit(..), …)` with the implicit call inlined (intra-realization step via `realization_step_ids`) and via a named sibling (cross-realization) | `E_IMPLICIT_UNSUPPORTED_OP` in the per-op loop before dispatch, keyed on producer-op identity, never an OCCT handle; an isosurface Voxel parent is NOT refused | both (φ) |
| B21 | Interim export refusal | `-o x.stl` on an ImplicitSolid-produced body (before or after #6308, before η); `STLOutput` before υ | `E_IMPLICIT_EXPORT_UNROUTED` prefix on stderr, no file (Phase-B `None`); both arms observed under `reify build` (declarative refusals fire in `build_outputs`; `reify check` of the same file exits 0, as B8); `voxel_to_mesh.ri -o x.stl` still exits 0 (`cli_build_voxel_to_mesh.rs` green) | consumer (φ) |
| B22 | Mis-ordered call | `implicit(f, 2mm, vec3(..))` | `ArgTypeMismatch` on the FEATURE slot: `implicit: feature argument expects Length, got Vector3<…>` (the existing form: `box(10mm, vec3(..), 10mm)` prints `box: height argument expects Length, got Vector3<Scalar[m]>`, exit 1) (distinct-typed misorder only; a Length in a Vector3 position is accepted on main) | consumer (ε) |
| B23 | Clipped field | extent smaller than the body | `W_IMPLICIT_CLIPPED` naming `implicit`, computed from the sampled grid in ε's lowering | consumer (ε) |
| B24 | Constructor domains | `sdf_smooth_union(a, b, 0mm)`, `sdf_gyroid(0mm, 1mm)`, `sdf_box(-1mm, …)`, `sdf_sphere(0mm)`, a non-finite input | one `SdfInvalidArg` (`E_SDF_INVALID_ARG:` prefix) per class + `UndefCause::OpContractFailed{code}`; no NaN/∞ ever reaches `sdf_eval` | producer (β) |
| B25 | `implicit`/measurement domains | `feature = 0mm`, `extent = vec3(0mm, …)` | `SdfInvalidArg` before the budget arithmetic; no `ImplicitBudget` | both (ε, θ) |
| B26 | Labelled argument | `sdf_clearance(b: x, a: y, …)` | compile-time `SdfLabelledArgUnsupported`, never a misbind (lifted by #6313) | consumer (β) |
| B27 | Empty solid | field > 0 everywhere on the extent (misplaced centre) | `W_IMPLICIT_EMPTY` (`ImplicitEmpty`) from ε's lowering; never a silent 0-triangle body | consumer (ε) |
| B28 | Undef argument to `implicit` | (a) an argument whose Undef inherits `AwaitingSolve`; (b) any other root Undef argument | (a) Warning + Indeterminate constraints, no Error; (b) `ImplicitUndefArg` Error, which the code-less `CHECK_ERROR_EXIT_ALLOWLIST` entry cannot excuse, `reify check` exit ≠ 0 | both (ε) |
| B29 | Clipped / empty measurement operands | sphere `a` outside the extent; clipped sphere volume; `sdf_penetration` over empty `a` | `sdf_clearance` → Undef with coded cause, not 0; `sdf_volume` → `W_IMPLICIT_CLIPPED`; `sdf_penetration` → Undef with `ImplicitEmpty`/`ImplicitClipped` cause (constraint Indeterminate), never a value | consumer (θ) |
| B30 | `sdf_sweep` misuse | empty list; list lacking the body; a non-List value; a non-snapshot element | `SdfSweepMisuse` (`E_SDF_SWEEP_MISUSE:`), Undef with cause; never +∞ or a vacuous Satisfied | both (κ) |
| B31 | Unresolvable `sdf_from_solid` subject | inline `sdf_from_solid(box(20mm, 20mm, 20mm), 1mm)` | compile-time `SdfFromSolidUnresolvableSubject` (`E_SDF_FROM_SOLID_UNRESOLVABLE_SUBJECT:` prefix on stderr, never the exit code alone) | both (ι) |
| B32 | Preview path | GUI `tessellate_from_values` on a design using `sdf_from_solid` | `SdfFromSolidPreviewUnavailable` **W** + `UndefCause`; no Error on a healthy design | consumer (ι) |
| B33 | `ElasticOptions` on the implicit route | `mesh_size` varied; `require_hex_wedge: true` | DOF count changes with `mesh_size` (honoured); `require_hex_wedge` emits its coded refusal | consumer (μ) |
| B34 | Declarative export routed (B21's flipped twin) | `STLOutput`/`ThreeMFOutput` on an implicit body after #6366 + η | file written, no `ImplicitExportUnrouted`; the variant no longer exists | consumer (υ) |
| B35 | STL path carries no owner-routing error | η's `reify build smooth_bracket.ri -o x.stl` | no `QueryKernelMismatch` diagnostic on the STL path (automatic walks never Error on a healthy implicit design) | consumer (η) |

## 8. Decomposition plan

B+H: foundation intermediates roped to integration-gate leaves (C-as-integration). Greek labels; task
IDs backfilled at decompose (2026-10-08). Every leaf names its signal and its B-rows; every intermediate names what
it unlocks. Every language-surface leaf documents the names it registers in `fields.md`/`geometry.md` in
the same diff (R27). A leaf whose Rust test reads a `tests/prd-gate/fixtures/*.ri` registers its basename in
`scripts/verify.sh` `_RUST_COUPLED_RI_FIXTURES` in the same diff (`tests/infra/test_verify_scope.sh`
PG-DRIFT reds otherwise; R39).

### Phase 1 — Foundations

- **α #8389 — `SdfExpr` + `sdf_eval` + `Sdf` field carriage + `sample` arm + `sdf_sphere` end-to-end +
  the registration route.** Lands `Rigid3`, `SdfExpr` with Sphere, Box, Cylinder, Torus, HalfSpace,
  Union, Intersect, Difference, SmoothUnion, Offset, Shell, Transform, Mirror, Gyroid and their
  `sdf_eval` arms, `content_hash`, `FieldSourceKind::Sdf`, `Value::SdfExpr` (next free content_hash tag,
  discriminant-rank list, uniqueness test), the `sample` arm, and `sdf_sphere` (name registration,
  result type `F`, Length slot, eval constructor). Chooses the registration route for the PRD (§7.3).
  Pins that `gradient(sdf_sphere(..))` takes the existing refusal path.
  Modules: `reify-ir` (sdf_expr.rs, value.rs), `reify-expr` (sdf_eval.rs, lib.rs intercept, calculus.rs
  test), `reify-compiler` (type `F`, `builtin_signatures.rs`, units.rs or `reify-builtins` rows), chunk
  docs. **Prereqs:** none. **Unlocks:** β, ι, κ.
  **Signal (LEAF, user-observable):** B1, B2 — `reify eval` prints `<Struct>.z = -0.005 m`.
- **β #8390 — the remaining `sdf_*` builtins (primitives, CSG, smooth union, offset, shell, transform,
  mirror, gyroid, `sdf_from_samples` with the Samples variant) with compiler Length typing, constructor
  domain refusals, the shared labelled-argument refusal + `examples/implicit/sdf_algebra.ri` + its engine
  test.** Modules: `reify-compiler` (registration per α's route, `builtin_signatures.rs`, label check),
  `reify-expr`, `reify-ir` (Samples), `reify-core` (`SdfInvalidArg`, `SdfLabelledArgUnsupported`),
  `examples/implicit/`, `reify-eval/tests` (the `fn_field_example_smoke.rs` shape), chunk docs.
  **Prereqs:** α. **Unlocks:** ε, κ, θ, ο.
  **Signal (LEAF):** B3, B24, B26 — every combinator sampled against its closed form in
  `sdf_algebra.ri` with every constraint `Satisfaction::Satisfied`.
- **γ #8391 — Owner-routed-or-rejected queries and bridges + the post-process `UndefCause` channel.**
  Modules: `reify-eval` (`geometry_ops.rs` `try_eval_geometry_query` / `try_eval_kinematic_query` /
  `try_eval_conformance_query` + `geometry_ops/tests.rs`, `engine_build.rs`
  `post_process_conformance_queries` / `post_process_kinematic_queries`, `realize_solid_sdf.rs`,
  `realization_handles`), `reify-core` (`QueryKernelMismatch`), `tests/prd-gate/fixtures/` (B14 fixture;
  `scripts/verify.sh` `_RUST_COUPLED_RI_FIXTURES` if a Rust test reads it). **Prereqs:** none.
  **Unlocks:** θ, ι, μ, λ. **Signal (LEAF):** B14 on today's `isosurface` body. `E_QUERY_KERNEL_MISMATCH`
  is documented by whichever of γ/ο lands second.
- **δ #8392 — OpenVDB dense-ingest FFI + `ingest_dense_sdf` + `DenseSdfGrid` + `ImplicitOptions` + stub arm +
  trait default + `SingleKernelHolder` delegate + the test-fixtures `tbb::global_control` FFI hook ε's B17
  consumes.** Modules: `reify-kernel-openvdb` (ffi.rs, kernel_real.rs,
  kernel.rs, cpp/openvdb_wrapper.{h,cpp}), `reify-ir` (trait, `DenseSdfGrid`, `ImplicitOptions` with a
  non-zero content_hash domain tag), `reify-geometry/src/lib.rs` (parity test
  `delegates_all_capability_methods_to_inner_kernel`). **Prereqs:** none. **Unlocks:** ε.
  **Signal (intermediate):** B5 round trip with the D-8 activation policy; user-observable through ε.
- **σ #8393 — Stdlib `.ri` fns invisible inside user `fn` bodies (S-6).** Modules: `reify-compiler`
  (`compile_builder/functions_phase.rs::phase_functions`; `traits_phase.rs` unverified). **Prereqs:**
  none. **Unlocks:** ρ. **Signal (LEAF):** a user fn wrapping a field stdlib fn (`pointwise_min`) AND a
  non-field stdlib fn (`standard_bolt_lengths`), in value and constraint position, evaluates identically
  to the direct call (assert Satisfied / equal value, not exit 0); INV-SF-1 provenance on any remaining
  Undef. No σ→β edge: `sdf_*` are Rust builtins and β's fn-wrapped combinator (B3) guards that path.
- **τ #8394 — Companion corrections (docs/yaml ONLY).** `docs/prds/v0_2/multi-kernel.md` (kernel-registration
  paragraph, "All implemented kernels default-on") gains the Fidget supersession note;
  `docs/prds/v0_3/multi-kernel-phase-3.md` §8 Phase 5 task κ entry records "kernel-level only; `.ri`
  signal superseded by implicit-solids"; `review/briefing.yaml` (purpose: five kernels;
  what_working_means: Fidget dual-contouring; known_gaps #8010 entry) is corrected;
  `docs/initial-design/geometry-engine-design-decisions.md` §10.6 points here; all cite #8411 as the Fidget
  owner. The `realize_solid_sdf.rs` doc fix is ι's. **Prereqs:** none. **Signal (LEAF):** the committed edits.

### Phase 2 — Vertical slice

- **ε #8395 — `implicit(..)` builtin + `ImplicitSolid` op + classifier arm (`NO_INPUT`) + descriptor row +
  execute arm + output-side dispatch tier + GUI override entry + hoisted grid policy
  (`MIN_FEATURE_VOXELS_ACROSS`, `DENSIFY_BUDGET_VOXELS`, grid-sizing fn, `ImplicitBudget`) +
  budget-before-allocation + `module_requires_openvdb` detector placed before `realize_for_check` in
  build/check/eval + `ImplicitClipped` / `ImplicitEmpty` / `ImplicitUndefArg` + B18 site +
  `examples/implicit/smooth_bracket.ri` (without `mass`).** Modules: `reify-compiler`
  (`builtin_signatures.rs` `length_arg(2, "feature")`, `LOWERING_ACCEPTED_ARITIES Exactly(&[3, 4])`,
  `NON_SELECTOR_ARG_SLOT_KEYS`; lowering; `compile_builder/entities_phase.rs`), `reify-ir` (op, descriptor
  row, hoisted constants beside `VoxelResolution`, manual Debug), `reify-eval` (`engine_build.rs`
  classifier/tier/overrides, `deps.rs`, lowering sites), `reify-kernel-openvdb` (register.rs,
  kernel_real.rs execute arm, re-exports), `reify-kernel-occt/src/lib.rs` (exhaustive arm), `reify-cli`
  (main.rs detector + placement + B18 site), `reify-core` (codes),
  `reify-eval/tests/harness_geometry/units_length_closure_guard.rs` (#7714 row), `geometry.md` chunk
  (`implicit` joins `GEOMETRY_FUNCTION_NAMES` / `CALLABLE_NAME_REGISTRIES`). **Prereqs:** β, δ.
  **Unlocks:** φ, ζ, η, θ, ι, μ, ν, ξ, ο, π; out-of-plan #7393.
  **Signal (LEAF, user-observable):** B4, B6, B7, B17, B18, B22, B23, B25, B27, B28 — `reify build
  examples/implicit/smooth_bracket.ri --verbose` prints `kernel: openvdb, repr: Voxel`; the e2e asserts
  `produced_repr == Voxel`, zero conversions, and the owner-tessellated mesh meets §7.9 and the
  smooth-union bbox envelope; `crates/reify-eval/tests/voxel_to_mesh_e2e.rs` stays green.
  **G7 waiver: error-severity-exits-nonzero** — B18's coded Error under `cfg(not(has_openvdb))` is not
  Error on a healthy path: that configuration is unsupported (`scripts/check-manifold-deps.sh` makes
  OpenVDB presence fatal; no wasm/OpenVDB target exists), so a design containing `implicit()` is
  unrealizable there, not healthy. Recorded in ε's task metadata.
- **φ #8396 — Sink refusals: `ImplicitNoBrep` (Phase-B STEP arm + `STEPOutput`), `ImplicitUnsupportedOp`
  (per-op loop before dispatch, producer-op identity, intra- and cross-realization parents; isosurface
  Voxel parents excluded), `ImplicitExportUnrouted` (CLI `-o` arm keyed on ImplicitSolid-produced
  bodies, Phase-B `None` ahead of both export arms; declarative `STLOutput`/`ThreeMFOutput` arm keyed on
  `realization_ref`).** Modules: `reify-eval` (export sites, per-op loop, declarative output sites),
  `reify-core` (three codes), `reify-cli/tests/harness_cli`. **Prereqs:** ε. **Unlocks:** η, υ, ο.
  **Signal (LEAF):** B8, B20, B21; `cli_build_voxel_to_mesh.rs` stays green.
- **ζ #8397 — GUI preview of implicit bodies.** Modules: `gui/src-tauri` (test in the
  `openvdb_kernel_tests` shape; override already landed by ε). **Prereqs:** ε. **Signal (LEAF):** B9
  (`mesh_stats_json` `face_count`, `bounding_box`).
- **η #8398 — Extend `OpenVdbKernel::export` as landed by #6308 (whose acceptance on `voxel_to_mesh.ri` may
  land the STL arm) to grid handles via `tessellate` and ThreeMF via `write_3mf`; `Obj`, `Step` → `Err`;
  CLI triangle floor and bbox test; removes φ's CLI `ImplicitExportUnrouted` arm by identity and retires φ's B21 CLI-arm `harness_cli` test in the same diff (B21's declarative twin stays until υ).** Modules:
  `reify-kernel-openvdb` (discriminating check keyed on `write_3mf`), `reify-eval` (refusal site),
  `reify-cli/tests/harness_cli`. **Prereqs:** ε, φ; out-of-batch **#6308**. **Unlocks:** ξ, υ.
  **Signal (LEAF):** B10, B35.
- **υ #8399 — Retire the declarative `ImplicitExportUnrouted` arm and delete the variant once #6366 routes
  `STLOutput`/`ThreeMFOutput` by owner.** Modules: `reify-eval` (declarative output sites), `reify-core`
  (variant deletion). **Prereqs:** φ, η; out-of-batch **#6366**. **Signal (LEAF):** B34 (B21's twin flips).

### Phase 3 — Measurement and the collision gate

- **θ #8400 — `sdf_volume`, `sdf_clearance`, `sdf_penetration` intercepts + operand clipping/empty-domain
  handling + derived-lets drain of γ's `UndefCause` channel + the `mass` line of `smooth_bracket.ri`.**
  Reuses ε's grid-sizing fn, `ImplicitBudget` and `ImplicitClipped`, β's `SdfInvalidArg`. Modules:
  `reify-expr`, `reify-compiler`, `reify-eval` (derived-lets sink), `examples/implicit/smooth_bracket.ri`,
  chunk docs. **Prereqs:** β, γ, ε. **Unlocks:** λ, ν, ο. **Signal (LEAF):** B11, B12, B29 within §7.8,
  including the post-process-derived-argument case observed in `reify check` output.
- **ι #8401 — `sdf_from_solid(solid, voxel)` post-process + `FromSolid` variant + `is_realizable` + coded
  degradation of `realize_solid_sdf_at` + the `sdf_from_solid` detector arm + B19's compile-time closure
  check + the defensive runtime check in ε's lowering + the `realize_solid_sdf.rs` doc fix.** Modules:
  `reify-eval` (post-process pass at both sites, UnifiedDag exclusion, realize_solid_sdf.rs, lowering
  check), `reify-compiler` (signature; closure walk; `SdfFromSolidUnresolvableSubject`), `reify-ir`
  (FromSolid, `is_realizable`), `reify-expr` (eval arm), `reify-core` (`SdfFromSolidNoKernel`,
  `SdfFromSolidFailed`, `SdfFromSolidUnresolvableSubject`, `SdfFromSolidPreviewUnavailable`,
  `ImplicitFromSolidInRealization`), `reify-cli/src/main.rs` (detector arm), chunk docs. **Prereqs:** α,
  γ, ε. **Unlocks:** λ, ο. **Signal (LEAF):** B13 both halves, B19, B31, B32.
- **κ #8402 — `sdf_sweep(field, snapshots, body: BodyId)`.** Modules: `reify-expr`, `reify-ir` (Sweep),
  `reify-compiler`, `reify-stdlib` (`geometry.rs::decompose_transform` visibility), `reify-core`
  (`SdfSweepMisuse`), chunk docs. **Prereqs:** α, β. **Unlocks:** λ, ο. **Signal (LEAF):** a swept
  `sdf_sphere` along a two-snapshot translation samples negative at both poses and positive midway
  beyond r (within 1e-12m tolerances per B3); B30. The `TODO(#6007)` at the signature row meets the PTODO
  fingerprint ratchet (`tests/infra/test_reify_audit_ptodo.sh`): run it DB-absent before landing and
  regenerate `crates/reify-audit/ptodo-baseline.txt` in the same diff if the cite fingerprints.
- **λ #8403 — Printer tool-dock gate on `sdf_*` (dogfood integration gate).** Modules:
  `prj/printer_v01/printer.ri`, `docs/projects/printer_v01.md`, two committed extracted fixtures
  `tests/prd-gate/fixtures/implicit_solids_tooldock_gate.ri` (ToolBody + ToolDock: the old y-pair
  sampled-union gates, the new y- and x-pair `sdf_*` gates, and NOT the old x-pair union, which SIGSEGVs
  under #7383 as the whole file does) and `implicit_solids_tooldock_gate_seeded.ri` (pitch = 55 mm), both
  registered in `_RUST_COUPLED_RI_FIXTURES`; `crates/reify-eval/tests/harness_sweep/idler_seat_e2e.rs`
  (`check_printer` runs kernel-free: extend `PRINTER_VOLUME_UNRESOLVED` with the new ToolDock cells, which
  report `EvalUnresolved` there per ι). Sketch evidence: `tests/prd-gate/fixtures/implicit_solids_collision_gate.ri`.
  **Prereqs:** γ, θ, ι, κ (ε
  transitively through θ and ι; #7383 cited, NOT an edge). **Unlocks:** π, ρ.
  **Signal (LEAF, user-observable):** B15 on the extracted fixture `tests/prd-gate/fixtures/implicit_solids_tooldock_gate.ri`; the whole-file check is noted, not
  asserted; the sampled-union gate is kept beside the new one in `printer.ri` for one release; h, τ (≥ 2h), per-body
  padded-grid memory, runtime and the sub-τ blind spot are recorded in the project doc. #8260
  (coordinate-target-fea λ, pending) is a design precedent for a leaf editing `prj/` and a file-lock
  neighbour, not landed evidence.

### Phase 4 — FEA consumer

- **μ #8404 — FEA on an implicit body at fixed parameters.** `solve_elastic_static(material, body, loads,
  supports, ElasticOptions(..))` (the 5-arg body overload) on an implicit body; the `ElasticOptions`
  disposition table on the implicit route (every param honoured with its site named, or coded-refused when
  non-default). Modules: `reify-eval` (VolumeMesh edge — already owner-tessellating, work limited to the
  disposition; `mc_adaptivity` only if measured necessary, §10 Q3), `examples/implicit/fea_implicit_bracket.ri`,
  `reify-eval/tests` e2e (body harness `solve_elastic_static_body_e2e.rs`; differential shape from
  `harness_fea_solver_e2e/fea_bracket_minimize_mass_e2e.rs`; heavy-test-filter atom registered in the same
  diff), `reify-cli/tests` (the `reify eval` run). **Prereqs:** ε, γ (ordering safety: no foreign-id kernel
  query on the body); out-of-batch **#6660**, **#7052**. **Unlocks:** ρ. **Signal (LEAF):** B16, B33 —
  the engine e2e AND the CLI run (μ keeps the hard edges and therefore asserts the CLI itself).

### Phase 5 — Inputs, docs, re-home, corrections

- **ν #8405 — `implicit()` and measurement over `fn_field` lambda fields.** `examples/implicit/lambda_sdf.ri`
  indexes `p.x/p.y/p.z` and uses `sqrt`/`abs`/`max` (no Vector3 projection — #6892 is not a dependency);
  a Field whose domain is not `Point3<Length>` is refused with `SdfInvalidArg`. Modules: `reify-expr`,
  `reify-compiler`, `examples/implicit/`, chunk docs (self-documenting). **Prereqs:** ε, θ; out-of-batch
  **#6871**. **Unlocks:** ρ. **Signal (LEAF):** `lambda_sdf.ri` builds, samples and measures correctly
  (Satisfied constraints); the unannotated-domain refusal fires.
- **ξ #8406 — Imported VDB SDF → `sdf_from_samples` → `implicit` → STL.** A Rust test-time generator
  (`OpenVdbKernel::realize_voxel_from_mesh` + `write_vdb_grid` into a tempdir; CLI run with CWD=tempdir via
  `common::run_with_args_in`, as `cli_imported_field_eval.rs`); no committed binary, no Python (no
  pyopenvdb on the host). Fixture: a 20 mm cube SDF in SI metres, no units tag, feature 2 mm ⇒ h 0.5 mm,
  A = 2400 mm² ⇒ floor 4,800 (≈ 19.8 k expected); `implicit()`'s extent stays inside the VDB bounds. The
  example runs only from a directory holding `fixtures/<x>.vdb` (README note like
  `examples/imported_field/README.md`). The VDB's finite samples end ≈ 11 mm from the centre (cube half
  10 mm + band 3 × 0.5 mm, probed at decompose), so the extent is 21 mm; grammar evidence
  `tests/prd-gate/fixtures/implicit_solids_imported_sdf.ri`. Modules: `examples/implicit/`, `reify-cli/tests`. **Prereqs:** ε,
  η. **Unlocks:** ρ. **Signal (LEAF):** `examples/implicit/imported_sdf_to_stl.ri` → `Triangles: N ≥ 4,800`.
- **ο #8407 — Docs-truth: `fields.md`/`geometry.md` coherence pass (every documented signature compiled in a
  smoke `.ri`), `examples/best_practices/implicit_solids.ri` + its `INDEX.md` row (same commit —
  bidirectional pin), the `.claude/skills/reify-design/SKILL.md` index line, discoverability ("check two
  parts don't collide", "lattice", "smooth blend" find `sdf_*`).** The exemplar is swept kernel-free by
  `crates/reify-eval/tests/harness_corpus_gates/best_practices_constraint_gate.rs` (bidirectional
  `EXPECTED_INDETERMINATE` pin): its constraints are pure `sdf_*` sampling (Satisfied kernel-free), or the
  rows are added in the same diff. **Prereqs:** β, ε, φ, θ, ι, κ.
  **Unlocks:** ρ. **Signal (LEAF):** the four docs-truth acceptances.
- **π #8408 — Fidget measurement checkpoint.** Record, in `docs/notes/implicit-solids-baseline.md` (dated,
  as-of SHA, release profile, host named, load average): end-to-end `reify build`/`reify check` of a
  release `reify` built in the lane under `taskset -c <one core>` with `/usr/bin/time -f '%e %U %S'`,
  best-of-N, minus a trivial-module baseline; subjects: λ's committed extracted fixture (stages as λ runs
  them: from_solid voxelisation, Sweep/FromSolid sampling, penetration sampling — no marching cubes) and
  `smooth_bracket.ri` at feature 2 mm and 0.5 mm. No stage split (no timer exists; t1 is an end-to-end
  budget). **Prereqs:** λ, ε. **Unlocks:** #8411. **Signal (LEAF):** the committed note; #8411 compares the §11
  trigger against it.
- **ρ #8409 — Re-home `sdf_*` under `std.implicit`.** A real `std.implicit` stdlib module whose pub names
  reach the builtins (wrapper `.ri` fns — needs σ — or a builtin-namespace alias table); an explicit name
  table (`sdf_box → std.implicit.box`, `sdf_sphere → std.implicit.sphere`, `sdf_union → std.implicit.union`,
  … and the names for `sdf_volume`, `sdf_from_solid`, `sdf_sweep`; `implicit`, like `isosurface`, stays a bare geometry-constructor name); bare names kept one minor
  release with `SdfDeprecatedBareName` W (new code, message-prefixed, one emit site); migrates every
  in-repo bare use. Self-documenting. **Prereqs:** σ, ο, λ, μ, ν, ξ; out-of-batch **#5505**, **#5500**
  (transitively **#5499**). **Signal (LEAF):** `import std.implicit as sdf; sdf.box(..)` **evaluates** to
  the same value as the bare call (unresolved qualified calls exit 0 today, so an exit code proves nothing);
  bare names emit the W (§10 Q6).
- **ω #8410 — PRD close.** **Prereqs:** all 21 other leaves — α β γ δ ε φ ζ η θ ι κ λ μ ν ξ ο π ρ σ τ υ. **Signal:** the terminal `Status` header per
  the overlay's freeze shape, on the PRD and its manifest. ω may wait long on the out-of-batch chains
  (#5505/#5500, #6871, #6308, #6660/#7052, #6366) — accepted.

### Out-of-plan tasks

Out-of-plan tasks carry `prd_path` but no `prd_task_label` and have no manifest block (#8411 is filed in this batch; #7393 already exists):

- **#7393** (existing task, filed 2026-09-11 from #7009's triage; depends on #5403) — "cmd_check and
  cmd_eval never register the OpenVDB kernel for isosurface modules". Not re-filed. ε's
  `module_requires_openvdb` now delivers the OpenVDB loading #7393 proposed; the decompose rescopes #7393 to
  the residual it did not trace — `realize_for_check` demands BRep, so the isosurface Surface terminal is
  dispatched to OCCT even with OpenVDB loaded (`examples/multi_kernel/voxel_to_mesh.ri`: `GeometryOp::Surface
  … must not reach OcctKernel::execute()`, rc=1) — plus making the executor's silent OCCT reroute loud, and
  adds the edge #7393 → ε. Removed from ε's signal.
- **#8411** (F3 at drafting) — "[MILESTONE] Fidget evaluator re-open decision (implicit-solids §11 t1–t4)".
  `metadata.execution_class: "decision"`, depends on π. On dispatch it escalates: compare π's baseline to
  t1; t2–t4 are product signals; outcome is either "author the Fidget-evaluator PRD" or "park as a deferred
  external-trigger bookmark". Owns `crates/reify-kernel-fidget` (G7 nothing-vacuous-and-unowned). τ and
  §11 cite it.

### Dependency view

```
Phase 1   α ─→ β          γ          δ          σ          τ        (α, γ, δ, σ, τ: no intra prereqs)
Phase 2   β, δ ─→ ε ─→ φ ;  ε ─→ ζ ;  ε, φ (+#6308) ─→ η ;  φ, η (+#6366) ─→ υ
Phase 3   β, γ, ε ─→ θ ;  α, γ, ε ─→ ι ;  α, β ─→ κ ;  γ, θ, ι, κ ─→ λ   (#7383 cited, no edge)
Phase 4   ε, γ (+#6660, #7052) ─→ μ
Phase 5   ε, θ (+#6871) ─→ ν ;  ε, η ─→ ξ ;  β, ε, φ, θ, ι, κ ─→ ο ;  λ, ε ─→ π
          σ, ο, λ, μ, ν, ξ (+#5505, #5500, #5499) ─→ ρ
Close     all 21 leaves (α…υ and φ) ─→ ω
Out of plan   ε ─→ #7393 ;  π ─→ #8411

α ──┬─→ β ──┬─→ ε ──┬─→ φ ──┬─→ η (+#6308) ──┬─→ ξ
    │       │       │       │                 └─→ υ (+#6366)
    │       │       │       ├─→ ζ
    │       │       │       ├─→ μ (+γ, #6660, #7052)
    │       │       │       ├─→ θ (+β, γ) ──┬─→ ν (+#6871)
    │       │       │       │               └─→ λ ──→ π
    │       │       │       └─→ ι (+α, γ) ──→ λ
    │       └─→ κ ──────────────────────────→ λ
    └──────────────────────→ ι
δ ──→ ε ;  γ ──→ θ, ι, μ, λ
```

### Per-leaf premise notes (G6 / manifest seed)

- α/B1–B2: closed-form identity — each primitive's `sdf_eval` *is* the textbook distance; exactness
  holds for primitives, rigid transforms and mirrors only (I-2). B2 is producer-self because α registers
  `sdf_sphere`; the printed line is a structure let's `<Struct>.<cell> = -0.005 m`.
- β/B3: `reify check` `==` is exact f64 ((p+t)−t is not bit-exact; probe `(1mm + 12mm) - 12mm == 1mm`
  → VIOLATED), so comparisons involving coordinate arithmetic are `abs(a − b) <= 1e-12m`; the engine
  test asserts Satisfied because `examples_smoke` fails only on Error diagnostics and check exits 0 on
  INDETERMINATE.
- γ/B14: observable today only under `reify build -o` (check/eval cannot load OpenVDB before ε, and a
  declarative build without `-o` demands BRep); the assertion is the code, the INDETERMINATE verdict and
  the Undef cause — never the exit code or the STL.
- δ/B5: the active voxel-centre bbox of a padded grid spans (n−1)·h, so "bbox == extent" can never hold
  for a correct ingest; the activation policy is pinned in D-8.
- ε/B7: end-to-end capability traced to ε's own dependency set (β, δ) plus shipped OpenVDB marching
  cubes; nothing downstream of ε is required. ε's example does not assert an STL (φ refuses it). The
  `voxel_to_mesh.ri` check failure is #7393, not ε's.
- φ/B20–B21: negative assertions on the variant and site; on main the unresolved builtins compile with a
  warning and take their result type from their arguments, so "exit ≠ 0" would be vacuous.
- η/B10: requires #6308 upstream — DAG-direction PASS only once the dependency is wired; #6308 may land
  the STL arm first, so η's discriminating check is `write_3mf`.
- θ/B11–B12: floors stated in §7.8 with their counting basis; the bias ceiling is below the half-voxel
  bias cost and 3×measured is recorded first. B11/B12 are pure cells in `Engine::eval`; the
  post-process-derived case is what observes the derived-lets drain.
- ι/B13, B19: B13's positive half needs OpenVDB under `reify check`, which ι's own detector arm
  supplies (hence ι ← ε); B19 is producer-self only because ι introduces `sdf_from_solid` (on main the
  name compiles with a warning, rc=0).
- κ/B30: an empty pose list would make min over ∅ = +∞ and a swept-volume gate vacuously Satisfied; the
  refusal is the guard.
- λ/B15: parity is per-gate Satisfied-vs-Satisfied, never "exit 0" (a non-strict check exits 0 on
  INDETERMINATE); the whole-file check SIGSEGVs (#7383) so the durable signal is the extracted fixture;
  the sub-τ blind spot is recorded, not hidden.
- μ/B16: fixed parameters only — geometry-backed constraints on auto params are declined on main
  (`EvalUnresolved`) and realization runs once per build, so no "interior optimum" is reachable by any
  leaf; the monotone differential is the anti-vacuity check; gmsh is a dev-dep as
  `solve_elastic_static_body_e2e.rs` already does; the heavy-test filter atom lands in the same diff.
- ξ: the fixture is generated at test time in SI metres; `validate_grid_units` checks dimension only, so a
  mis-scaled unit is undetectable by construction and the generator convention is the guard.
- ρ: the signal asserts an evaluated value because unresolved qualified calls exit 0 today.
- B8/B14/B19–B23, B24–B31: negative assertions — each must observe the coded diagnostic fire
  (rejection-check) on the named variant and site.

## 9. Out of scope for this PRD

- **Fidget as evaluator / `ReprKind::Sdf` demand / `#kernel(fidget)`.** Triggered by §11; evaluated by #8411.
- **`gradient(f)` on an `Sdf` field.** `calculus.rs::validate_differentiable_field` accepts only
  Analytical/Composed lambdas on main; α pins the refusal path (§7.2).
- **Geometry-in-the-loop optimisation** (an implicit body inside `minimize`/auto-param solves, FEA-driven
  topology or free-form optimisation): realization runs once per build and geometry-backed constraints on
  auto params are declined (`engine_fixpoint.rs::run_unified_pass`); the future PRD is listed in
  `docs/prds/v0_6/engine-unified-build-dag.md`. μ covers the fixed-parameter solve only.
- **OBJ export of implicit bodies.** reify-ir has no OBJ writer (`write_stl_binary`, `write_stl_ascii`,
  `write_3mf` only); OCCT and Manifold return `Err` for Obj; `cmd_build` has no `.obj` mapping.
- **Level-set pruning / activation optimisation** of the ingested grid (§10 Q9): v1 keeps every voxel of
  the padded block active.
- **Bounds inference** through the combinator tree (§10 Q2).
- **TPMS beyond gyroid** (Schwarz P/D, Neovius); **lattice sugar** (`lattice_infill`); **non-rigid
  transforms** (scale, shear) on fields.
- **Mixed BRep/implicit modelling inside one `implicit()`** (`from_solid` in a realized field; needs
  two-phase realization) and **ops with an implicit parent** (Boolean/Transform/Pattern/Modify) — refused
  in v1. The field-level route (`sdf_penetration` over `from_solid` fields) covers the collision use case.
- **Level-set renormalisation** (exact Euclidean distance after CSG/offset via OpenVDB
  `LevelSetFilter`) — a later `implicit(.., renormalise)`.
- **Parameter sensitivities** of `sdf_*` fields through dual AD (S-12), and any optimiser/NN program;
  this PRD supplies the generator and the decode-never-fails property, nothing more.
- **BC placement by spatial predicate** on non-BRep bodies (meshing-service / FEA PRDs own BCs).
- **Surface remeshing** of marching-cubes output for FEA quality; **watertightness** of `volumeToMesh`
  output for gmsh is assessed in μ and recorded, not guaranteed here.
- **Interval-sound sweeps** (no tunnelling proof); **OpenVDB-served kernel queries** on implicit solids;
  **typing `sweep`'s result** (#6007).
- **`std.implicit` namespace** before stdlib-namespace ν/η land (ρ waits); **label binding** (#6313 lifts
  β's refusal).

## 10. Open questions

1. **Smooth-union kernel.** Polynomial (`h = clamp(0.5 + 0.5(b−a)/k)`, `mix(b, a, h) − k·h(1−h)`) vs
   exponential. **Suggested:** polynomial (bounded, no exp). Decide during β.
2. **Bounds inference.** Carry an optional AABB on `SdfExpr` nodes so `extent` can default.
   **Suggested:** v2. Decide at ω / follow-up.
3. **Marching-cubes adaptivity for FEA surfaces — DECIDED (R21).** `ImplicitOptions` carries no
   `adaptive` field (no reader; collides with `ElasticOptions.adaptive`). A knob is added only if μ
   measures a need, named `mc_adaptivity`, read at the VolumeMesh-edge owner tessellation via
   `GeometryKernel::realize_mesh_from_voxel(handle, iso, adaptive)`, never sourced from
   `ElasticOptions.adaptive`.
4. **`sdf_volume` surface sub-sampling depth** (h/2 vs h/4). **Suggested:** h/2; measure in θ.
5. **Budget diagnostic for measurement builtins — DECIDED (R2).** Share the code: ε hoists the
   constants, the grid-sizing fn and `ImplicitBudget` into reify-ir; θ (which now depends on ε) reuses
   them. Requested/max counts are asserted through the typed Rust error in a unit test and appear in
   message text only.
6. **Deprecation window for bare names after ρ.** **Suggested:** one minor release.
7. **Typing `sweep`'s snapshot list** so `sdf_sweep`'s second parameter can be nominal. Owned by
   **#6007** (lead adds a consumer note); D-12's `SdfSweepMisuse` stands until then.
8. **GUI preview of fields that contain `from_solid` — DECIDED (R34).** Preview-path degradation is
   `SdfFromSolidPreviewUnavailable` **W** + `UndefCause` (never Error on a healthy design); a static helper
   over (`geometry_kernels`, `default_kernel_name`, `named_steps`) may remove it, since
   `tessellate_from_values` lacks only `self.realization_handles`.
9. **Level-set pruning / activation.** v1 keeps every voxel of the padded block active (D-8). Whether a
   later ingest should prune or tolerance-deactivate |v| ≥ band·h voxels (and what the active bbox and
   interior tiles then read) is open; any change re-pins B5. Decide after π's baseline.

## 11. Supersession and the Fidget deferral record

- **Task #8010** ("Link reify-kernel-fidget into the production CLI and GUI") is **cancelled** by the
  2026-10-08 session (esc-8010-4). Linking is routing-inert (§2.1) and ships a kernel that cannot mesh a
  20 mm part. This supersedes Leo's 2026-09-28 "unintended gap" ruling and, for Fidget only, the
  `multi-kernel.md` "All implemented kernels default-on" decision: Fidget is not *implemented* in that
  decision's sense (unroutable; metre-scale domain). Leaf τ records both.
- **Fidget re-open triggers** (any one; π is the baseline — release profile, one pinned core, main host,
  end-to-end): (t1) end-to-end `reify check` of λ's committed extracted fixture, or `reify build` of
  `smooth_bracket.ri` at feature 0.5 mm, exceeds 2 s wall (best-of-N under `taskset -c <one core>`, minus
  the trivial-module baseline — the GUI slider budget the kinematics PRDs use; there is no stage timer, so
  the budget is end-to-end); (t2) a consumer needs an interval-sound (tunnelling-free) sweep or containment
  proof — Fidget's interval evaluator is the named provider; (t3) a consumer needs sharp-feature
  preservation marching cubes cannot deliver — Fidget's dual contouring is the named provider; (t4) the
  free-form optimisation program needs analytic ∂field/∂param beyond dual AD's cluster limit — Fidget's
  `Tree::deriv` is the named provider. **Owner action:** out-of-plan milestone **#8411** (depends on π)
  compares the baseline to t1 and either authors the Fidget-evaluator PRD or parks as a deferred
  external-trigger bookmark. The adoption shape is a lowering `SdfExpr → fidget::Tree` behind the same
  `implicit()` and measurement builtins, plus a per-body `(Tree, AABB)` mesher — never a second surface
  syntax.
- Until a trigger fires, `crates/reify-kernel-fidget` stays a workspace member (its tests run in verify)
  and unlinked, owned by #8411. Known defects to fix on adoption: no `reset()` (INV-GEO-3), fixed `[-8,8]³`
  domain, depth cap 7, f32 numerics, JIT-only (`VmShape` fallback absent).

## 12. Decompose amendments (2026-10-08)

Lead rulings applied in this pass (main `586b793377`; evidence from the phase-A seats
manifest:algebra/queries/slice-core/slice-refusals/egress/integration, fixtures, g7). Each entry names
what changed and the probe or symbol that forced it.

1. **R1 — α scope.** α now delivers `sdf_sphere` end-to-end so B2 is producer-self, lands the fourteen
   core variants (Samples/FromSolid/Sweep arrive with β/ι/κ), chooses the registration route (I-REG-1
   forbids `"name" =>` dispatch for registered names; `builtin_signatures.rs` is a slot checker, not a
   registry), drops `lipschitz_bound` (no production reader, G7), moves `is_realizable` to ι, and pins
   the `gradient(sdf)` refusal path (`calculus.rs::validate_differentiable_field` accepts only
   Analytical/Composed). §7.1–7.3, §8 α, §9.
2. **R2 — budget / grid policy.** ε hoists `MIN_FEATURE_VOXELS_ACROSS`, `DENSIFY_BUDGET_VOXELS` and a
   grid-sizing fn into reify-ir (reify-expr cannot name reify-kernel-openvdb) and adds `ImplicitBudget`;
   θ gains ε (and γ) as prereqs and reuses them; Q5 DECIDED. D-7, §8 θ, §10 Q5.
3. **R3 — ε ↔ ι.** ι's prereqs are α, γ, ε; ι owns the builtin, FromSolid, `is_realizable`, the pass, the
   detector arm, B19 with `ImplicitFromSolidInRealization`, and the defensive lowering check (on main an
   unresolved `sdf_from_solid` compiles with a warning, rc=0, so ε's B19 would pass vacuously). D-11,
   §7.7, §8 ι, B19.
4. **R4 — ε's S-7 clause.** Dropped from ε's signal: with OpenVDB loaded, `realize_for_check` still
   demands BRep and `GeometryOp::Surface` reaches `OcctKernel::execute` (measured on `voxel_to_mesh.ri`,
   rc=1) → #7393. `module_requires_openvdb` is placed before `realize_for_check`; dispatch classifies
   ImplicitSolid with `NO_INPUT` and keys the tier on an output-side predicate (a `&[Voxel]` input class
   is value-identical to Surface's). S-7, §3 Gap 3, D-6, §7.4–7.5, §8 ε, #7393.
5. **R5 — γ scope.** `distance_between_placed` and `resolve_export_body_color` removed (owned by #6366
   items 1 and 3; need #6308's `ExportBody.kernel`); `post_process_conformance_queries` and
   topology selectors added; owner identity threaded inside the three `try_eval_*_query` fns (the pinned
   tests `*_resolves_via_kernel_handle_id` flip); `realization_handles` keeps the owner;
   `realize_solid_sdf_at` returns a typed failure enum; one post-process `UndefCause` channel; per-site
   dispositions (author query → Error, DFM rule → Indeterminate + W, kinematic walks only on request);
   B14 rewritten as a named-let fixture under `reify build -o` (the literal `volume(isosurface(box(20mm)))`
   does not compile; today volume is silently 0 m³, VIOLATED). S-9, §3 Gap 3, §7.6, §8 γ, B14, B35.
6. **R6 — diagnostic codes.** `DiagnosticCode` is a fieldless `Copy` enum (212 unit variants); struct
   payloads replaced by message text with the mnemonic as prefix (`main.rs::report_eval_output` prints
   `{severity}: {message}`); every variant named with its owner; "one site each" → "one site per sink".
   Goal transcript, D-9, §7.6–7.7, every B-row.
7. **R7 — interim export refusal.** `ImplicitExportUnrouted` keyed on ImplicitSolid-produced bodies
   (an owner-keyed trigger fires on `voxel_to_mesh.ri -o x.stl`, exit 0 today, pinned by
   `cli_build_voxel_to_mesh.rs`), defined independently of #6308's landing order, firing at the export
   sink with Phase-B `None` ahead of both arms (today an Error still writes the file: 15,572 bytes probed);
   declarative arm keyed on `realization_ref`; new leaf υ (φ, η, #6366) retires the declarative arm and
   the variant. §5, §7.5, §8 φ/η/υ, B21, B34.
8. **R8 — UNSUPPORTED_OP.** Sited in the per-op loop before dispatch keyed on producer-op identity
   (B20's fixtures inline the implicit call, visible only via `realization_step_ids`;
   `named_step_reprs` stores a ReprKind and cannot tell ImplicitSolid from an isosurface Voxel);
   isosurface parents excluded; selectors move to γ. §7.6, §8 φ, B20.
9. **R9 — B22.** Only the FEATURE slot can reject (`ArgTypeMismatch`); a Length in a Vector3 position is
   accepted on main (probe `transform3(orient_identity(), 2mm)` rc=0; no `ExpectedArg::Vector`). ε
   registers `length_arg(2, "feature")`, `Exactly(&[3, 4])` and the `NON_SELECTOR_ARG_SLOT_KEYS` row. D-4,
   §7.3, B22.
10. **R10 — argument labels.** Labels on this PRD's rows are refused with `SdfLabelledArgUnsupported`
    (β) until #6313: `atan2(x: 1.0, y: 0.0)` returns π/2 silently today and the new rows are dense with
    same-typed asymmetric slots. S-3, D-4, §5, B26.
11. **R11 — half-space.** `sdf_half_space(plane: Plane)`, unit normal by construction (a `Vector3<Real>`
    stand-in let a non-unit n silently scale the field and break I-2). §7.1, §7.3.
12. **R12 — value domains.** Constructors refuse out-of-domain inputs with `SdfInvalidArg` (SmoothUnion
    k > 0, Gyroid cell > 0, wall ≥ 0, radii/half-extents > 0, finite inputs); ε/θ validate feature > 0
    and extent > 0 before the budget arithmetic; I-1 holds by construction. §3 Gap 1, D-7, §7.1, B24, B25.
13. **R13 — B3.** `reify check` `==` is exact f64 (probe `(1mm + 12mm) - 12mm == 1mm` → VIOLATED);
    coordinate-arithmetic comparisons are `abs(a − b) <= 1e-12m`. B3, premise note.
14. **R14 — β CI.** β ships an engine test (the `fn_field_example_smoke.rs` shape) asserting every
    `sdf_algebra.ri` constraint Satisfied (`examples_smoke` fails only on Error diagnostics; check exits 0
    on INDETERMINATE); one fn-wrapped combinator asserted equal to the direct call; `sdf_from_samples`
    domain-form acceptance is β's only, dimension-only. §6 G1, §7.3, §8 β, B3.
15. **R16 — κ.** Decode via reify-stdlib `geometry.rs::decompose_transform` (not reify-eval's private
    `accept_transform_to_arrays`, unreachable from κ's crates); the body record is the `bodies` element
    whose `id` equals the BodyId; slots 2–3 compile-unchecked (`sweep` compiles as `Real`; probe `no
    matching overload for probe(Real)`); "never a bare Int" dropped; `SdfSweepMisuse` covers non-List,
    non-snapshot, empty list, missing body; #6007 cited at the signature row. D-12, §7.3, §7.7, §8 κ, B30.
16. **R17 — σ.** Restated as "stdlib `.ri` pub fns are invisible inside user fn bodies" (probed on
    pointwise_min, compose, threshold, standard_bolt_lengths; cause area
    `functions_phase.rs::phase_functions`); signal covers a field and a non-field fn in value and
    constraint position, asserting Satisfied; σ is a prereq of ρ; no σ→β edge. S-6, §8 σ.
17. **R18 — θ bounds and domains.** Volume: sound bound A·h plus bias ceiling A·h/3 ≈ 209 mm³ (the
    saved `max(3×measured, A·h/2)` floor could never catch the 306.4 mm³ inward half-voxel bias);
    clearance/penetration: feasibility-restricted derivation, assert ≤ 1.5·L·h (the saved L·h had zero
    margin in the worst alignment); clipped/empty operands coded (a clipped `a` made penetration read 0 →
    gate vacuously Satisfied); a post-process-derived case observes the derived-lets drain; θ appends the
    `mass` line. D-10, §7.8, §8 θ, B11, B12, B29.
18. **R19 — δ.** δ delivers `ImplicitOptions { voxel_size, band_voxels }` (no `adaptive`: no reader,
    name collision with `ElasticOptions.adaptive`) and the f32 `DenseSdfGrid` (`SampledField.data` is
    `Vec<f64>`, 8 B/voxel); activation policy pinned (every voxel active); B5 bounds restated as
    centre ± (n−1)·h/2 (the active voxel-centre bbox of a padded grid can never equal the extent);
    `SingleKernelHolder` delegate; manual Debug; `args` carry field/extent/feature/centre for `deps.rs`.
    D-7, D-8, §7.4, §8 δ, B5, §10 Q9.
19. **R20 — ε other.** Modules add `reify-kernel-occt/src/lib.rs`, `deps.rs`, `entities_phase.rs`,
    `units_length_closure_guard.rs` (exhaustive matches); C4 sentence replaced (`check_length_field`
    classifies `&Value`s and OpenVDB has no tripwire site; the guard `implicit()` meets is Contract C's
    closure guard); B7 names `reify build … --verbose` without `-o` and moves the zero-conversion
    assertion to a crate-internal test (`RealizationKernelProvenance` has no conversion field); B17
    specifies two fresh engines under `tbb::global_control` (the cited precedent has neither run-twice nor
    a thread control; a same-engine rebuild is a cache hit); B18 assigned to ε with I-7 restated as "not
    registered" (`register.rs` omits the inventory submit) and the G7 waiver recorded; `ImplicitEmpty` W
    added (an empty body tessellated to 0 triangles at exit 0); `ImplicitUndefArg` disposition (the
    code-less `CHECK_ERROR_EXIT_ALLOWLIST` entry would excuse a genuine design error); ε ships
    `smooth_bracket.ri` without `mass`; the bbox envelope is the smooth-union extent (z_min = −3.5 mm);
    the Approach line's crate list widened. §1, §7.3–7.4, §8 ε, B7, B17, B18, B27, B28.
20. **R21 — μ re-scope (G6, premise false).** Probe `isdec-egress-mu1.ri`: a constraint whose
    geometry-backed inputs reach an auto param is declined (`engine_fixpoint.rs::run_unified_pass`,
    `EvalUnresolved`, rc=1) and realization runs once per build, so "inside minimize … interior optimum"
    is unreachable; the 4-arg body call does not exist (only the 5-arg overload with `ElasticOptions`).
    μ becomes a fixed-parameter solve with a monotone-differential anti-vacuity check, example
    `fea_implicit_bracket.ri`, no mass objective (no θ edge), CLI asserted by μ (hard edges #6660/#7052
    kept), `ElasticOptions` disposition table, `mc_adaptivity` only if measured; precedent paths cited by
    role. Geometry-in-the-loop moved to §9 with the `engine-unified-build-dag.md` pointer. §1, §2.1, §3
    (c), D-15, §6 G1 (c), §8 μ, B16, B33, §10 Q3.
21. **R22 — η.** OBJ dropped (reify-ir has no OBJ writer; `cmd_build` has no `.obj` mapping); η extends
    `OpenVdbKernel::export` as landed by #6308 (which may land the STL arm) with the discriminating check
    on `write_3mf`; η's prereqs ε, φ. §7.4, §8 η, §9, B10.
22. **R23 — ζ.** B9 asserts `face_count` and `bounding_box` (`commands.rs::mesh_stats_json`;
    `triangle_count` does not exist). B9.
23. **R24 — ξ.** Rust test-time generator (no pyopenvdb on the host; no committed binaries), named
    20 mm cube fixture with floor 4,800, SI-metre convention, README note; domain-form acceptance removed
    from ξ (β's). §7.9, §8 ξ.
24. **R25 — λ (G6).** `reify check prj/printer_v01/printer.ri` SIGSEGVs (rc=139, OCCT Fuse in ToolDock
    mech_x, #7383); the durable signal is a committed extracted fixture per the #8260 pattern, with per-gate
    parity for the four named gates, a seeded violation, x-pair determinacy, and the sub-τ blind spot
    recorded; #7383 cited, not an edge; #8260 cited as precedent and file-lock neighbour, not landed
    evidence. §1, §5, §6, §8 λ, B15.
25. **R26 — ν.** Prereqs ε, θ, #6871 (the measurement builtins are θ's); `lambda_sdf.ri` indexes
    `p.x/p.y/p.z` with `sqrt`/`abs`/`max` (Vector3 projection is #6892, not a dependency); a non-
    `Point3<Length>` domain is refused with `SdfInvalidArg` (`sample` silently accepts a Point3 for an
    unannotated `fn_field` today). S-1, D-2, §8 ν.
26. **R27 — docs.** Every language-surface leaf documents its names in the same diff (PDOCCOVER and the
    fence gate red otherwise; `geometry_chunk_smoke.rs` requires a `CALLABLE_NAME_REGISTRIES` entry); ο's
    prereqs are β, ε, φ, θ, ι, κ (`sdf_from_solid`/`sdf_sweep` must compile in the fences); ν and ρ
    self-document. §7.3, §8 (all leaves), ο.
27. **R28 — π.** End-to-end recipe (release binary, `taskset`, `/usr/bin/time`, best-of-N, baseline
    subtracted, load recorded) on λ's extracted fixture and `smooth_bracket.ri`; stages named as λ runs
    them (no marching cubes; no stage timer exists); t1 restated end-to-end; #8411 is the trigger evaluator.
    §8 π, §11.
28. **R29 — ρ.** Out-of-batch edges #5505 and #5500 (`import std.*` is filtered today, rc=0 silently;
    #5499 is #5500's prerequisite, not the resolver); intra prereqs σ, ο, λ, μ, ν, ξ (ρ migrates every
    bare use); mechanism constrained (real `std.implicit` module + explicit name table); new
    `SdfDeprecatedBareName` W (no builtin deprecation mechanism exists); signal asserts an evaluated value.
    S-2, D-3, §5, §6, §8 ρ.
29. **R30 — τ.** docs/yaml only; the `realize_solid_sdf.rs` doc hunk moves to ι (the false sentence is
    "cannot name reify-kernel-openvdb" — it is a normal dependency; the chord-tolerance limitation stays);
    briefing edits enumerated; τ cites #8411. §8 τ.
30. **R31 — B2.** The printed line is `<Struct>.<cell> = -0.005 m` from a structure let (probe
    `T.z = -0.005 m`). B2.
31. **R32 — Goal block.** Transcript reordered to `cmd_build`'s (provenance, `mesh updates`, `Wrote …
    (N bytes)`, `Triangles: N`); STEP line `error: E_IMPLICIT_NO_BREP: …`; ≈ 1.7 M voxels (164·124·84 with
    D-7 padding); `structure` kept (both forms parse); unit-literal hazard noted. §1, §7.9.
32. **R33 — #6988 coupling.** §6 row for #6988/#8213 (share `realize_solid_sdf.rs`); ι's `FromSolid`
    payload follows what `realize_solid_sdf_at` returns after #6988 or densifies explicitly with the
    budget stated. §5, §6, §7.1, §7.7.
33. **R34 — ι other.** UnifiedDag mirror: explicit exclusion with the D-11 reason (the SYNC REQUIREMENT
    binds only `run_post_processes`'s ladder; `hydrate_value_cell_in_loop` is a static fn with one
    kernel); B13 runs under the default scheduler; non-matching subjects refused at compile time with
    `SdfFromSolidUnresolvableSubject` (the house precedent's silent fall-through, #6007's record); Q8
    DECIDED as `SdfFromSolidPreviewUnavailable` W (Error on every healthy GUI preview otherwise). D-11,
    §7.7, §8 ι, B13, B31, B32, §10 Q8.
34. **R35 — text corrections.** S-2 field-op inventory (`pointwise_max/min` are `.ri` stdlib fns, not
    `FIELD_OP_NAMES`); S-6 per R17; §7.2 "~4 match sites" → every exhaustive match (`FieldSourceKind::`
    appears in ~20 source files); §7.4 mirror list → one descriptor row + two arms (table-driven on main);
    S-11 `GRID_DENSIFY_MAX_VOXELS` is the late C++ backstop; S-1 #6871/#6892 split; μ precedent paths; ω
    may wait on out-of-batch chains. S-1, S-2, S-6, S-11, §7.2, §7.4, §8 ω.
35. **R36 — mass line.** ε ships `smooth_bracket.ri` without `mass`; θ (now after ε) appends it with a
    sanity assertion; the Goal notes it. §1, §8 ε/θ, B11.
36. **R37 — codes left unnamed by the rulings, pinned at manifest assembly.** `QueryKernelUnserved` W
    (γ's DFM/automatic-walk disposition), `ImplicitArgAwaitingSolve` W (ε), `ImplicitNoOpenVdb` (ε, B18),
    `ImplicitFeaOptionUnsupported` (μ's `require_hex_wedge` refusal — no `E_PARAM_NOT_HONORED` exists;
    #7079 may generalise it); θ reuses `ImplicitEmpty`/`ImplicitClipped`; selectors use
    `QueryKernelMismatch`; mnemonic spelling rule added. λ's extracted fixture is named
    `tests/prd-gate/fixtures/implicit_solids_tooldock_gate.ri`. The slot-typing manifest checks are
    route-agnostic because α picks the registration route (I-REG-1). D-9, §8 γ/ε/θ/λ/μ.

37. **R38 — D3 premise-verification findings** (run `wf_cf813704-d57`, 11 CLI-observable leaves, 44 agents,
    BLOCKS with 11/11 probed; the full disposition is in the manifest). Folded: B22 pins the existing
    `<builtin>: <slot> argument expects Length, got Vector3<…>` wording; B21's two arms are observed under
    `reify build` (`reify check` exits 0, as B8); B4, B19 and B31 assert their prefix on stderr, never the
    exit code alone; B13 reads `-0.010 m` in printed SI; λ's extracted fixture omits the old x-pair union (a
    verbatim extraction SIGSEGVs as the whole file does, #7383); ξ's fixture is the 20 mm cube at extent
    21 mm (finite samples end ≈ 11 mm from the centre). §7.10, §8 λ/ξ.
38. **R39 — join critic findings** (fable seat over PRD + manifest + rulings). `sdf_penetration` over an
    empty or clipped `a` returns Undef with cause, never a value (a value plus a Warning reads Satisfied on
    λ's `<= 0mm` gate); every leaf whose Rust test reads a prd-gate fixture registers it in
    `_RUST_COUPLED_RI_FIXTURES` in the same diff; ι adds `sdf_from_solid` to
    `geometry_ops::is_geometry_consumer_call` and λ extends `idler_seat_e2e.rs`'s
    `PRINTER_VOLUME_UNRESOLVED`; λ's second fixture `implicit_solids_tooldock_gate_seeded.ri`; `implicit`
    stays a bare name under ρ; η retires φ's B21 CLI-arm test; δ owns the `tbb::global_control` test hook;
    ο's exemplar respects `best_practices_constraint_gate.rs`; ω lists all 21 leaves; κ checks the PTODO
    ratchet; §7.7 states FromSolid/Sweep frames; #7393 (existing) replaces the placeholder F1 and is
    rescoped to the post-ε residual. D-10, §7.7, §7.10 B17/B29, §8 preamble/δ/ε/η/ι/κ/λ/ο/ρ/ω.

**G7 disposition summary.** All 23 G7 hits are resolved by the rulings above through redesign, except
B18's `error-severity-exits-nonzero` waiver (R20, recorded on ε's row). Already-handled by the saved
design: D-11/B19 (compile-time INV-SF-3), B8's sink placement (never demand-keyed), S-14 provenance
(every `None` path coded), λ's per-gate parity (never exit 0). The vacuous-and-unowned Fidget crate is
owned by #8411.
