# Audit: Structural Stability / Buckling Analysis

**PRD path:** `docs/prds/v0_5/structural-stability-buckling.md`
**Auditor:** audit-structural-stability-buckling
**Date:** 2026-05-12
**Mechanism count:** 14
**Gap count:** 14 (zero WIRED — see "Top concerns" #1)

## Top concerns

- **Nothing in this PRD is implemented yet — every mechanism is FICTION at runtime.** The PRD is explicitly a v0.5 stub deferred behind v0.3 FEA and v0.4 shells. Even the foundations it leans on (`solve_elastic_static`, ComputeNode dispatch, `ElasticMaterial` runtime instantiation) are not yet wired on main. This audit therefore mostly inventories *the additional surface area buckling adds on top of FEA*, not "what was promised vs. shipped" — there is nothing shipped.
- **The eigenvalue solver is the single largest net-new kernel surface.** Lanczos / Arnoldi / shift-invert via faer-rs is named, but `crates/reify-solver-elastic/` has no eigensolver module, faer's eigensolve surface is unused, and no decomposition task tracks this. This is one of the few mechanisms the FEA/shells/multi-load-case PRDs do *not* transitively cover — it is genuinely buckling-specific.
- **Geometric stiffness K_g assembly is a separate kernel pass with no precedent.** The current `reify-solver-elastic` assembles only the elastic stiffness K (`assembly/global.rs`); the stress-dependent geometric stiffness needed for the eigenproblem `(K + λ K_g)φ = 0` is not present anywhere and would need its own element-level kernel for tets/P1/P2 and (more importantly) every shell element class.
- **Cascading dependency on three unshipped foundations (GR-001 + ComputeNode dispatch + shells engine integration).** Materials require GR-001 struct-ctor runtime eval; `solve_buckling` follows the same `@optimized` lowering path as `solve_elastic_static` (whose path-chain is `compute-node-infrastructure` M-014/M-015/M-016, all FICTION); shell-dominated buckling requires shell engine integration (`structural-analysis-shells` T18-T20) to also be live. A single 5-deep dep chain — none of which is buckling's local problem to solve, but all of which gate any progress.
- **Result type contains nested non-trivial parametric Field that the type system explicitly cannot express in `param` position today.** `Mode { mode_shape: Field<Point3, Vector3<Length>> }` inside `List<Mode>` would hit both TODO(field-in-param, task #3117) AND the `List<TraitObject>`-style parametric-instantiation surface (task 2227, done) — but for a nested struct containing a Field, not a bare TraitObject. Unknown whether the existing wiring covers this composition.

## Mechanisms

### M-001: `solve_buckling(body, material, loads, supports, options) -> BucklingResult` stdlib `fn` declaration

- **State:** FICTION
- **Failure mode:** F1 (compile-time contract → no runtime backing)
- **Evidence:** No `fn solve_buckling` in any `.ri` under `crates/reify-compiler/stdlib/`. No grep hit for `solve_buckling` anywhere in the repo. PRD is the sole reference. Sibling `fn solve_elastic_static` is itself FICTION (task 3378 deferred; see `findings/structural-analysis-fea.md` M-001).
- **Blocks:** All downstream buckling mechanisms; transitively all PRD use cases.
- **Note:** Identical shape to the FEA M-001 gap — same surface-absent failure. No task has been filed for the stdlib declaration. Decomposition for this PRD has not been performed; per `docs/architecture-audit/README.md` it is deferred to v0.5+.

### M-002: `@optimized("solver::buckling")` lowering for `fn` context → ComputeNode

- **State:** FICTION
- **Failure mode:** F6 (load-bearing ComputeNode dispatch absent from production code)
- **Evidence:** Same chain as `compute-node-infrastructure` M-014: `eval_user_function_call` does not inspect `optimized_target`; `insert_compute_node` has only test callers; the dispatch registry (M-015) does not exist. The `solver::buckling` target string has zero hits outside of grep prefixes — even fewer references than `solver::modal` (which is at least named in test fixtures `graph.rs:1142,1161` and `compute_cache_key.rs:339`).
- **Blocks:** M-001 surface activation; entire PRD.
- **Note:** Transitive on `compute-node-infrastructure` tasks 3379/3383/3384 (pending). If/when `solve_elastic_static` is wired, `solve_buckling` is a follow-on registration step, not a new mechanism class.

### M-003: `BucklingResult` stdlib structure (List of `Mode`)

- **State:** FICTION
- **Failure mode:** F1 (PRD-declared result type does not exist anywhere)
- **Evidence:** No `structure def BucklingResult` in `crates/reify-compiler/stdlib/`. No grep hits. The PRD's result-type sketch (`ordered list of Mode { eigenvalue, mode_shape: Field<Point3, Vector3<Length>> }`) has not been encoded.
- **Blocks:** Every result-interpretation helper (M-008..M-010).
- **Note:** Sibling-precedent `ElasticResult` (solver_elastic.ri:295) is encoded with `Real` placeholders for Field-typed fields pending TODO(field-in-param, task #3117). The same placeholder strategy would apply here, BUT see M-005 — there is an additional structural complication.

### M-004: `Mode` substructure (`eigenvalue`, `mode_shape`)

- **State:** FICTION
- **Failure mode:** F1 (PRD-declared substructure absent)
- **Evidence:** No `structure def Mode` in the stdlib. No grep hits.
- **Blocks:** M-003.
- **Note:** Eigenvalue is a dimensionless load multiplier (Real); mode_shape is a `Field<Point3<Length>, Vector3<Length>>`. The "Number" type the PRD uses (e.g. `safety_factor_buckling -> Number`) is `Real` in Reify (per the `solver_elastic.ri` encoding-deviations comment, line 35).

### M-005: `List<Mode>` parametric instantiation with nested Field-typed param

- **State:** FICTION
- **Failure mode:** F2 (parametric-instantiation surface partially built; nested-Field composition not exercised anywhere)
- **Evidence:** Task 2227 (`List<TraitObject>` / `Option<TraitObject>` / `Set<TraitObject>` / `Map<K,TraitObject>` call-site conformance) is done per audit-brief §"Things to take as given". But that wires conformance for *TraitObjects*; here the parametric is a *concrete struct* containing a Field-typed field. Whether the current resolver handles `List<Mode>` where `Mode` carries `Field<Point3, Vector3<Length>>` (in `param` position, blocked by TODO field-in-param task #3117) is not verified by any existing test. `crates/reify-compiler/tests/field_compile_tests.rs:362-364` covers `Field<Point3, Scalar> ∘ Field<Scalar, Scalar>` composition only.
- **Blocks:** M-003.
- **Note:** This is the composition risk: List-of-Concrete-Struct works (e.g. `List<LoadCase>` precedent for multi-load-case is the same shape pattern), and Field-in-param is its own known TODO — but the *combination* is novel. Likely OK if the same Real-placeholder workaround used by `ElasticResult` is applied to `Mode.mode_shape`, but unverified.

### M-006: Eigenvalue solver (Lanczos / Arnoldi / shift-invert)

- **State:** FICTION
- **Failure mode:** F6 (kernel surface that PRD leans on does not exist anywhere)
- **Evidence:** No "Lanczos" / "Arnoldi" / "shift_invert" / "eigensolve" / "generalized_eigenvalue" mention in `crates/`. The only "eigenvalue"-adjacent code is `compute_eigenvalues_3x3` (referenced in `fea_multi_case.ri:185` for per-grid-point 3×3 stress-tensor principal-stress extraction) and `eigenvalues()` Vector builtin (`reify-lsp/src/completion.rs:530-531`, `examples/linalg.ri`) — both for small dense matrices, not large sparse generalized eigenproblems. faer-rs is in use for the CG linear solver (task 2919 done) and dense linear algebra; its sparse eigensolve / Lanczos surface is unused.
- **Blocks:** Entire PRD — no eigenvalue solver, no buckling.
- **Note:** The largest net-new kernel surface in this PRD. faer-rs *does* have a sparse eigensolve story (it has been growing one), but adoption requires its own task. Also: PRD notes "Geometric multiplicity handling for symmetric structures" — symmetric Lanczos with deflation / block-Lanczos is harder than the basic variant, so the implementation surface depends on whether degenerate modes are supported in v1.

### M-007: Geometric stiffness matrix `K_g` assembly

- **State:** FICTION
- **Failure mode:** F6 (separate kernel pass with no precedent; PRD leans on it but it does not exist)
- **Evidence:** `crates/reify-solver-elastic/src/assembly/global.rs` and `assembly/` contains only elastic-stiffness K assembly (P1/P2 tet, plus shell K via `shell_assembly.rs`). No `K_g`, `geometric_stiffness`, `K_geom`, `stress_stiffness`, or similar in any element kernel. PRD §"Sketch of approach" describes it as "Internally runs a linear-static solve to compute pre-stress, assembles K_g, then eigenvalue-solves" — but the K_g assembly step has no decomposition task.
- **Blocks:** Entire PRD.
- **Note:** This requires its own per-element formulation for every element kind in scope: P1 tet, P2 tet, MITC3+ shell, and (if hex/wedge meshing is also live for buckling) hex/wedge. Each is a separate kernel — substantially more code than the eigenvalue solver itself. Strongly coupled to shell engine integration: the PRD says shells are the *dominant* use case, so shell-element K_g must be present in v1.

### M-008: `critical_load(result) -> Force` result-interpretation helper

- **State:** FICTION
- **Failure mode:** F1 (PRD-declared helper does not exist)
- **Evidence:** No `critical_load` mention in `crates/` or stdlib. No grep hits. The PRD's "Critical load = lowest eigenvalue × reference load magnitude" formulation requires reaching back into the input load magnitudes; this composition is not encoded anywhere.
- **Blocks:** M-001 user surface.
- **Note:** Conceptually trivial given M-003/M-006 present — multiply eigenvalue (Real) by stored reference-load magnitude (Force). The interesting design question is what counts as the "reference load magnitude" when loads is `List<Load>` with heterogeneous types (point loads, pressure loads, body forces) — addressed in §"Open design questions" but unresolved.

### M-009: `mode_shape(result, n) -> Field<...>` result-interpretation helper

- **State:** FICTION
- **Failure mode:** F1 (PRD-declared helper absent)
- **Evidence:** No `mode_shape` grep hits in `crates/` outside the PRD itself.
- **Blocks:** M-001 user surface; M-013 GUI rendering.
- **Note:** Indexing a `List<Mode>` by integer n and reading its `mode_shape` field — depends on `BucklingResult.modes[n].mode_shape` access pattern working with the Real-placeholder approach (M-005).

### M-010: `safety_factor_buckling(result, applied_load) -> Real` helper

- **State:** FICTION
- **Failure mode:** F1 (PRD-declared helper absent)
- **Evidence:** No `safety_factor_buckling` grep hits anywhere.
- **Blocks:** M-001 user surface.
- **Note:** Per §"Open design questions" → "Reference load magnitude", the design lean is to define eigenvalue *as* the safety factor directly (when reference load == applied load). If that decision lands, this helper might collapse to `critical_load(result) / magnitude(applied_load)` or simply `result.modes[0].eigenvalue` — but neither path is encoded.

### M-011: Material extension (reuses `ElasticMaterial`)

- **State:** FICTION
- **Failure mode:** F1 (GR-001 transitively — `ElasticMaterial` struct constructors do not evaluate at runtime)
- **Evidence:** GR-001 (cite-and-move-on per audit-brief §"Things to take as given"). PRD says "no extension needed — uses the same `ElasticMaterial`", which would be true *if* `ElasticMaterial` had runtime instantiation; today `Steel_AISI_1045()` → `Value::Undef`.
- **Blocks:** M-001.
- **Note:** Not a buckling-specific gap, but recorded because the PRD explicitly leans on `ElasticMaterial`. If GR-001 lands, this row collapses to WIRED automatically.

### M-012: Load / Support inputs (reuses FEA loads/supports machinery)

- **State:** PARTIAL
- **Failure mode:** F1 (PRD describes `List<Load>` / `List<Support>` as if statically typed; reality is kind-tagged Maps from builtin constructors)
- **Evidence:** `fea_multi_case.ri:19-27` documents the drift explicitly: "PRD writes `loads : List<Load>` and `supports : List<Support>`. `Load` and `Support` are not statically-typed structs in the current stdlib; they are kind-tagged Maps produced by `point_load`, `pressure_load`, `fixed_support`, ... TODO(load-trait)". Same drift propagates here — the buckling PRD signature uses identical wording. `crates/reify-solver-elastic/src/lib.rs:340` exports `apply_body_force, apply_dirichlet_row_elimination, apply_point_load, apply_traction_load`. Runtime ctors `point_load(...)`, `FixedSupport(...)` work today but with the inconsistent snake_case/PascalCase noted in audit-brief §"Things to take as given".
- **Blocks:** M-001 contract clarity.
- **Note:** Not a fresh gap — inherited from FEA/multi-load-case. PRD did not re-acknowledge the drift; whoever decomposes this PRD must.

### M-013: GUI rendering of mode shapes (animation via phase sweep)

- **State:** FICTION
- **Failure mode:** F1 (no GUI surface for animated mode-shape rendering exists)
- **Evidence:** `findings/fea-gui-rendering.md` covers static-deformation rendering (deformed-shape view per multi-load-case `findings/multi-load-case-fea.md`); no precedent in either for time-varying / phase-swept rendering. Search for "animate", "phase", "mode_shape" in the GUI's React/Three.js side finds no mode-shape animation precedent. The "displacement field × eigenvalue scaling sweep a phase parameter" workflow has no existing analog.
- **Blocks:** PRD's dominant visualization need (per §"Sketch of approach" final bullet).
- **Note:** Cross-PRD breadcrumb: would compose with `fea-gui-rendering-shells.md` (v0.4) and `fea-gui-rendering.md` (v0.3) but adds time-domain animation as a new dimension. No existing PRD owns this rendering primitive.

### M-014: Imperfection sensitivity / mode-scaled geometry re-analysis

- **State:** FICTION
- **Failure mode:** F1 (PRD §"Open design questions" raises it as a stdlib helper but no implementation; even the helper signature is unresolved)
- **Evidence:** PRD §"Open design questions" → "Imperfection sensitivity": "Standard treatment: scale the first mode shape into the geometry at small amplitude and re-analyze. Worth providing as a stdlib helper but adds workflow complexity." No `scale_imperfection`, `seed_imperfection`, or similar in stdlib. No decomposition task.
- **Blocks:** Real-world shell-buckling accuracy (per PRD: "Real shells buckle far below the linear-buckling eigenvalue because of geometric imperfections").
- **Note:** This is the gap between "linear eigenvalue analysis answer" and "engineering-trustworthy answer for shells". PRD acknowledges it but leaves it as open design. Closely related to mesh-morphing — applying a mode-shape × small-amplitude perturbation to nodal positions is a special case of mesh morphing, which has its own PRD (`docs/prds/v0_3/mesh-morphing.md`) with substantial gaps of its own (per `findings/mesh-morphing.md`).

## Cross-PRD breadcrumbs

- **`structural-analysis-fea.md` (v0.3)** — Direct foundation. M-001..M-011 chain assumes `solve_elastic_static` is wired and the eigenvalue extension is purely additive. FEA's own M-001/M-002 are FICTION (per `findings/structural-analysis-fea.md`); buckling cannot begin until FEA lands and validates. The Material starter library + Load/Support drift are inherited unchanged.
- **`structural-analysis-shells.md` (v0.4)** — Critical foundation. PRD §"Pre-conditions for activating" explicitly states shells must ship first because "slender structures are usually thin enough to be shell-modeled. Buckling on a tet-only foundation would underserve the typical use case." Shell-element K_g assembly (M-007) requires the MITC3+ kinematics already in `shell_kinematics.rs` / `shell_assembly.rs` but extends them with stress-stiffness terms not present.
- **`multi-load-case-fea.md` (v0.3.x)** — PRD §"Relationship to other PRDs" calls out "buckling load factor per load case is a natural envelope". Composition mechanism unspecified; presumably a `MultiCaseBucklingResult` parallel to `MultiCaseResult` — would compound the parametric-instantiation surface (M-005) by one more level.
- **`fea-gui-rendering.md` + `fea-gui-rendering-shells.md`** — Mode-shape rendering (M-013) needs the deformed-shape rendering pipeline these PRDs build, plus a new animation primitive on top.
- **`compute-node-infrastructure.md` (v0.3)** — `solve_buckling`'s `@optimized` dispatch follows the same M-014/M-015/M-016 chain (all FICTION per `findings/compute-node-infrastructure.md`). Cache-key composition (M-004 there) is reusable as-is; significance-filter allowlist (M-010 there, hardcoded "solver::elastic_static") would need to expand to include "solver::buckling".
- **`structural-analysis-modal.md`** (mentioned in §"Relationship to other PRDs" but not yet filed) — Eigenvalue solver (M-006) would be shared infrastructure with modal analysis. Decoupling argument: if Lanczos is built carefully it serves both `(K + λK_g)φ = 0` (buckling) and `(K - ω²M)φ = 0` (modal) with mass-matrix M instead of K_g. PRD does not call out this generalization explicitly.
- **`mesh-morphing.md` (v0.3)** — Imperfection-seeding (M-014) is a special case of nodal-position morphing. Cross-pollination possible but not designed.
- **GR-001** — Material runtime instantiation (M-011) and any structure-ctor inputs (e.g. options) transitively blocked. Standard chain; cited and moved on.

## Notes on audit scope

This PRD is a deferred v0.5+ stub with no decomposition tasks filed. There is therefore nothing to compare "PRD-promised" vs "task-tracked" vs "code-shipped" — all three columns are empty. The audit value here is in (a) enumerating the *additional* surface area buckling adds beyond FEA + shells + multi-load-case (M-006 eigensolver, M-007 K_g assembly, M-013 mode-shape animation, M-014 imperfection seeding — the four non-trivially-buckling-specific gaps), and (b) flagging the M-005 nested-parametric composition as a type-system question worth checking before someone decomposes the PRD.
