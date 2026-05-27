# Audit: Shell Elements for Thin-Body Structural Analysis

**PRD path:** `docs/prds/v0_4/structural-analysis-shells.md`
**Auditor:** audit-structural-analysis-shells
**Date:** 2026-05-12
**Mechanism count:** 25
**Gap count:** 16
**WIRED:** 9 — **PARTIAL:** 9 — **TODO:** 5 — **FICTION:** 1 — **DRIFT:** 1 — **ORPHAN:** 0

## Top concerns

- **MITC3 / MITC3+ DRIFT is the headline architectural divergence.** The PRD picks MITC3+ specifically to avoid the "silent inaccuracy in the L/t=5–20 marginal-thickness regime" of plain-MITC3. What shipped is **bare MITC3 on flat-facet triangles**, with a divergence-theorem proof (task 3349) that the cubic-bubble enrichment is mathematically inert for flat-facet elements. Benchmarks (task 3034) show **21–2200× under-prediction** vs published MacNeal-Harder references on the three curved-shell tests (pinched cylinder, Scordelis-Lo roof, hemisphere); test bands were widened to make them pass. The "credibility-killing bug class" the PRD explicitly chose MITC3+ to avoid is now present. Curved-element formulation (the actual MITC3+ path) is filed as task 3392 (pending, low priority).
- **The entire user-visible runtime entry point is missing.** Auto-classification dispatch (T18 / task 3031), shell extraction failure handling (T19 / task 3032), mixed-region partitioning (T12 / task 3023), mixed-region validation (T22 / task 3035), and the end-to-end example (T23 / task 3036) are all `pending`. `reify-shell-extract` is in the workspace but **no other crate depends on it** — extraction is callable in isolation but not wired into any solve path. T18 is gated on task 2924 (FEA engine integration) which is in turn gated on task 3426 (pending under GR-001) — so the entire shell stack inherits the same runtime-evaluation gap.
- **OpenVDB Voxel ReprKind is half-realized.** PRD's hard precondition is "OpenVDB Voxel ReprKind realizable from B-rep via the dispatcher's conversion chain." Reality: `realize_voxel_from_mesh` exists in `reify-kernel-openvdb` (FFI works behind `cfg(has_openvdb)`), but `BRep→Voxel sampling: deferred to a separate follow-up` per `register.rs:33`, and `(Convert, Voxel)` is excluded from the capability descriptor (`register.rs:30-32`). `reify-shell-extract` is currently tested against synthetic `SampledField`s, not real OpenVDB grids — a tactical workaround that masks the precondition gap.
- **Stress-frame helper `to_global(stress, frame)` is FICTION.** Solver-side `shell_element_frame()` produces the rotation matrix; both stdlib doc (`solver_elastic.ri:276-280`) and `shell_result.rs:39` reference a `to_global(stress, frame)` helper that does not exist anywhere — `grep -r '"to_global"|fn to_global'` returns zero hits in stdlib/eval/compiler. Same shape of comment-only mechanism as the GR-001 family.

## Mechanisms

### M-001: voxel-medial mask algorithm (T1, task 3008)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-shell-extract/src/medial.rs` (`compute_medial_mask`, exported via `lib.rs:148`); task 3008 merged b3a4889f5a; covered by smoke test in `lib.rs:106-137` and unit tests within the module.
- **Note:** Operates on synthetic `SampledField` input, not yet a real OpenVDB grid handle. The crate-level doc (`lib.rs:21-34`) explicitly admits this skeleton-crate posture: ship the algorithm against synthetic inputs, wire real producers in a follow-up.

### M-002: mid-surface mesh extraction from medial mask (T2, task 3009)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-shell-extract/src/mid_surface.rs` (`extract_mid_surface`); task 3009 done with `files = ["crates/reify-shell-extract/src/mid_surface.rs", "crates/reify-shell-extract/src/lib.rs"]`. Binary marching-cubes on the sparse mask grid; per-vertex thickness sampled from SDF.
- **Note:** Vertex deduplication intentionally deferred to T9 mesher per the doc comment at `mid_surface.rs:31-33`.

### M-003: spurious-branch pruning (T3, task 3010)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-shell-extract/src/pruning.rs`; task 3010 merged 3051dc5eb7. Empirical default `shell_branch_prune_ratio = 1.0`, `max_prune_iterations = 8`.
- **Note:** PRD lists "empirical default TBD" as an open question (line 89); ship-time picked default lives in code, not yet validated against real geometry.

