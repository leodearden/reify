# Phase 3 — File-Based Synthesis

**Date:** 2026-05-12
**Method:** Read every findings file under `findings/` (40 PRDs) and clustered every non-WIRED mechanism by underlying mechanism rather than PRD. Sibling agent is doing the memory-based path; this file is the file-only synthesis.

## 1. Cluster Table

Each cluster collapses gap findings from multiple PRDs that point to the **same underlying mechanism**. Sorted by (PRDs citing × estimated blocks count) descending — cross-cutting blockers first.

| # | Cluster | Mechanism | Dominant state | F# | PRDs citing | Blocks count | Candidate disposition |
|---|---|---|---|---|---|---|---|
| C-01 | GR-001: struct-ctor runtime evaluation | `StructureName(...)` → `Value::Map` at runtime (instead of `Value::Undef`) | FICTION | F1 | structural-analysis-fea, multi-load-case-fea, structural-analysis-shells, structural-stability-buckling, composite-laminated-shells, varying-thickness-shells, fea-gui-rendering, persistent-naming-v2 (M-022 AdHocSelector parallel), field-source-kinds (M-016 grid-kind), kinematic-constraints-toplevel (M-022 stdlib types), pragmas (transitive), reify-doc-tool (M-006 sibling), persistent-fea-cache (transitive) | ~30+ | **PRD-shape work** — umbrella PRD for "structure-instance runtime representation" (declared seed GR-001) |
| C-02 | `@optimized fn` lowering + ComputeNode dispatch | The producer of ComputeNodes — `fn` annotated `@optimized("target")` lowers to insert_compute_node; no production caller today | FICTION | F6 | compute-node-infrastructure (M-014/M-015/M-016), structural-analysis-fea (M-001/M-002), structural-analysis-shells (M-018), multi-load-case-fea (M-005/M-006), fea-gui-rendering (M-019), persistent-fea-cache (M-011), warm-state-eviction (M-013), a-posteriori-error-estimation (M-018), structural-stability-buckling (M-002), hex-wedge-meshing (transitive), composite-laminated-shells (transitive), mesh-morphing (less direct) | 14+ | **PRD-shape work** — tasks 3379/3383/3384 must land first; the "producer of ComputeNodes" should be a single named PRD effort |
| C-03 | Field<X,Y> in param position (TODO #3117) | Type resolver doesn't accept `Field<X,Y>` in `param` position; every kernel result reverts to `Real` placeholder | TODO | F1 | structural-analysis-fea (M-022), a-posteriori-error-estimation (M-011), structural-analysis-shells (M-016/M-017), multi-load-case-fea (M-009), composite-laminated-shells (M-007/M-011), varying-thickness-shells (M-006), fea-gui-rendering-shells (M-004), structural-stability-buckling (M-003/M-005) | 8+ | **fix-now** — task #3117 already filed; resolve as enabling work for FEA stack |
| C-04 | Library-shipped / no-DSL-consumer (selector resolution) | Library function exists with tests, but no surface DSL path invokes it: `resolve_unique_by_attribute`, `resolve_unique_by_tag`, ad-hoc `@face("name")` evaluator, narrow_arms_under_guard, NodePolicyOverrides config | PARTIAL/FICTION | F2 | persistent-naming-v2 (M-013, M-014, M-019, M-022), topology-selectors (M-003), match-block-decls (M-012), node-trait-composition (M-010), auto-type-param-resolution (M-009/M-016) | 10+ | **investigate-further** — many "done" tasks discovered to be library-only; needs a "definition of done = user-observable" disposition |
| C-05 | Auto-resolve / type-param resolver compile-pipeline call site | Phase A/B/C orchestrator + DFS + backjumping all wired; no production caller invokes them from `compile_*`; `CompiledModule.auto_type_substitution` never written | FICTION | F1 | auto-type-param-resolution (M-009/M-010/M-014), auto-resolution-backtracking (M-002/M-014), kleene-logic (M-002 sibling — `implies` operator no parser), match-block-decls (M-001 — decl-level match no parser), specialization-scope (M-002 — sub body no parser), shadowing-warning (M-015/M-016) | 7+ | **fix-now** — wire orchestrators into compile pipeline; small, contained |
| C-06 | Grammar-level fictions (PRD assumes unparseable syntax) | Surface DSL the PRD authors invented but never landed in tree-sitter grammar: `auto:` in type_arg_list, `sub name : Type { body }`, decl-level `match`, `forall ... : <body>` for sub bodies, `subject to`, `chain` body, kind-bound `auto: Nat`, `implies` operator, schema = { x: Length(mm) } block, Length(mm) typed column, `name = "..."` user-label syntax, `sum(... for ... in ...)` comprehension, `@shell(thickness = linear_taper(...))` Expr annotation arg, `#[allow(shadowing)]` Rust-bracket form, `RegularGrid1` struct ctor | FICTION | F1 | auto-resolution-backtracking, auto-type-param-resolution, kleene-logic, match-block-decls, specialization-scope, multi-load-case-fea (M-015), money-dimension (M-014), varying-thickness-shells (M-005), field-source-kinds (M-016), imported-field-source-hdf5-csv (M-006/M-007), persistent-naming-v2 (M-015), shadowing-warning (M-015), forall-statement-form (M-013 chain) | 13+ | **PRD-shape work** — needs a "PRD authors must confirm grammar+parser+lowering before signing the PRD" gate. **2026-05-27 update:** `= auto` at the param-default position was always parseable via `auto_keyword` and has been removed from this cluster's mechanism list. Broader binding-site coverage (sub-overrides, named-args, let, connect-param) is being addressed by `docs/prds/auto-binding-site-positions.md` (α task 3802 landed; β–ε tasks 3804/3805/3806/3807 queued). |
| C-07 | Task marked done; runtime contract absent | Task closure flag is optimistic relative to user-observable behavior. Examples: task 2954 (screenshot_window docs-only), 2967 (auto-resolve panel, GUI ready but backend producer absent), 2959/2963 (FEA scalar_channels schema only), 250 (AdHocSelector — runtime returns Undef), 2699 (eval dispatch for 11 selectors absent), 2657/2658 (Manifold MeshGL stub), 2347 (audit doc stale), 215 (CompiledModule doc field never added), 3034 (shell benchmarks pass by widened bands), 2657 (compute-node-infrastructure follow-up), 2971 (NFS detection unbuilt), 2335 (freshness propagate walk no caller), 2671 (mechanism builder), 2645 (OpenVDB ingest never wired into elaborate_field), ~~2491 (auto-type-param-resolution call-site)~~ [mis-cited 2026-05-12 reopen-amend sweep: 2491 is actually polish-grade OCCT-inertia test tweaks; real auto-type-param gap is owned by task #3465], 2349/2352/2354 (stdlib trait edges shipped but audit doc stale), ~~2456 (cache config no consumer)~~ [mis-cited 2026-05-12 reopen-amend sweep: 2456 is actually a WarmStatePool donate-event fix; real cache-config gap is owned by task #3467] | DRIFT | F5 | fea-gui-rendering, persistent-naming-v2, stdlib-trait-breadth, node-trait-composition, freshness-4-variant, imported-field-source, topology-selectors, auto-type-param-resolution, reify-doc-tool, kinematic-constraints-v02, kinematic-constraints-toplevel (M-007 v0.1 closed-chain v0.2 superseded) | 15+ | **investigate-further** — symptom of process gap, not architecture; propose "what counts as done" policy |
| C-08 | Load / Support type system (kind-tagged Maps vs trait-typed structs) | PRD prose says `List<Load>`/`List<Support>` with nominal traits; code ships builtin name-dispatched ctors producing `Value::Map` with `kind` key. snake_case (`point_load`) vs PascalCase (`FixedSupport`) inconsistency. No `trait def Load` / `trait def Support` | DRIFT | F3 | structural-analysis-fea (M-011/M-012), structural-analysis-shells (M-015), multi-load-case-fea (M-001), structural-stability-buckling (M-012), composite-laminated-shells (M-002), kinematic-constraints-v02 (M-007 multi-DOF analog), kinematic-constraints-toplevel (M-022 stdlib types) | 7+ | **PRD-shape work** — adjacent to C-01; design decision needed about nominal vs structural conformance |
| C-09 | DRIFT on diagnostic-code naming (W_X vs PascalCase) | PRD says `W_DEEP_DOT_CHAIN` / `W_SHADOW` / `W_TOPOLOGY_TAG_STALE` / `E_GEOMETRY_UNBOUNDED` style; code uses `DeepDotChain` / `Shadowing` / `TopologyTagStale` PascalCase. PRD says `EventKind::error`; code uses `EventKind::Failed` | DRIFT | F7 | deep-dot-chain (M-004), shadowing-warning (M-013), topology-selectors (M-009), geometry-traits (M-009 doc only), freshness-4-variant (M-014), specialization-scope (E_SPECIALIZATION_FORBIDDEN_DECL — matches), pragmas (multiple) | 6+ | **accept-and-document** — codebase has converged on PascalCase; update PRD prose en masse |
| C-10 | Persistent-naming "library function + no DSL caller" (selector v2 vocabulary) | Selector vocabulary v2: `intersect/union/complement/except`, `faces_perpendicular_to`, `extremal_by_bbox`, `faces_by_surface_kind`, etc. all live in `reify-eval/src/selector_vocabulary_v2.rs` but none are registered in `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` | FICTION | F1 | persistent-naming-v2 (M-019), topology-selectors (M-003 — the 11 task-2699 names not in eval dispatch) | 2 PRDs, 22+ functions | **fix-now** — register names in dispatch table |
| C-11 | Boolean/fillet/chamfer attribute propagation eval-side wiring | OCCT FFI (`boolean_fuse_with_history`, `fillet_with_history`, `chamfer_with_history`) exists + propagator `propagate_attributes_via_brepalgoapi_history` exists, but `AttributeHistory::Boolean/Fillet/Chamfer` variants don't exist + `execute_with_history` falls through to `AttributeHistory::None` | FICTION | F1 | persistent-naming-v2 (M-009, M-010, M-012), mesh-morphing (transitively M-005), topology-selectors (transitively), structural-analysis-fea (transitively via topology-selectors) | 5+ | **fix-now** — add enum variants and wire dispatch; tasks 2656/2831 already filed |
| C-12 | Cache directory / config-file ingestion (env-only or unread) | Config-file plumbing exists in `reify-config` but no consumer reads it: `Manifest::kernel_pins`, `CacheConfig`, `NodePolicyOverrides`, `warm_state_budget_bytes`, `auto_type_params::max_depth` | PARTIAL | F2 | multi-kernel (M-013), persistent-fea-cache (M-009), node-trait-composition (M-010), warm-state-eviction (M-002), auto-type-param-resolution (M-004 partial), pragmas (M-016 kernel_pragma unread), deep-dot-chain (M-003), reify-doc-tool (M-020 declared_version unread) | 8+ | **fix-now** — wire the consumers; per-PRD trivial; pattern is structural |
| C-13 | GUI → backend event channel absent | Frontend subscribes to events with optimistic comment "backend wired later"; Tauri-side emitter is missing: auto-resolve events (start/iteration/complete), morph stats RPC, solver progress overlay, mode-shape animation, mesh-morph debug RPC, FEA case picker, GUI shell display mode | FICTION | F1 | fea-gui-rendering (M-013, M-015), fea-gui-rendering-shells (M-001..M-009), multi-load-case-fea (M-016), mesh-morphing (M-014), structural-stability-buckling (M-013), warm-state-eviction (M-011), persistent-naming-v2 (M-018 Manifold hook UI) | 7+ | **PRD-shape work** — need a "GUI event channel inventory" PRD that catalogs which IPC events need backends |
| C-14 | Engine wiring (kernel module callable in isolation, no engine consumer) | `reify-mesh-morph` engine wire absent, `reify-shell-extract` not depended on by `reify-solver-elastic`/`reify-eval`, dispatcher::dispatch not called from execute_realization_ops, propagate_freshness_only no engine caller, dispatch_volume_mesh no caller, mesh_surface_to_volume_with_diagnostics no caller | FICTION/PARTIAL | F4/F6 | mesh-morphing (M-012/M-013/M-014/M-015/M-016/M-017/M-018), structural-analysis-shells (M-018), hex-wedge-meshing (M-017/M-018), multi-kernel (M-004/M-014/M-015), freshness-4-variant (M-013), structural-analysis-fea (multiple) | 8+ | **PRD-shape work** — propose "engine integration phase" as standard PRD-decomposition step; tasks 2924/2947 sit here |
| C-15 | Unbounded geometry primitives (half_space / extrude_infinite) | Diagnostic infrastructure (`E_GEOMETRY_UNBOUNDED`), inference fallback, warning machinery all wired and waiting; producers of `Bounded=false` are absent | FICTION | F1 | geometry-traits (M-006/M-009), topology-selectors (M-016) | 2+ | **investigate-further** — small surface, but conceptually-coupled to several follow-ups |
| C-16 | Material starter library (stdlib structures unevaluable) | `Steel_AISI_1045()`, `Aluminium_6061_T6()` etc. parse fine but → `Value::Undef`; transitively blocks every FEA-stack consumer; pure subcase of C-01 but worth surfacing for cardinality | FICTION | F1 | structural-analysis-fea (M-010), multi-load-case-fea (transitive), structural-analysis-shells (M-016), structural-stability-buckling (M-011), composite-laminated-shells (M-001/M-002/M-013) | 5+ | (subsumed by C-01) **PRD-shape work** |
| C-17 | OpenVDB / multi-format ingestion path | OpenVDB FFI lives in `reify-kernel-openvdb` with full ingest module; `reify-eval` doesn't depend on `reify-kernel-openvdb`; `elaborate_field`'s `CompiledFieldSource::Imported` arm returns `Value::Undef`. HDF5 and CSV crates absent from workspace | FICTION | F1 | imported-field-source (M-007/M-008/M-009/M-010/M-011/M-013), imported-field-source-hdf5-csv (M-001/M-004/M-005), structural-analysis-shells (M-025 voxel realization), multi-kernel (M-011), varying-thickness-shells (M-006 imported_thickness_map) | 5+ | **PRD-shape work** — small focused PRD: "wire OpenVDB into elaborate_field" |
| C-18 | Kernel/eval ReprKind chain coverage gaps | Convert edges absent from capability descriptors (BRep→Mesh, BRep→Voxel, Voxel→Mesh, Mesh→BRep); cache key fields incomplete (force_tet not in compute key); per-handle ReprKind tracking absent | FICTION/PARTIAL | F6 | multi-kernel (M-007/M-009/M-010/M-011/M-014/M-015), hex-wedge-meshing (M-024), structural-analysis-shells (M-025) | 3+ | **PRD-shape work** — Phase 3 of multi-kernel PRD |
| C-19 | Mid-surface / shell-extract → engine bridge | Mid-surface mesh + segmentation + per-vertex thickness all produced in `reify-shell-extract`; never transported through IPC to GUI; never bridged to `reify-solver-elastic`'s persistent-cache ElasticResult | FICTION | F1 | structural-analysis-shells (M-018/M-019/M-020/M-022/M-023), fea-gui-rendering-shells (M-002/M-004/M-014), varying-thickness-shells (M-001), composite-laminated-shells (M-005/M-006) | 4+ | **PRD-shape work** — shell-extract engine integration PRD |
| C-20 | MITC3 vs MITC3+ DRIFT (shell accuracy benchmark bands widened) | Shipped element is bare MITC3 on flat facets; benchmarks (3034) widened bands to 21–2200× to pass; PRD claimed MITC3+ specifically to avoid the regime this hit. Curved-element MITC3+ is task 3392 (low priority pending) | DRIFT | F1 | structural-analysis-shells (M-005, M-021), composite-laminated-shells (transitive), structural-stability-buckling (transitive), varying-thickness-shells (transitive) | 4+ | **investigate-further** — Phase 2 named this specifically as a one-instance failure; needs Leo decision on whether to retire MITC3+ claim or unblock task 3392 |
| C-21 | Vertex-correspondence in PNv2 always-empty | `CorrespondenceMap.vertex_to_vertex` is structurally empty; any P1 mesh with surface nodes on B-rep vertices fails morph (`ProjectionFailure::MissingCorrespondence`) | FICTION | F1 | mesh-morphing (M-002), persistent-naming-v2 (transitively) | 2 | **investigate-further** — Leo decides between "restrict morph eligibility" and "fill the bijection" |
| C-22 | Eigenvalue solver + geometric stiffness K_g | Lanczos/Arnoldi/shift-invert via faer-rs named as the path but no `eigensolve` / `K_g` module exists; eigenvalue solver is the largest net-new kernel surface for buckling | FICTION | F6 | structural-stability-buckling (M-006, M-007), (modal analysis future PRD) | 1 PRD currently | **PRD-shape work** — buckling-specific decomposition once foundations land |
| C-23 | Failed/error event naming (PRD vs code drift) | PRD/spec say `EventKind::error`; code says `EventKind::Failed` | DRIFT | F7 | freshness-4-variant (M-014), node-trait-composition (transitive) | 2 | **accept-and-document** — rename PRD or code, choose one |
| C-24 | Doc-comment propagation through compilation (M-006 reify-doc-tool) | PRD's preamble asserts `TopologyTemplate / CompiledFunction / TraitDef / EnumDef` "all carry `doc: Option<String>`" — none of these compiled types has the field; AST has doc; LSP reads from AST; compiler drops it | FICTION | F1 | reify-doc-tool (M-006), pragmas (M-012 cross-ref) | 2 | **fix-now** — add `doc: Option<String>` to compiler structs; mechanical |
| C-25 | DSL build_doc_model + reify-doc CLI wiring | `build_doc_model(&CompiledModule, &str) -> DocModel` absent; HTML formatter exists but CLI uses stub; CLI passes empty DocModel; `reify doc` produces empty output | FICTION/PARTIAL | F1/F4 | reify-doc-tool (M-005, M-008, M-022) | 1 PRD | **fix-now** — wire `build_doc_model` and replace `render_html_stub` |
| C-26 | Stdlib audit / config docs / examples missing or stale | **RESOLVED 2026-05-14 (task 3529):** `stdlib-trait-audit.md` renamed to `stdlib-trait-breadth-audit-v01.md` (PRD-named path) and refreshed to reflect post-2349/2352/2354 state; PRD-named example file `integration_full_v01.ri` missing `#precision(0.001m)` (resolved by commit `7f01d82e9c`); example `multi_load_bracket.ri` doesn't exist; `examples/m11_annotations.ri` doesn't exercise solver_hint collections; PRD-named smoke test on `examples/m5_purpose.ri` doesn't exist; `docs/auto-type-param-resolution.md` completeness unverified | DRIFT/TODO (partially resolved) | F5 | stdlib-trait-breadth (M-002), pragmas (M-018), multi-load-case-fea (M-015), solver-hint-payloads (M-012), freshness-4-variant (M-018), auto-type-param-resolution (M-014), structural-analysis-fea (M-027), structural-analysis-shells (M-023), hex-wedge-meshing (M-023 validation suite) | 9+ | **accept-and-document** — single sweep to update or file follow-ups |
| C-27 | Dimensional type aliases at stdlib level (`Pressure`, `Energy`, etc.) | `DimensionVector::PRESSURE` exists but no `type Pressure = Scalar<PRESSURE>` alias at `.ri`-source level. Every stdlib trait downgrades typed params to `Real` with comment | TODO | F1 | stdlib-trait-breadth (M-012), structural-analysis-fea (multiple), structural-analysis-shells (M-017), money-dimension (M-014) | 4+ | **fix-now** — task 3115 already filed |
| C-28 | `Solid` as a usable stdlib `.ri` param type | `Solid` resolves to `Type::Geometry` as builtin alias but stdlib traits cannot use it as a slot type; PRD-spec `Physical` shape (`geometry: Solid`) blocked | ORPHAN | F1 | stdlib-trait-breadth (M-013), geometry-traits (transitive), structural-analysis-fea (transitive Material chain) | 3+ | **investigate-further** — language-design question; may want PRD |
| C-29 | Composed/derived stress recovery for varying shapes | `to_global(stress, frame)` helper named in stdlib but absent; `linear_combine` outputs synthesized with Undef frame; envelope helpers Real-placeholder typed | FICTION/PARTIAL | F1 | structural-analysis-shells (M-024), multi-load-case-fea (M-010), composite-laminated-shells (M-007/M-012), fea-gui-rendering-shells (M-007) | 4+ | **fix-now** — small, cleanly-scoped helpers; depends on C-01 |
| C-30 | Local-disk NFS detection + cache GC + cache CLI surface | `reify cache stats|clear|gc|export|import` not in `reify-cli/src/main.rs`; NFS detection (PRD §"Local-disk only") not implemented; cost-aware LRU GC not implemented | TODO | F4 | persistent-fea-cache (M-010, M-013, M-014, M-015, M-016, M-017, M-018, M-019) | 1 PRD, ~8 tasks | **PRD-shape work** — already PRD-decomposed; just blocked on upstream FEA wiring |
| C-31 | Kinematic interferes/clearance: FK-transform application | OCCT distance probe deliberately does NOT apply per-body `world_transform`; sweep-driven interference NOT supported; `interferes_with`/`min_clearance` share gap | DRIFT | F2 | kinematic-constraints-toplevel (M-019/M-020/M-021) | 1 PRD | **fix-now** — single fix; well-scoped |
| C-32 | Long-chain diagnostic / per-stage tolerance budget unreachable | `is_long_chain_realization` + `long_chain_diagnostic` exist but no in-tree caller; `per_stage_tolerance_for_plan` degenerates because `BUDGET_QUERY_TRIPLE_V02 = BooleanUnion (BRep, &[BRep])` fixes `n_stages = 0` | PARTIAL | F2 | per-purpose-tolerance (M-008, M-011), multi-kernel (M-017) | 2 | **investigate-further** — gated on multi-kernel dispatch landing (C-18) |
| C-33 | Cancellation handle placeholder | `CancellationHandle` is `struct CancellationHandle;` (unit type); real type deferred to P3.5; FEA cancellation regression test unimplementable | TODO | F3 | compute-node-infrastructure (M-003, M-007), structural-analysis-fea (M-007), structural-stability-buckling (M-002 transitive) | 3+ | **fix-now** — task 3384 — pick one of three options |
| C-34 | Imported-tolerance promise: extractor reads non-existent `tolerance` cell | `extract_input_tolerance_promise` reads `ValueCellId(input_template_name, "tolerance")`; stdlib `Input` trait has no `tolerance` cell — uses `Provenance.tolerance_guarantee` indirection | DRIFT | F1 | per-purpose-tolerance (M-009, M-013) | 1 PRD | **fix-now** — small contained fix |
| C-35 | Tolerance MVP scope (bare-param-subject only) | `RepresentationWithin(subject, tol)` recognizes only bare-param subjects; member-access subjects (`subject.head`) deferred | PARTIAL | F1 | per-purpose-tolerance (M-001), structural-analysis-fea (transitively) | 2 | **investigate-further** — design open whether to widen or document |
| C-36 | NodeTraits + scheduler dispatch never bridge | Parallel taxonomies: `NodeTraits` bitflags + `NodeArchKind` (7 variants) in reify-types AND `NodePolicyOverrides` + `NodeKind` (5 variants) in reify-runtime. Scheduler reads NodePolicyOverrides, ignores NodeTraits | FICTION | F1 | node-trait-composition (M-002, M-003, M-005, M-008, M-009, M-011) | 1 PRD, 6 mechanisms | **investigate-further** — Leo decides between "merge taxonomies" and "rename/retire one" |
| C-37 | Kinematic singularity surfacing | `solve_loop_closure_with_diagnostics` wired but bypassed by snapshot()/sweep(); typed diagnostic variants reserved but never reach `EvalResult`; `is_singular` flag never on Snapshot Map | PARTIAL/FICTION | F4 | kinematic-constraints-v02 (M-009, M-010, M-011), kinematic-constraints-toplevel (M-007 closed-chain) | 2 | **fix-now** — connect the wrapper to user surface |
| C-38 | Method-call AST shape absent | Reify `ExprKind::FunctionCall { name: String, args: Vec<Expr> }` takes bare name, not Expr callee. PRD acceptance assumed but vacuously satisfied; "a.b.foo()" can't be expressed | FICTION | F1 | deep-dot-chain (M-008) | 1 PRD | **accept-and-document** — flag as language-design open |
| C-39 | Manifold MeshGL attribute hook stub | Manifold `KernelAttributeHook::propagate_attributes` returns `Discarded` with `tracing::warn!(reason="task_9_pending")` — only trait wiring; no MeshGL walk | FICTION | F1 | persistent-naming-v2 (M-018), multi-kernel (M-018) | 2 | **fix-now** — file a tracking task with explicit body |
| C-40 | Composite/buckling/varying-thickness greenfield | Every named entity (OrthotropicMaterial, Laminate, Ply, tsai_wu/hashin/max_strain, K_g, eigensolver, linear_taper, …) is FICTION; PRDs are stubs deferred to v0.5+ | FICTION | F1 | composite-laminated-shells (all), varying-thickness-shells (all), structural-stability-buckling (all) | 3 PRDs | **accept-and-document** — properly deferred v0.5+ work; not active gaps |
| C-41 | Stdlib doc generator stdlib-page surface absent | `cmd_doc` CLI rejects stdlib-walking; no `reify doc` surface produces stdlib trait/structure pages | FICTION | F1 | solver-hint-payloads (M-011), reify-doc-tool (M-022/M-023) | 2 | **fix-now** — wire stdlib-walk in CLI |
| C-42 | Cost-per-byte LRU comparator inactive | `WarmStatePool` stores cost_per_byte but eviction comparator is pure LRU; test actively pins the drift (`cost_per_byte_does_not_alter_lru_eviction_order`) | DRIFT | F2 | warm-state-eviction (M-004, M-005) | 1 PRD | **investigate-further** — flip comparator + wire cold-compute timing |
| C-43 | Warm-state pool: drain_events to journal never wired | WarmStatePool buffers Evicted/Donated events; `drain_events()` has zero non-test callers; no GUI surface | FICTION/PARTIAL | F4 | warm-state-eviction (M-010, M-011) | 1 PRD | **fix-now** — wire drainer at eval boundary |
| C-44 | Backward-compat alias `result.stress = result.stress.mid` | PRD-promised aliasing for shell ElasticResult, documented in stdlib comments only; not implemented | FICTION | F1 | structural-analysis-shells (M-016), fea-gui-rendering-shells (transitive) | 2 | **fix-now** — small mechanical fix |

---

## 2. Pattern Application

The Phase 2 summary named six recurring shapes. Mapping clusters to patterns:

### Pattern 1: Scaffold-without-a-caller (one-sided contract)
The most common pattern by cluster count: producer-side or consumer-side infrastructure built with tests but no production call site.
- **C-02** (@optimized fn lowering — ComputeNode producer absent)
- **C-04** (library-shipped / no-DSL-consumer — selector resolvers)
- **C-05** (auto-resolve / type-param orchestrator no compile-pipeline call site)
- **C-10** (Persistent-naming selector v2 — Rust pubs not in dispatch table)
- **C-11** (Boolean/fillet/chamfer attribute propagation — eval-side wire absent)
- **C-12** (Config-file ingestion — reify-config types unread)
- **C-13** (GUI subscribes to events; backend emitter absent)
- **C-14** (Engine wiring — mesh-morph/shell-extract isolated)
- **C-17** (OpenVDB ingest never wired into elaborate_field)
- **C-18** (Kernel ReprKind chain — convert edges absent)
- **C-19** (Mid-surface produced but never bridged to IPC/solver)
- **C-25** (build_doc_model absent; HTML formatter exists but CLI uses stub)
- **C-29** (`to_global` etc. helpers named but absent)
- **C-32** (long-chain diagnostic builder exists; no caller)
- **C-37** (Kinematic singularity wrapper bypassed)
- **C-39** (Manifold MeshGL hook stubbed)
- **C-41** (Doc tool: stdlib-page surface absent)
- **C-43** (warm-state drain_events never wired)
- **C-44** (`result.stress` alias documented only)

### Pattern 2: Grammar-level fictions
PRDs assume parseable syntax that doesn't exist.
- **C-06** (auto:, sub body, decl match, subject to, schema {}, Length(mm), Expr annotation args, #[allow(shadowing)], chain body, name = "...", `sum(... for ... in ...)`) — note: `= auto` at param-default position was always parseable and has been removed; broader binding-site coverage is in `docs/prds/auto-binding-site-positions.md`
- partial overlap with **C-22** (eigenvalue solver imagines `solve_buckling(...)` with no fn surface)
- partial overlap with **C-24/C-25** (PRD assumes compile-types carry `doc` field; doesn't)

### Pattern 3: Tasks marked done while wiring absent
- **C-07** is the headline collection (~15+ task-done-with-absent-runtime cases)
- Also touches: **C-02** (3380/3381/3382 done but no dispatch), **C-04** (task 2652 done but no surface caller), **C-11** (task 2656 in flux), **C-10** (2658 done as library only), **C-43** (task 2345 follow-up unowned)

### Pattern 4: GR-001 transitive blast radius
- **C-01** is the seed.
- Transitively: **C-16** (Material library), **C-08** (Load/Support), **C-29** (composed stress recovery), parts of **C-19** (mid-surface attribute), most of **C-40** (greenfield v0.5)
- Confirmed by 7+ audits as a load-bearing blocker (FEA, multi-load-case, kinematic-toplevel, varying-thickness, composite-laminated, structural-stability, field-source-kinds)
- One audit explicitly notes GR-001 does NOT transitively block (mesh-morphing M-008 sidesteps via direct primitive composition)

### Pattern 5: PRD/spec/code three-way drift on naming/types
- **C-09** (diagnostic codes W_X vs PascalCase)
- **C-23** (EventKind::error vs Failed)
- **C-20** (MITC3 vs MITC3+)
- **C-26** (audit docs, examples, deliverables stale)
- **C-08** (Load/Support kind-tagged Maps vs trait-typed structs)
- **C-34** (tolerance promise reads non-existent field)
- **C-31** (kinematic interferes ignores FK transform)
- Plus per-PRD drift cases: SchemaNode (auto-type-param), `inferred_traits` field (geometry-traits), `topology_fingerprint` ComputeNode bucket (compute-node-infrastructure), Tube convexity (geometry-traits), `sub_index` fragility (topology-selectors), pragma `#kernel` v0.1 vs v0.2 scope

### Pattern 6: Bare-MITC3 vs MITC3+ DRIFT (specific instance)
- **C-20** captures this exclusively.

### Clusters fitting NO Phase-2 named pattern
These are uncaptured shapes worth surfacing:

- **C-15** (Unbounded primitives) — "loaded gun, no target": diagnostic infrastructure exists waiting for producers that don't exist
- **C-21** (vertex_to_vertex always empty) — known-empty hole in shipped data structure
- **C-22** (eigenvalue solver / K_g greenfield)
- **C-30** (cache GC + CLI surface absent — straight up unbuilt, no scaffold either)
- **C-33** (CancellationHandle placeholder type, real type deferred)
- **C-36** (parallel taxonomies that don't compose) — Pattern 1 adjacent but distinctly a "two systems, never bridged" shape
- **C-38** (method-call AST absent — Reify language is missing a feature PRD assumes)
- **C-42** (active test pins drift — `cost_per_byte_does_not_alter_lru_eviction_order` actively pins the FICTION as the v0.1 contract)

---

## 3. New Patterns Surfaced

Three recurring shapes Phase 2 didn't name, observed across ≥3 clusters:

### New Pattern A: "Active drift pin" — test cements PRD violation
- **C-42** (cost_per_byte_does_not_alter_lru_eviction_order)
- **C-20** (shell benchmarks pass only after band-widening for MITC3-not-MITC3+; test bands span both shipped and PRD-promised values)
- **M-014 (field-source-kinds)** (`expected zero FieldSampledV02 errors after v0.2 implementation` — pins the old behavior as outdated)
- Distinct from "task marked done": here a test deliberately encodes the gap as a contract. Closing the gap requires explicit test removal/replacement.

### New Pattern B: "Two parallel taxonomies that don't compose"
- **C-36** NodeTraits vs NodePolicyOverrides (4-flag bitfield vs 3-valued enum; 7 NodeArchKind variants vs 5 NodeKind variants)
- **C-08** Load/Support kind-tagged Map dispatch vs trait-typed nominal conformance
- Two competing dispatch mechanisms; neither side has the data needed to feed the other.

### New Pattern C: "PRD names a deliverable filename that's never produced; pre-existing doc reused"
- `docs/notes/stdlib-trait-breadth-audit-v01.md` — **RESOLVED 2026-05-14 (task 3529)**: renamed from `stdlib-trait-audit.md` and refreshed to reflect post-2349/2352/2354 state
- `examples/m6/multi_load_bracket.ri` — directory `m6/` doesn't exist
- `examples/shells/thin_walled_bracket.ri` — directory doesn't exist
- `crates/reify-eval/tests/multi_load_case_superposition_validation.rs` — absent
- `crates/reify-eval/tests/persistent_cache_integration.rs` — absent
- `crates/reify-shell-extract/src/diagnostics.rs` — listed in task metadata.files but doesn't exist
- `crates/reify-solver-elastic/src/classify.rs` — same

Sub-pattern of "tasks marked done" but more specific: PRD/task points at an artifact that doesn't exist. Tracked via task metadata.files; auditor noticed by grepping for the filename.

---

## 4. Disposition Candidates per Cluster

Roll-up of dispositions:

| Disposition | Clusters |
|---|---|
| **PRD-shape work** | C-01, C-02, C-06, C-08, C-13, C-14, C-17, C-18, C-19, C-22, C-30 (11 clusters) |
| **fix-now** | C-03, C-05, C-10, C-11, C-12, C-24, C-25, C-27, C-29, C-31, C-33, C-34, C-37, C-39, C-41, C-43, C-44 (17 clusters) |
| **accept-and-document** | C-09, C-23, C-26, C-38, C-40 (5 clusters) |
| **investigate-further** | C-04, C-07, C-15, C-20, C-21, C-28, C-32, C-35, C-36, C-42 (10 clusters) |

Caveat: each cluster's disposition is "the best fit for the *cluster*, not for any specific PRD". Within most clusters individual PRDs may want different dispositions. Leo can split.

---

## 5. Surprises / Red Flags

### 5a. The "scaffold without a caller" pattern is endemic
Phase 2 named this as one of six patterns; in practice it accounts for ~19 of the 44 clusters. It is the **dominant** failure shape, not "a recurring shape". This suggests a recurring decomposition habit: when a PRD says "Phase A builds X, Phase B builds Y", the task tree often grants Phase A its own "done" gate before any consumer of X exists. Tasks 3380/3381/3382/3385 are textbook: each one is honest, each builds its named primitive, none of them produces a runtime ComputeNode.

### 5b. The runtime/compile-time boundary is recurrently mis-modeled
- GR-001 (struct ctor): PRD assumes runtime ctor; compiler accepts call-syntax; runtime returns Undef
- @optimized fn: compiler captures annotation; runtime ignores it
- doc-comment field (C-24): PRD assumes compiled-types carry doc; AST carries doc; compiler drops doc at lowering
- `RegularGrid1`/`RegularGrid2`/`RegularGrid3`: PRD imagined typed ctors; ships as string tags because struct ctors don't work
- `name = "..."` user_label (M-015 persistent-naming-v2): PRD references a v0.1 feature that never existed at parse time
- Multiple kinematic types claimed first-class in docs; live as Map records with `kind` discriminant

This is a **family** of GR-001-shaped gaps that each PRD encountered independently. Worth a Phase 3 disposition decision: **"all gaps of this shape belong under one umbrella PRD"** vs **"each PRD declares its own workaround"**.

### 5c. Multiple PRDs cite *each other's* fixtures or follow-ups, then both are absent
- mesh-morphing M-018 says benchmark depends on FEA task 2930 (which is pending)
- FEA task 2930 says cantilever fixture is shared
- buckling PRD says it depends on shells PRD (deferred) and FEA (also gated)
- shells PRD says it composes with multi-load-case (depends on FEA), mesh-morphing (depends on FEA), error-estimation (depends on FEA)

This is **cluster cycle on the FEA stack**: every consumer expects a producer that itself expects them. Practically, **task 2924 (FEA #16) is the linchpin** — at minimum 5+ PRDs unblock when it lands.

### 5d. The audit confirmed the inversion noted in PRD ordering
- v0.4 shells PRD shipped substantially (MITC3+ kinematics, MPC plumbing, shell benchmarks, voxel-medial extraction, mid-surface mesher) ahead of v0.3 solid-FEA engine integration
- v0.3 hex/wedge meshing shipped per-element primitives and the dispatcher truth-table tests ahead of the dispatcher being called
- Persistent-cache foundation (trait, header, atomic I/O, engine-version hash) shipped ahead of any ComputeNode consumer

Several audits note this; it is a real ordering hazard but distinct from gaps. Worth Leo's awareness: the FEA stack has surface area built ahead of its engine seam.

### 5e. Three audits explicitly cite "task X done provenance was via found_on_main / reconciler false-positive"
- persistent-naming-v2 task 2657 done despite Manifold MeshGL walk being stub (M-018)
- topology-selectors task 2699 carries `reopen_reason` from 2026-05-09 listing 11 missing dispatch arms — task is `done` in metadata anyway
- reify-doc-tool task 215 done despite `CompiledModule.doc` field missing on every compiled type
- node-trait-composition task 2358 done with note that AlwaysCancelWhenStale landed weeks before task closed

**Reconciler-flip false-positives** are a real source of "done-but-fictional" entries. Five distinct cases surfaced. This argues for the disposition-class "what does done mean" (C-07) being treated as a process issue, not just per-task fixes.

### 5f. Naming inconsistency between snake_case loads and PascalCase supports
Already in audit-brief "things taken as given" but worth flagging that ~5 audits cite this independently. It's not just cosmetic — the inconsistency interacts with future trait introduction (M-007 multi-load-case-fea explicitly).

### 5g. The "additive" framing on top of a fictional foundation
v0.5 PRDs (composite-laminated-shells, structural-stability-buckling, varying-thickness-shells) all claim to be additive on the v0.4 shells PRD, which is itself "deferred" with engine integration absent. Composite-laminated-shells PRD specifically says "the through-thickness integration becomes a sum over plies" — but the current through-thickness integration is **analytical closed-form for constant thickness**, not a sum-over-anything. The "additive" word is doing more work than the codebase supports.

### 5h. Under- and over-classification in the per-PRD audits
- `field-source-kinds.md` is unusually thorough — found 4 DRIFT cases and a PRD-prose-stale issue. The PRD describes an old state of the world.
- `compute-node-infrastructure.md` is rigorous about distinguishing M-014 (FICTION, the producer) from M-015 (FICTION, the registry) — useful precision
- `kinematic-constraints-toplevel.md` mostly flagged DRIFT where some are arguably FICTION (the v0.1 closed-chain-error contract is unimplementable in code; v0.2 replaced it)
- `migration-toolchain.md` correctly skipped as process-only
- `kleene-logic.md` is quite small (9 mechanisms) and almost-all WIRED — a useful counterpoint that not every PRD is broken

The cross-audit consistency is high enough to trust the cluster mapping.

---

**File-only synthesis ends here. The supervisor compares this to the memory-based path.**
