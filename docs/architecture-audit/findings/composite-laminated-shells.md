# Audit: Composite / Laminated Shell Elements

**PRD path:** `docs/prds/v0_5/composite-laminated-shells.md`
**Auditor:** audit-composite-laminated-shells
**Date:** 2026-05-12
**Mechanism count:** 14
**Gap count:** 14

## Top concerns

- **Every named runtime entity in the PRD is fiction.** No `OrthotropicMaterial`, `Laminate`, `Ply`, `tsai_wu`, `hashin`, or `max_strain` symbol exists anywhere in the codebase (`crates/`, `stdlib/*.ri`, or PRDs). The PRD is a green-field design, with no scaffolding yet — but it lands on top of the already-broken structure-constructor evaluation (GR-001), the unresolved `Field<X,Y>` param-position issue (TODO #3117), and a not-yet-shipped parent shells PRD.
- **Foundation is explicitly absent.** Parent v0.4 `structural-analysis-shells.md` is "design resolved + decomposed (2026-05-05), deferred"; downstream tasks (Shells T5/T6 in fused-memory) are partially done but the kernel is **constant-thickness, isotropic D-matrix only** (`shell_assembly.rs:10-11`, `:201`, task 3014 observation memory). Composite swap-in requires re-architecting the through-thickness integration loop, the D-matrix construction, and `ElasticResult.stress`'s `top/mid/bottom` shape (which currently models surface fibre only, not per-ply).
- **No decomposition tasks exist.** Unlike sibling v0.5 stubs that have decomposition tasks queued under their PRDs, this PRD is purely a stub (`Status: stub — deferred, candidate v0.5+`). No tasks own any of the proposed mechanisms, which is appropriate for a stub but means everything is `FICTION` rather than `TODO`.
- **Layup syntax open question collides with `List<Struct>` call-site conformance.** The proposed `Laminate { plies : List<Ply> }` shape needs either (a) struct-constructor runtime eval (GR-001) plus `List<Ply>` flowing through `Value::List` of struct-instance Maps, or (b) an alternate constructor design. The PRD names it as open, and task 2227 (`List<TraitObject>` wiring) is done — but `List<Struct>` of a concrete (non-trait) struct in param position has not been confirmed wired in any audit-relevant memory.

## Mechanisms

### M-001: `OrthotropicMaterial` stdlib structure with `E1, E2, G12, ν12, density, X_T, X_C, Y_T, Y_C, S` cells

- **State:** FICTION
- **Failure mode:** F1 (compile-time contract; no code)
- **Evidence:** No grep hit for `Orthotropic` or `OrthotropicMaterial` in `crates/`, `stdlib/*.ri`, or `docs/prds/` (except this PRD). Existing materials stack has `Material` (`materials_mechanical.ri:63`), `ElasticMaterial` trait (`materials_fea.ri:88`), and four isotropic concrete structures (`Steel_AISI_1045`, `Aluminium_6061_T6`, `Titanium_Ti6Al4V`, `ABS_Plastic` in `materials_fea.ri:132-249`). All four are isotropic-only (carry `youngs_modulus`, `poisson_ratio`, `density`, `yield_stress` — no directional moduli or ply allowables).
- **Blocks:** Tasks gated on this PRD activation (none currently queued).
- **Note:** Type would be a new structure-def with 10 cells; co-blocks with GR-001 (struct-constructor eval) and the open question of whether `ElasticMaterial` trait covers orthotropic or a new `OrthotropicElasticMaterial` trait is needed (the trait's surface in `materials_fea.ri:88` was designed for isotropic).

### M-002: Orthotropic constitutive law trait surface (per-direction moduli, ply allowables)

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** `ElasticMaterial` trait (`materials_fea.ri:88`) requires `youngs_modulus, poisson_ratio, density, yield_stress` only — fundamentally isotropic. `IsotropicElastic` Rust struct in `crates/reify-solver-elastic/src/constitutive.rs:9-93` builds the 6×6 D matrix from scalar `E, ν`; no `MaterialConstitutiveLaw` trait abstracts over isotropic vs orthotropic.
- **Blocks:** All downstream composite mechanisms.
- **Note:** This is the conceptual fork that determines whether orthotropic is a sibling structure to `Steel_AISI_1045` (separate trait) or a parameterised member of a polymorphic constitutive-law surface. Open design.

### M-003: Per-ply orthotropic D-matrix construction (6×6 in material frame, rotated to laminate frame)

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** `IsotropicElastic::d_matrix() -> [[f64; 6]; 6]` (`constitutive.rs:88`) is the only D-matrix builder. No rotation by fibre orientation; no orthotropic 6×6 stiffness routine.
- **Blocks:** M-004, M-005 (through-thickness sum needs per-ply D).
- **Note:** Classical lamination theory; well-known maths but a new code path.

### M-004: `Laminate` stdlib structure with `plies : List<Ply>` ordered stack

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No `Laminate`, `Ply`, or stdlib `List<<StructureName>>` of concrete (non-trait) structs in `materials_fea.ri` or `solver_elastic.ri`. Closest precedent: `fea_multi_case.ri:50` uses `List<LoadCase>` as a list of structures, but typed as `List<Real>` placeholder per the `Field<X,Y>`-in-param TODO (#3117); kind-match silently accepts the runtime list. Whether the same placeholder-list mechanism transfers to `List<Ply>` is unverified by any audit memory.
- **Blocks:** M-005 (kernel iterates the ply list), M-008 (helper functions).
- **Note:** Coupled to GR-001 (struct-ctor eval) and the open design question of constructor surface (list-literal vs dedicated ctor vs external file).

### M-005: Through-thickness sum-over-plies integration in shell element kernel

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** `crates/reify-solver-elastic/src/shell_assembly.rs:10-11` explicitly: "Reissner-Mindlin shell element under a **constant-thickness isotropic** linear-elastic constitutive law. Through-thickness integration is..." (analytical, single material). `:118` describes it as "Baked in as a private constant — it is a property of the through-thickness". Task 3014 ("Shells T6: shell stiffness assembly under isotropic linear-elastic constitutive law") confirms "Constant-thickness, isotropic D matrix. Through-thickness integration analytical (closed form for membrane + bending + transverse shear contributions)."
- **Blocks:** M-006, M-007.
- **Note:** The PRD says "the through-thickness integration becomes a sum over plies with discontinuous derivatives at ply boundaries" — this is a structural rewrite of the shell stiffness assembly path, not an additive extension.

### M-006: Per-Gauss-point layered constitutive evaluation

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** Shell kinematics module (`shell_kinematics.rs:44`) returns kinematic primitives only — no per-Gauss-point material evaluation hook; D matrix is computed once at element scope from the single material. No infrastructure for "compute D per Gauss point as a layered stack rather than a single isotropic relation."
- **Blocks:** M-005.
- **Note:** New code path; would need either a per-Gauss-point material callback or an unrolled per-ply integration scheme.

### M-007: Per-ply stress and strain result fields in `ElasticResult`

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** `ElasticResult` in `stdlib/solver_elastic.ri:295-316` has `displacement, stress, frame, max_von_mises, converged, iterations` only. `ShellStress` (`:352-356`) has `top, mid, bottom` — a 3-channel through-thickness shape designed for **single-material** outer/neutral/inner fibres, NOT per-ply (the comment at `:343-345` is explicit: "preserves the invariant that ShellStress always has all three channels populated even for solid-element results"). No precedent for `List<Field<...>>` or per-ply indexed field collections.
- **Blocks:** All composite-result consumers (GUI, multi-load-case envelopes).
- **Note:** PRD says "top, mid, bottom of each ply" — a 3 × N_plies result tensor, which has no analogue in the current result-data shape. Coupled to the `Field<X,Y>` in param position TODO (#3117) — every existing field-typed slot in `ElasticResult/ShellStress` is `Real` placeholder.

### M-008: `tsai_wu(...)` stdlib failure-criterion function

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No `tsai_wu` grep hit anywhere in repo. Closest precedent: `von_mises_stress` field on `AnalysisResult` in `stdlib/analysis.ri:30,36` (a scalar field, not a function). No stdlib function precedent for "stress × allowables → failure index field" mapping.
- **Blocks:** M-011 (failure-index result field).
- **Note:** Requires both M-001 (allowables in `OrthotropicMaterial`) and M-007 (per-ply stress fields) to be wired before this function has well-defined inputs.

### M-009: `hashin(...)` stdlib failure-criterion function

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No `hashin` grep hit. Same shape as M-008.
- **Blocks:** M-011.
- **Note:** Hashin distinguishes fibre-tension/fibre-compression/matrix-tension/matrix-compression modes — output cardinality higher than scalar Tsai-Wu.

### M-010: `max_strain(...)` stdlib failure-criterion function

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No `max_strain` grep hit. Same shape as M-008.
- **Blocks:** M-011.

### M-011: Per-failure-criterion failure-index field in `ElasticResult`

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No `failure_index` grep hit. PRD says "plus failure-index field per failure criterion." `ElasticResult` (`solver_elastic.ri:295`) does not declare any failure-index cell; `Field<X,Y>` in param position TODO (#3117) still gates field-typed result cells.
- **Blocks:** GUI composite-result rendering (not yet PRD'd).
- **Note:** Cardinality grows with criterion count × ply count — UX/data-shape open question.

### M-012: Inter-laminar shear stress recovery (equilibrium post-processing)

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No post-processing equilibrium-recovery pass in `crates/reify-solver-elastic/` (only `error_estimator.rs` and direct stress evaluation `shell_result.rs`). PRD acknowledges "Standard but not free in implementation."
- **Blocks:** Practical composite analysis (delamination is the dominant failure mode per PRD).
- **Note:** PRD-flagged open issue; mentioned but neither task nor code stub exists.

### M-013: Layup helpers (symmetric, balanced, quasi-isotropic constructors)

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No grep hit. PRD: "Helpers for symmetric, balanced, and quasi-isotropic layups."
- **Blocks:** Convenience layer; not load-bearing.
- **Note:** Sugar around M-004; whether stdlib fn or constructor variants is open.

### M-014: Tabular layup import helper (external JSON/TOML/spreadsheet)

- **State:** FICTION
- **Failure mode:** F1
- **Evidence:** No `ImportHelper`, `read_toml`, `import_csv`, `json_load` grep hit in `crates/reify-compiler/stdlib/` or `crates/reify-eval/`. Adjacent infrastructure: `field_import_provenance.rs` for VDB/CSV ingestion, but that targets `Field<X,Y>` not structure-of-structs literal data. PRD calls this an open design question ("lean: import helper for tabular cases").
- **Blocks:** Not load-bearing; deferred-of-deferred.
- **Note:** Cross-cuts a broader open question about whether Reify gains a generic stdlib-data-from-file mechanism.

## Cross-PRD breadcrumbs

- **`structural-analysis-shells.md` (v0.4)** — this PRD's hard prerequisite. Mid-surface extraction, MITC3+ kinematics, `ShellStress` shape, `@shell` annotation all live there. Status per parent PRD: "design resolved + decomposed (2026-05-05) — deferred."
- **`structural-analysis-fea.md` (v0.3)** — gates the entire FEA stack including `ElasticResult`, `ElasticOptions`, solver loop. Composite extends `ElasticResult` shape.
- **`multi-load-case-fea.md` (v0.3.x)** — PRD says "composes with multi-load-case" for per-load-case envelopes. Envelope helpers (`envelope_von_mises`, `linear_combine`) in `fea_multi_case.ri` are scalar/single-stress-field — extending to per-ply, per-criterion envelopes is an additional cross-cut.
- **`fea-gui-rendering-shells.md` (v0.4)** — PRD says "composes with" for per-ply visualisation; sibling PRD is itself deferred.
- **`structural-analysis-progressive-damage.md`** — PRD seeds this hypothetical follow-on; not filed.
- **GR-001 (structure-constructor runtime eval)** — every proposed stdlib structure (`OrthotropicMaterial`, `Laminate`, `Ply`, instances of starter library like `T300_5208`) hits this gap. No mechanism in this PRD is unblocked by GR-001's resolution alone, but every mechanism is blocked by it.
- **TODO(field-in-param, task #3117)** — per-ply stress/strain/failure-index result fields all need `Field<X,Y>` in param position, same as existing `ElasticResult.stress/frame/displacement`.
- **Task 2227 (`List<TraitObject>` call-site conformance)** — done; partially relevant if `List<Ply>` is typed as `List<TraitObject>`-of-`Ply`. If `Ply` is a concrete struct (not a trait object), the call-site conformance check for `List<<ConcreteStruct>>` in param position is not confirmed wired by audit memory.