### M-004: per-region segmentation classifier (T4, task 3012)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-shell-extract/src/segmentation.rs` (`segment_regions`, `RegionClassification` enum, `RegionInfo`); task 3012 merged e8d680d8b4. Default `shell_threshold = 0.2` matches PRD ElasticOptions default at line 1272-1275.
- **Note:** Operates on `MedialMask` + `MidSurfaceMesh` inputs; not yet driven by a real geometry-realization input.

### M-005: MITC3+ element formulation (T5, task 3013) — **MITC3, not MITC3+**

- **State:** DRIFT
- **Failure mode:** F1 (PRD specifies an element family the implementation cannot deliver on the shipped geometry representation)
- **Evidence:** `crates/reify-solver-elastic/src/shell_assembly.rs:25-43` ("Why this is MITC3, not MITC3+") — divergence-theorem proof that `K_NB ≡ 0` for flat-facet triangles; empirical confirmation `2.411163e-7 vs 1.8248e-5` (76× under-prediction on pinched cylinder); task 3349 done with `done_provenance.note` admitting Option A scope reduction; `mitc3_plus.rs` filename retained but is bare MITC3. Curved-element MITC3+ filed as task 3392 (pending, low priority).
- **Blocks:** 3034 benchmarks (passed only after band-widening per esc-3034-165), 3325 cancelled, real shell credibility on curved geometry.
- **Note:** PRD's design-rationale paragraph at line 58 (lines starting "(a) MITC3+ valid range...(b) DKT's silent inaccuracy...") was the explicit reason for picking MITC3+. The shipped element falls into the exact failure mode the PRD wanted to avoid.

### M-006: shell stiffness assembly under isotropic linear-elastic constitutive law (T6, task 3014)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-solver-elastic/src/shell_assembly.rs` (`shell_element_stiffness`, `build_shell_frame`, `plane_stress_d`); task 3014 merged 0849e5df78. Through-thickness analytical integration; local-to-global per-element using mid-surface frame.
- **Note:** Underlying element is bare MITC3 — see M-005.

### M-007: shell stress recovery (top/mid/bottom local-frame + frame field) (T7, task 3016)

- **State:** PARTIAL
- **Failure mode:** F6 (data structure exists; runtime population path not yet driven)
- **Evidence:** `crates/reify-solver-elastic/src/shell_result.rs:1-13` — "This file ships the data-only contract (define the shape + tet constructor); engine-integration tasks T18-T20 are responsible for actually populating these fields from the MITC3 kernel"; task 3016 merged 1a925afbf4. `shell_element_frame()` defined.
- **Blocks:** T18 (3031), T22 (3035).
- **Note:** Data shape ready; producer untestable until T11 mixed assembly drives a real shell solve through a higher-level path.

### M-008: shell BC application (FixedSupport auto-clamp + PinnedSupport opt-out) (T8, task 3017)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-solver-elastic/src/shell_boundary.rs` (`build_support_bcs`, `SupportKind`, `SupportBodyKind`, `SupportCompatibility` for the `Pinned-on-Tet` diagnostic case); task 3017 merged c2fdf546ec. Stdlib `PinnedSupport` / `FixedSupport` builtin constructors live in `crates/reify-stdlib/src/supports.rs:93+`.
- **Note:** Solver-internal wiring complete; PRD's "auto-clamp on FixedSupport on a shell entity" is implemented as `(SupportKind, SupportBodyKind)` cross-product at the BC builder layer — caller responsible for tagging the right body kind. End-to-end "user writes `FixedSupport(face)` on a shell-classified body and rotations auto-clamp" needs T18 dispatch to wire it.

### M-009: shell mid-surface mesher (triangulate + quality + remesh) (T9, task 3019)

- **State:** PARTIAL
- **Failure mode:** F4 (Gmsh-2D / MMG2D bindings absent from workspace)
- **Evidence:** `crates/reify-shell-extract/src/mesher.rs:1-26` — "Neither Gmsh nor MMG2D has Rust FFI bindings in this workspace today.... ships a pure-Rust algorithm" (vertex dedup + quality gating). PRD §111 explicitly specifies "Default Gmsh 2D from extractor mesh" + "MMG2D-style remeshing on quality failure". Laplacian smoothing path explicitly stubbed: "Callers may set `max_remesh_iterations > 0` but will receive the same `QualityBelowThreshold` error". Task 3019 merged 36d03e3c06.
- **Note:** Quality gate produces a structured diagnostic; remediation pathway absent.

### M-010: MPC tying mechanism (shell rotation ↔ tet displacement gradient) (T10, task 3020)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-solver-elastic/src/mpc.rs` (`MpcRow`, `MpcRow::shell_tet_tying`, `apply_mpc_row_elimination`); task 3020 merged a2a5cd07d6. Three through-thickness tying points; row-elimination reuses Dirichlet plumbing per PRD.
- **Note:** Solver-internal; not yet driven by a higher-level mixed-region body assembler (T12 pending).

### M-011: mixed-element global assembly (tet + shell + tying) (T11, task 3021)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-solver-elastic/src/assembly/global.rs` — D=6 mixed-element global K with orphan-tet-rotation handling; `mixed_tet_and_shell_share_node_assembles_into_unified_6dof_per_node_global_k` test at `global.rs:1416`; task 3021 merged 7ea04b6e4a.
- **Note:** Tested with hand-built mixed fixtures, not driven by extractor output.

### M-012: mixed-region body partitioning (T12, task 3023)

- **State:** TODO
- **Failure mode:** F6 (downstream wiring task; all upstream pieces exist)
- **Evidence:** Task 3023 pending; `metadata.files = ["crates/reify-eval/src/engine_build.rs", "crates/reify-shell-extract/src/lib.rs"]` — neither file currently contains shell-partitioning code. The `engine_build.rs` doesn't import `reify-shell-extract`.
- **Blocks:** T18 (3031), T22 (3035), T23 (3036).
- **Note:** The "T4 segmenter output → mesh tet regions with Gmsh, mesh shell regions with mid-surface mesher, wire MPCs" assembly does not exist as a code path.

### M-013: `@shell(thickness = ...)` annotation (T13, task 3024)

- **State:** PARTIAL
- **Failure mode:** F1 (parser side wired; no downstream consumer)
- **Evidence:** `crates/reify-compiler/src/annotations.rs:66-205` (parse + context validation; bare `@shell` accepted; one-arg numeric thickness accepted); task 3024 merged 86188b918c. `grep -rn '@shell' crates/reify-eval crates/reify-solver-elastic` returns zero hits in `src/` — no engine consumer.
- **Blocks:** T18 (3031).
- **Note:** Same shape as the structure-ctor gap: declarative surface wired, runtime consumer absent.

### M-014: `@solid` annotation (T14, task 3025)

- **State:** PARTIAL
- **Failure mode:** F1 (parser side wired; no downstream consumer)
- **Evidence:** `crates/reify-compiler/src/annotations.rs:194-203` (bare `@solid` only, "force-tet is unconditional"); task 3025 merged 7926c5b168. No engine consumer.
- **Blocks:** T18 (3031).
- **Note:** Symmetric to M-013.

### M-015: `PinnedSupport` stdlib + shell-aware `FixedSupport` extension (T15, task 3027)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-stdlib/src/supports.rs:93-110` ("FixedSupport" / "PinnedSupport" builtin dispatch); `shell_boundary.rs` consumes them; task 3027 merged c515e0544600.
- **Note:** Both ship via the PascalCase builtin-constructor path (Map-producing), not the `structure def` runtime-ctor path that GR-001 blocks. Naming-inconsistency cited in audit-brief "Things to take as given" (snake_case for loads, PascalCase for supports) applies here.

### M-016: `ElasticResult` structured stress (top/mid/bottom + frame) + backward-compat alias (T16, task 3028)

- **State:** PARTIAL
- **Failure mode:** F1 (GR-001 transitive — `structure def ElasticResult` and `structure def ShellStress` not runtime-instantiable; backward-compat alias `result.stress = result.stress.mid` not implemented anywhere)
- **Evidence:** `crates/reify-compiler/stdlib/solver_elastic.ri:295-316` (`structure def ElasticResult`), `:318+` (`structure def ShellStress`); `crates/reify-solver-elastic/src/shell_result.rs` (Rust data container); task 3028 merged 90e8092f37. `grep -rn 'result\.stress\.mid' crates/` returns only commentary, no code-path; GR-001 transitive on ElasticResult / ShellStress runtime instantiation. The `result.stress` alias to `result.stress.mid` is documented (`solver_elastic.ri:325-328`) as "engine-integration tasks T18-T20 are responsible" — currently fiction.
- **Blocks:** T18 (3031), T22 (3035).
- **Note:** "Backward-compatible alias" was PRD-promised; concretely it's documented intent only.

### M-017: `ElasticOptions` shell extension (shell_threshold, shell_voxel_size, shell_branch_prune_ratio, shell_force) (T17, task 3030)

- **State:** PARTIAL
- **Failure mode:** F1 (GR-001 transitive — `structure def ElasticOptions` not runtime-instantiable; defaults declared in stdlib but consumer reads them via Map-shape Value once the runtime ctor is real)
- **Evidence:** `crates/reify-compiler/stdlib/solver_elastic.ri:149-220` (full ElasticOptions structure_def with shell fields, range constraints); task 3030 merged 6645b90dc5. Consumer side: `crates/reify-shell-extract/src/segmentation.rs:74` reads `SegmentationOptions.shell_threshold` (Rust-side), `pruning.rs:56` reads `PruneOptions.shell_branch_prune_ratio` (Rust-side). No bridge between the stdlib structure_def and the Rust struct fields.
- **Blocks:** T18 (3031).
- **Note:** `shell_force : ShellForce` is an enum-typed field; the `ShellForce` enum (`Auto | Off | On`) is documented in the stdlib `.ri` but enum-typed `structure def` fields are subject to the same GR-001 runtime-ctor gap. Effectively the user cannot today author `ElasticOptions { shell_force: ShellForce.On }` and have the value reach the solver.

### M-018: auto-classification dispatch (T18, task 3031)

- **State:** TODO
- **Failure mode:** F6 (depends on M-001..M-017 plus the absent FEA engine entry-point)
- **Evidence:** Task 3031 pending; `metadata.files = ["crates/reify-solver-elastic/src/lib.rs", "crates/reify-solver-elastic/src/classify.rs", "crates/reify-kernel-openvdb/src/kernel.rs", "crates/reify-eval/src/dispatcher.rs"]` — `classify.rs` does not exist; `dispatcher.rs` has no shell-routing code. Gated on 2923 + 2924 (FEA engine integration, both also pending).
- **Blocks:** T19 (3032), T23 (3036).
- **Note:** The "per-body extraction cached as ComputeNode keyed on geometry hash + extraction options" — i.e. wiring `reify-shell-extract` outputs into the M-003 (ComputeNode) cache-key story from the FEA findings — has zero implementation. This is where the shells PRD inherits the entire GR-001 / 2924 / 3426 runtime-eval blocker chain from the FEA PRD.

### M-019: shell-extraction failure handling + diagnostic mapping (T19, task 3032)

- **State:** TODO
- **Failure mode:** F6
- **Evidence:** Task 3032 pending; `metadata.files = ["crates/reify-eval/src/engine_build.rs", "crates/reify-shell-extract/src/diagnostics.rs"]` — `diagnostics.rs` does not exist; no `engine_build.rs` consumer of shell extraction.
- **Blocks:** T23 (3036).
- **Note:** PRD's "hard error on explicit @shell, fallback-with-diagnostic on auto" policy depends entirely on T18 reaching the extraction call sites, so this gates on M-018.

### M-020: persistent-naming for derived mid-surface entities (T20, task 3033)

- **State:** PARTIAL
- **Failure mode:** F6 (records produced; fold into TopologyAttributeTable deferred)
- **Evidence:** `crates/reify-shell-extract/src/mid_surface_naming.rs:14-27` — "Mid-surface geometry is **derived** (voxel-side) and pre-dates OCCT-handle assignment — there is no handle to key against at the point of population. The downstream engine integration (deferred to T18) takes this struct, assigns handles to the regions and edges, and then folds the records into the table." Task 3033 merged cc78176a66.
- **Blocks:** T18 (3031) for the fold-into-table step.
- **Note:** Selector syntax (`body.mid_surface().face("region_0")`) is not currently a real path from user `.ri` code — that needs T18 to attach handles before the persistent-naming table can resolve `body.mid_surface().face(...)`.

### M-021: shell benchmark suite (pinched cylinder, Scordelis-Lo, hemisphere, twisted beam) (T21, task 3034)

- **State:** PARTIAL
- **Failure mode:** F1 (benchmarks present but smoke-tested with bands 21–2200× wider than published references because of M-005 DRIFT)
- **Evidence:** `crates/reify-solver-elastic/tests/shell_benchmarks.rs` — file-level docstring (`13-32`): "They do **not** assert against the published MacNeal-Harder reference... locking on curved geometry (factor 21–2200× under-prediction at coarse mesh resolution). Tightening the bands to the published references" gated on MITC3+. Per-test commentary at `:380` ("76× under the published reference"), `:710-711` ("~21× gap"), `:903` ("MITC3 4×4 output is ~1.4e-2; published MacNeal-Harder reference 0.3024"). Task 3325 (real validation) was cancelled per `done_provenance` of task 3034; task 3349 was the renaming + band widening that closed the gap-of-record. Task 3392 (curved-element MITC3+) was filed but is pending, low priority.
- **Blocks:** Real shell credibility.
- **Note:** The PRD-listed benchmarks all exist as files, all pass — but pass only because the bands span both today's under-predicted value and the published reference. This is functionally an inverted assertion of the PRD's "MITC3+ accuracy" claim. Combined with M-005, this is the single most consequential gap in the shells PRD.

### M-022: mixed-region validation (flexure-on-block) (T22, task 3035)

- **State:** TODO
- **Failure mode:** F6
- **Evidence:** Task 3035 pending; expected file `crates/reify-eval/tests/shell_mixed_region_validation.rs` does not exist; the validation depends on T12 (M-012) wiring the extractor→mesher→assembler→MPC chain.
- **Note:** Until M-012 / M-018 land, no fixture exists to run a mixed shell/tet benchmark from user-level geometry.

### M-023: end-to-end thin-walled-bracket example with `param thickness = auto` (T23, task 3036)

- **State:** TODO
- **Failure mode:** F6
- **Evidence:** Task 3036 pending; expected file `examples/shells/thin_walled_bracket.ri` does not exist (no `examples/shells/` directory). Depends on M-018 (auto-classification routing), M-005 (correct shell accuracy — currently DRIFT), and M-021 (validation underpinning the accuracy claim — also affected by M-005).
- **Note:** `param thickness : Length = auto` is grammar-supported at the param-default position via `auto_keyword` — this dependency is NOT a blocker. The end-to-end demo's `minimize mass subject to max(stress.top.von_mises) < material.yield_stress` syntax depends on (a) GR-001 (`Steel_AISI_1045()` runtime structure ctor), (b) M-016 (`stress.top` field access on a runtime structure-instance), (c) M-005 underlying-element accuracy claim being credible. Today none of (a)-(c) is wired. Broader `auto` binding-site coverage (beyond param-default) is being addressed by `docs/prds/auto-binding-site-positions.md` (α task 3802 landed; β–ε queued). *(2026-05-27 update)*

### M-024: `to_global(stress, frame)` stdlib helper (PRD §"Stress frame")

- **State:** FICTION
- **Failure mode:** F1 (comment-only mechanism — no implementation)
- **Evidence:** Referenced in `crates/reify-compiler/stdlib/solver_elastic.ri:278-280` and `crates/reify-solver-elastic/src/shell_result.rs:12, 39`. `grep -rn '"to_global"|fn to_global|to_global =>' crates/reify-stdlib/ crates/reify-eval/src/ crates/reify-compiler/stdlib/` returns zero implementation hits. `shell_element_frame()` (the Rust producer of the rotation matrix) is wired, but no stdlib-visible name binds it.
- **Blocks:** User code wanting to transform shell stress to global frame.
- **Note:** Same shape as GR-001 family — a PRD-promised user-visible name that has no runtime backing. Cleanly fixable; just hasn't been done.

### M-025: B-rep → Voxel realization via dispatcher chain (PRD precondition)

- **State:** PARTIAL
- **Failure mode:** F4 (FFI gated on `cfg(has_openvdb)`; B-rep→Voxel sampling deferred)
- **Evidence:** `crates/reify-kernel-openvdb/src/register.rs:30-33`: "Voxel→Mesh surfacing (`Convert { from: Voxel } → Mesh`): marching-cubes / level-set surfacing... deferred as a follow-up task" and "BRep→Voxel sampling: deferred to a separate follow-up." Capability descriptor declares only `(BooleanUnion|Difference|Intersection, Voxel)`. `realize_voxel_from_mesh` (FFI) exists at `kernel_real.rs:82` — Mesh→Voxel works, but BRep→Mesh→Voxel dispatcher chain has no shipping consumer for the shells path. `reify-shell-extract` doc (`lib.rs:21-34`) admits it currently works on synthetic `SampledField` because the OpenVDB-FFI producer isn't wired.
- **Blocks:** Real end-to-end shell extraction from user geometry (M-018, M-022, M-023).
- **Note:** This is the PRD's hard precondition. It's not part of the shells task list (it's in the v0.2 multi-kernel PRD's territory) but the shells PRD assumes it ships.

### M-026: `param thickness : Length = auto` for end-to-end example (T23)

- **State:** Cross-PRD breadcrumb — not classified here
- **Evidence:** PRD line 137 cites `param thickness : Length = auto` as the demo surface. *(2026-05-27 update: the param-default `= auto` form is grammar-supported via `auto_keyword`. The value-position `auto` coverage is owned by `docs/prds/auto-binding-site-positions.md`. The type-arg-position `auto:` / `auto(free):` form (M-002 / M-010) is **also** now parser-supported (grammar.js:710-729, commit a46e7d3888); the remaining gap there is semantic — the compile pipeline never invokes the auto-type-param resolver against parsed `auto_type_arg` nodes. See breadcrumbs section for the ownership split.)*
- **Note:** See breadcrumbs section.

## Cross-PRD breadcrumbs

- **GR-001 (struct-ctor runtime eval)** — gates the runtime form of `ElasticResult`, `ShellStress`, `ElasticOptions`, and `ShellForce` enum. The `Map`-tagged builtin-ctor path (PascalCase `FixedSupport`, `PinnedSupport`; snake_case `point_load`) is the operational substitute; structure-def syntax is currently parser-only with no consumer.
- **Task 3426 (pending) → 2924 (pending) chain** — the shells PRD's M-018 (auto-classification dispatch) routes through `solve_elastic_static`, which has no stdlib `fn` declaration. Inherits the FEA PRD's M-001 gap verbatim.
- **`compute-node-infrastructure.md`** — T18's "extraction cached as a ComputeNode keyed on geometry hash + extraction options" depends on the same `@optimized` → ComputeNode lowering for `fn` context that the FEA PRD's M-002 says is PARTIAL (field plumbing exists; `eval_user_function_call` ignores `optimized_target`).
- **`multi-kernel.md` v0.2 PRD** — owns the OpenVDB FFI follow-up that M-025 depends on. Shells PRD's "Pre-conditions for activating" line 42 states this as a gate; reality is the gate is half-open.
- **`persistent-naming-v2.md`** — M-020 (mid-surface naming) is structurally ready but folding into the OCCT-handle-keyed `TopologyAttributeTable` is deferred to T18. Note also that `Role::MidSurfaceEdge` and `FeatureId::derived_mid_surface` are present in `reify-types`, so the cross-PRD hook landed.
- **`auto-resolution-backtracking.md`** — owns the residual type-arg-position `auto:` work (B1 chain); M-026's `param thickness : Length = auto` references the value-position form, which is grammar-supported and now owned by `docs/prds/auto-binding-site-positions.md`. *(2026-05-27 update: ownership split — value-position `= auto` → auto-binding-site-positions PRD; type-arg-position `auto:` / `auto(free):` → auto-resolution-backtracking. Note: the type-arg-position grammar also landed (grammar.js:710-729, commit a46e7d3888); the remaining B1 chain is the semantic / compile-pipeline call-site wiring, not parser work.)*
- **`fea-gui-rendering-shells.md`** — sibling PRD. PRD §85 defers GUI rendering of shell results (mid-surface display, top/mid/bottom stress toggle, shell-normal debug overlay) to that PRD; not in scope here.
- **`mesh-morphing.md`** — PRD claims "mid-surface morphs alongside the original body geometry under parameter changes; warm-start preservation works the same way as for tet meshes." Today: no mid-surface morphing code path; depends on M-018 first.
- **`a-posteriori-error-estimation.md`** — PRD claims "Z-Z indicator extends to shell elements with through-thickness sampling." Not verified here; cross-PRD.
- **`hex-wedge-meshing.md`** — sibling PRD with overlapping thin-body motivation; mentioned by PRD as "partial overlap". Out of scope here.
- **`multi-load-case-fea.md`** — PRD says "shells participate in multi-load workflows the same way solids do; envelope reductions work over the new structured stress field." Verifying envelope behaviour over `Value::Map`-tagged shell `ElasticResult.stress.top` requires GR-001 — currently fiction.
