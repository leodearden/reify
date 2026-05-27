# Phase 3 Breadcrumb Map

**Date:** 2026-05-12
**Source:** `## Cross-PRD breadcrumbs` sections in `docs/architecture-audit/findings/*.md` (40 files)
**Method:** Harvest only — no synthesis, no PRD-text reading. Edges are directional (Source PRD → Target PRD). One row per discrete reference. Where a breadcrumb names a Reify-wide pattern (GR-001, task IDs, architecture sections) it is listed in §4 cluster view rather than §1.

---

## 1. Edge list (raw)

Sorted alphabetically by source PRD. Each row is a directed edge; bidirectional references appear as two rows. Targets prefixed with `(non-PRD:)` are non-PRD code or spec references retained because the breadcrumb gave them PRD-shaped weight.

| Source PRD | Target PRD | Mechanism / relationship | Note |
|---|---|---|---|
| a-posteriori-error-estimation | structural-analysis-fea | Source of `ElasticResult`; `Field<X,Y>` TODO(#3117) | M-002, M-005, M-011 hit same field-in-param blocker |
| a-posteriori-error-estimation | structural-analysis-shells | Generalises the same field-shape decision | M-002/M-005/M-011 likely intersect |
| a-posteriori-error-estimation | hex-wedge-meshing | Both extend task #2925 / Gmsh adapter | M-008 |
| a-posteriori-error-estimation | mesh-morphing | Lazy refinement / morph cache invalidation contract; cross-PRD pattern | M-013, M-015 |
| a-posteriori-error-estimation | fea-gui-rendering | v0.3 FEA GUI owns #2961/#2962/#2964 | M-014 |
| a-posteriori-error-estimation | fea-gui-rendering-shells | v0.4 shells GUI | M-014 |
| a-posteriori-error-estimation | multi-load-case-fea | Per-case accuracy guarantees mention reusing the refinement budget | M-009, M-016 |
| a-posteriori-error-estimation | compute-node-infrastructure | Gating v0.3 foundational PRD | M-018 transitive |
| auto-resolution-backtracking | auto-type-param-resolution | Sibling v0.1 PRD; M-002/M-014 likely shared orphans | leave to its own audit |
| auto-resolution-backtracking | (non-PRD:) Spec/Arch — fn-sig type res (task 3440) | M-007 inertness interacts with trait-resolution work | out of scope |
| auto-type-param-resolution | auto-resolution-backtracking | Sibling v0.2 PRD; M-005/M-009/M-010/M-013 duplicate its M-001/M-014/M-002/M-013 | dedupe in Phase 3 |
| auto-type-param-resolution | (non-PRD:) Architecture §6.2-6.4 / SchemaNode | SchemaNode naming mismatch (M-008) likely affects every §6.x-referencing PRD | sweep candidate |
| auto-type-param-resolution | (non-PRD:) Architecture §2.5 (monotonic-feasible) | Phase B feasibility primitive; any reuser implicitly depends | informational |
| auto-type-param-resolution | (non-PRD:) trait-conformance PRD (entity::satisfies_trait_bound) | Same predicate as conformance/checker.rs and trait_typed_param tests | dedupe if audited |
| auto-type-param-resolution | compute-node-infrastructure | Deferred type-substitution (M-013) is also prerequisite for @optimized fn lowering | bundle candidate |
| composite-laminated-shells | structural-analysis-shells | Hard prerequisite — mid-surface, MITC3+, ShellStress, @shell | parent PRD, deferred |
| composite-laminated-shells | structural-analysis-fea | Gates entire FEA stack (ElasticResult/ElasticOptions/solver loop) | composite extends ElasticResult |
| composite-laminated-shells | multi-load-case-fea | "Composes with multi-load-case" for per-load-case envelopes | per-ply/per-criterion envelopes are additional cross-cut |
| composite-laminated-shells | fea-gui-rendering-shells | "Composes with" for per-ply visualisation | sibling deferred |
| composite-laminated-shells | (non-PRD:) structural-analysis-progressive-damage | Seeded follow-on; not filed | hypothetical |
| compute-node-infrastructure | structural-analysis-fea | Task #16 (2924) is this PRD's stated consumer; FEA #4 (ElasticOptions/cacheable_hash) and #1 (Material starter lib) feed M-004 | M-014 transitive on GR-001 |
| compute-node-infrastructure | multi-load-case-fea | Assumes LoadCase/MultiCaseResult runtime ctors AND solve_load_cases @optimized-dispatched | consumer |
| compute-node-infrastructure | persistent-fea-cache | Consumes ComputeNodeData.cache_key (M-004) + PersistentlyCacheable trait | presupposes M-014/M-015 land |
| compute-node-infrastructure | mesh-morphing | Named "Consumer" in PRD; blocked on M-014/M-015/M-016 chain | also covers modal/thermal future consumers |
| deep-dot-chain | (non-PRD:) all-PRDs-with-method-call-syntax | If `a.foo()` is added to AST, dot-chain lint needs a new test | corpus-wide grep candidate |
| deep-dot-chain | (non-PRD:) diagnostic-code naming convention sweep | `W_DEEP_DOT_CHAIN` vs `DeepDotChain` style drift | corpus-wide naming pass |
| deep-dot-chain | shadowing-warning | Linter threshold configurability pattern — shadow lint likely shares the shape | shared LintConfig struct candidate |
| deep-dot-chain | specialization-scope | Same configurability pattern | shared LintConfig candidate |
| fea-gui-rendering | structural-analysis-fea | Owns ElasticResult, result interpolation, @optimized integration, progressive solve framework | direct prerequisites for M-006/7/10/11/12/13/14/18/19; 2924 gates whole PRD |
| fea-gui-rendering | compute-node-infrastructure | Owns tasks 3377-3385 that 2924 chains through | has its own audit slot |
| fea-gui-rendering | multi-load-case-fea | Future user of probe + overlay design; LoadCase/MultiCaseResult transitively block via GR-001 | future consumer |
| fea-gui-rendering | mesh-morphing | Named in PRD §Relationship as composing target for live rendering | coupling point, not blocking |
| fea-gui-rendering | structural-analysis-shells | Future extension for shell-element rendering (mid-surface + thickness) | not blocking v0.3 |
| fea-gui-rendering | (non-PRD:) prd-m6-gui | Field-rendering surface this PRD extends | not audited here |
| fea-gui-rendering-shells | fea-gui-rendering | Owns every reusable GUI primitive; 14/22 mechanisms not WIRED — additive framing assumes aspirational primitives | direct dep on its M-005/M-006/M-007/M-010/M-012 |
| fea-gui-rendering-shells | structural-analysis-shells | Owns kernel-side shell solver, ShellStress.top/mid/bottom, ElasticResult.frame, MPC plumbing, mid-surface extractor, segmentation API | T18-T20 unshipped; this PRD is gating consumer |
| fea-gui-rendering-shells | structural-analysis-fea | Owns ElasticResult, engine integration #2924, @optimized + ComputeNode dispatch | transitive GR-001 via 3426 |
| fea-gui-rendering-shells | compute-node-infrastructure | Owns tasks 3377-3385 that 2924 chains through | |
| fea-gui-rendering-shells | varying-thickness-shells | Thickness heat-map (M-008) becomes meaningful when thickness varies | v0.5 |
| fea-gui-rendering-shells | composite-laminated-shells | Future per-ply stress display extends top/mid/bottom toggle into per-ply selector | out-of-scope here |
| fea-gui-rendering-shells | mesh-morphing | Future composing target for live rendering; mid-surface morphs alongside body | not blocking |
| fea-gui-rendering-shells | persistent-naming-v2 | Mid-surface entities (face/edge naming for BC attachment) tracked there | shell-normal overlay would benefit but doesn't require |
| field-source-kinds | imported-field-source | Owns the actual Imported pipeline (tasks 2667/2668) | M-023 points there via task 2344 only — discoverability gap |
| field-source-kinds | (non-PRD:) sampled-field-source PRD (missing) | In-code commentary cites "esc-2341-149 steward decision" for 5-key surface; no PRD owns evolved design | candidate cross-PRD coordination gap |
| forall-statement-form | (non-PRD:) `chain` statement form PRD/spec | Appears in spec §5.4 indirectly; out of scope | |
| forall-statement-form | (non-PRD:) purposes PRD | `@forall` over purpose bodies currently rejected | future PRD must coordinate |
| forall-statement-form | (non-PRD:) SchemaNode PRD | SchemaNode runtime re-elaboration referenced; never built as Rust struct — engine_edit.rs grew re-emission in-place | watch for shape collision |
| forall-statement-form | (non-PRD:) connect PRD | `connector_sub` auto-creation + `frame_constraint` generation owned by connect PRD; deferred Connect arm explicitly drops both | out of scope |
| freshness-4-variant | compute-node-infrastructure | Independently flagged `Freshness::Pending` reuse as leading candidate for ComputeNode running-state | M-013 there |
| freshness-4-variant | node-trait-composition | Owns `still_refining=true` producer side (PROGRESSIVE node trait); without it landing M-007 here remains test-only | misattributed comment redirect |
| freshness-4-variant | persistent-fea-cache | If persistent caching stores ResultRef::of_hash across sessions, opaque-ID assumption (M-002) becomes load-bearing | currently §Out of scope |
| geometry-traits | field-source-kinds | Imported field source kind interacts with conformance | Phase 2/3 check |
| geometry-traits | topology-selectors | Solvespace-style attribute-persistent conformance attestations cited as "ties to feature-tag work" | §Out of scope |
| geometry-traits | stdlib-trait-breadth | Task 2347 audited stdlib trait inheritance; confirmed Watertight : Closed + Manifold | sibling work; doesn't block |
| geometry-traits | multi-kernel | Deferred to v0.2 | |
| geometry-traits | (non-PRD:) Value::TraitTag / type-as-value | Prerequisite for generic `conforms<T,R>`; not present in reify-types today | transitive dependency for any "trait names as values" PRD |
| hex-wedge-meshing | structural-analysis-fea | Owns non-swept `mesh_surface_to_volume_with_diagnostics` realization wiring (M-018) | shared upstream gap |
| hex-wedge-meshing | mesh-morphing | Named downstream consumer of `Engine::swept_kind_table()` (M-004) | orphan-accessor pattern test |
| hex-wedge-meshing | fea-gui-rendering | Named consumer of "hex/wedge meshed" status badge | same orphan-accessor pattern likely |
| hex-wedge-meshing | compute-node-infrastructure | Implicitly upstream: @optimized solver::elastic_static needs to read force_tet/require_hex_wedge from ElasticOptions at ComputeNode-dispatch | doubly-blocked by GR-001 |
| hex-wedge-meshing | a-posteriori-error-estimation | Refinement decisions are element-type-aware in spirit; once M-017 lights up, mixed hex/wedge/tet flow into Z-Z indicator | cross-cutting, invisible today |
| imported-field-source-hdf5-csv | imported-field-source | v0.2 base case extends; decomp tasks 2665-2669 marked `done`-with-merge-commits but eval-side glue not wired | downstream auditor reading v0.2 PRD would reasonably conclude works |
| imported-field-source-hdf5-csv | structural-analysis-fea | Explicitly lists "v0.2 imported-field-source has shipped" as pre-condition; if M-001 correct that pre-condition is not met | |
| imported-field-source-hdf5-csv | money-dimension | Cited by v0.3 PRD as providing unit-literal grammar that `Length(mm)` reuses | M-007 — clarify whether syntax exists |
| imported-field-source-hdf5-csv | per-purpose-tolerance | Listed as pre-condition (status `deferred to v0.2`) | coupling point for tolerance promises on imported data |
| imported-field-source-hdf5-csv | multi-kernel | OpenVDB ingestion is key motivator for OpenVDB sub-kernel; multi-kernel cites "OpenVDB unblocks the `imported` field source" | |
| imported-field-source | persistent-fea-cache | Depends on content-addressed cache keys; M-004's stub content hash would let two imports with different paths collide | cross-cite |
| imported-field-source | multi-kernel | Owns OpenVDB sub-kernel adapter (#2645 done); natural home for dispatcher indirection M-008 might use | |
| imported-field-source | structural-analysis-fea | Invokes FieldImportProvenance indirectly via arch §14.5 Input-occurrence contract | downstream consumer when imported fields go live for FEA stress-field round-trip |
| imported-field-source | multi-load-case-fea | Same arch §14.5 indirect invocation | downstream consumer |
| imported-field-source | (non-PRD:) tolerance_promise.rs / tolerance_combine.rs | Shares Gate 4 filter with build_field_import_provenance | informational coupling |
| imported-field-source | imported-field-source-hdf5-csv | Follow-on PRD (HDF5/CSV deferred); same wire-site question reappears | |
| kinematic-constraints-toplevel | kinematic-constraints-v02 | Owns closed-chain solver + planar/spherical/cylindrical/fixed/screw/gear/RnP joints; substantial v0.2 work landed in same files; M-007 shows v0.2 subsumed v0.1 without retiring v0.1 contract | separate audit batch |
| kinematic-constraints-toplevel | (non-PRD:) GR-001 follow-up | "Mechanism/Snapshot/Joint are types" same shape as GR-001's "Material/LoadCase are types"; kinematic sidesteps via Value::Map+kind tag | v0.2 fix may or may not retrofit |
| kinematic-constraints-toplevel | (non-PRD:) FFI task #2530 | When wired lifts center_of_mass/bounding_box from PARTIAL to WIRED | not audited |
| kinematic-constraints-toplevel | (non-PRD:) OCCT shape-transform | M-019/M-020/M-021 DRIFT unblocked by GeometryOp::ApplyTransform or per-pair on-the-fly OCCT transforms | out of scope per PRD task 8 |
| kinematic-constraints-v02 | (non-PRD:) mechanism-clearance / printer-build dogfood PRD | Snapshot Map's `{bodies, free_values, kind}` shape consumed by GUI clearance/visualisation; adding is_singular is shape-versioning concern | |
| kinematic-constraints-v02 | structural-analysis-fea | transform_at/compose/log/exp helpers (task 2583) now used by FEA-side rigid-body machinery | cross-cutting |
| kinematic-constraints-v02 | (non-PRD:) belt/cable physics PRD | Coupling specialisations gear/screw/RnP overlap conceptually | PRD explicit out-of-scope |
| kleene-logic | (non-PRD:) spec §15/§16 + guards PRD | `implies` operator advertised with no parser/eval backing; affects "static implication check" in spec §8.10 | drift |
| kleene-logic | (non-PRD:) undef-propagation umbrella PRD (§9.2) | This PRD covers §9.2.3 + §9.2.6 only | Phase 3 may want complete undef-propagation audit |
| kleene-logic | (non-PRD:) tree-sitter operator spelling | Grammar uses `&&`/`\|\|`/`!`; spec uses `and`/`or`/`not` | independent drift, shares root cause with M-002 |
| match-block-decls | shadowing-warning | §6.4 same-name match decls explicitly distinct from §8.8 trait merge collisions | coupling on cluster-shape changes |
| match-block-decls | auto-type-param-resolution | References §8.10 — same "existing implication check" presumed by both | shared dep if check is a gap |
| match-block-decls | specialization-scope | MatchArmDeclGroup recursion wired into walk_specialization_scope_members | already covered by spec-scope validation tests |
| match-block-decls | forall-statement-form | Same shape gap (grammar rule + tree-sitter lowering for statement-form) — different outcome despite similar pattern | |
| mesh-morphing | persistent-naming-v2 | TopologyAttributeTable + CorrespondenceMap wired (task 2590, 2652); vertex_to_vertex documented-empty hole | would surface in PNv2 audit too |
| mesh-morphing | structural-analysis-fea | Task 2925 (ReprKind::VolumeMesh) is realization-path gate; morph() operates on VolumeMesh directly | mesh-morph does NOT transitively block on GR-001 |
| mesh-morphing | persistent-fea-cache | PRD says earlier "caching morphed meshes with morph provenance" should be removed | did not verify; out of scope |
| mesh-morphing | (non-PRD:) mesh-morph-nearest-cached v0.4 stub | PRD names this stub as deferred follow-on; file does NOT exist in docs/prds/v0_4/ | minor ORPHAN-shaped issue |
| mesh-morphing | a-posteriori-error-estimation | "Mesh-morphing composes with parameter values during lazy-refinement at decision time" | not audited |
| mesh-morphing | (non-PRD:) GUI-rendering PRD | Morph-badge visualization tracked there | not audited |
| migration-toolchain | (non-PRD:) pragma syntax PRD / spec §14.2 | Pragma-parsing no-op shared; no active gap | |
| money-dimension | (non-PRD:) future cost-anything PRDs | `Buy`/`Costed` traits and `Money` vs `Scalar<Money>` deviation #1 surface in any procurement/BOM PRD | stdlib-wide convention |
| money-dimension | field-source-kinds | DimensionMismatch diagnostic infra (M-011) cited at PRD §4 suppression | consumer |
| money-dimension | structural-analysis-fea | Any FEA PRD adding new dimensions leans on same plumbing | consumer |
| money-dimension | multi-load-case-fea | `.sum` aggregation alongside count/keys/values; envelope worst-case probably wants richer aggregations | out of scope here |
| money-dimension | (non-PRD:) all-PRDs PRD-vs-task status drift pattern | Header still labels 2379-2383 as "in-flight" though all six landed | pattern likely affects other PRDs |
| multi-kernel | per-purpose-tolerance | Owns partial-order tolerance vocabulary used by cache key (M-006); shares 500ms long-chain threshold | |
| multi-kernel | persistent-naming-v2 | Task 9 owns real propagate_attributes body for ManifoldKernel (M-018) | |
| multi-kernel | imported-field-source | Owns actual consumer for OpenVDB ingestion (M-011) | |
| multi-kernel | imported-field-source-hdf5-csv | Same | |
| multi-kernel | structural-analysis-fea | Depends on VolumeMesh ReprKind (M-001 drift) for Gmsh surface→volume meshing | v0.3 extension |
| multi-kernel | compute-node-infrastructure | Adjacent dispatch story for stdlib fn-as-ComputeNode; analogous "infrastructure scaffold without runtime activation" pattern | |
| multi-kernel | (non-PRD:) Architecture §10.5 (stack/patchwork) | Canonical document; patterns unimplemented (M-014, M-015) | |
| multi-kernel | (non-PRD:) Architecture §16 open Q #8 | PRD claims to resolve via compile-time inventory registration; structurally correct (M-003) | |
| multi-load-case-fea | structural-analysis-fea | Owns solve_elastic_static (M-005), @optimized registration (M-006/task 2924), ElasticResult/ElasticOptions (done), Field-max/min (#2913 done), FEA analytical suite #2928, bracket #2929, diag infra #2929 | strict consumer |
| multi-load-case-fea | compute-node-infrastructure | Owns ComputeNode dispatch surface (M-006), cache-key composition + cache reuse (M-014) | Phase 3 should treat volume-mesh cache reuse claim as inherited |
| multi-load-case-fea | fea-gui-rendering | Owns FEA-mode toggle (done #2961), stress contour (#2962), visual regression #2954-2958 | strict extension |
| multi-load-case-fea | per-purpose-tolerance | Referenced indirectly via cg_tolerance and mesh-size derivation in ElasticOptions | not exercised beyond defaults |
| multi-load-case-fea | structural-analysis-shells | Listed as compositional in PRD §"Relationship to other PRDs"; not consumed at runtime | |
| multi-load-case-fea | hex-wedge-meshing | Compositional only | |
| multi-load-case-fea | mesh-morphing | Compositional only | |
| multi-load-case-fea | a-posteriori-error-estimation | Compositional only | |
| node-trait-composition | freshness-4-variant | Tasks 3 and 5 depend on Freshness PRD; task 2335 freshness propagation walk is gating | freshness audit should cross-check Pending/Intermediate semantics |
| node-trait-composition | compute-node-infrastructure | ComputeNode runtime struct exists; "missing struct counterparts" deferral references future ComputeNode/SchemaNode/SourceNode work | trait-set assignments may follow |
| node-trait-composition | structural-analysis-fea | Worked example node (warm_startable + progressive + committable) targets FEA solvers; @optimized registration is actual consumer that needs M-005 (per-node traits attachment) | |
| node-trait-composition | (non-PRD:) reify-implementation-architecture §7.5 | Cancellation behaviour by priority; M-008 documents gap | |
| per-purpose-tolerance | multi-kernel | M-008 (per-stage budget) and M-011 (long-chain diagnostic) depend on multi-kernel dispatch wired into op execution (task 2642) | both scaffolding until then |
| per-purpose-tolerance | imported-field-source | M-009/M-010 share Input occurrence boundary contract (arch §14.5); field_import_provenance.rs Provenance builder is task-5 there; tolerance_guarantee route reinforces M-009 DRIFT | |
| per-purpose-tolerance | structural-analysis-fea | May consume purpose-active RepresentationWithin on subjects-with-member-access (e.g. bracket.fea_subject); M-001 MVP scope clip silently drops these | Phase 3 should check FEA decomps |
| per-purpose-tolerance | structural-analysis-shells | Same | |
| persistent-fea-cache | structural-analysis-fea | Provides in-memory ComputeNode cache layer this PRD persists beneath; gate is task 2924 + 9 prereqs 3377-3385 | M-011 cannot land until ComputeNode dispatch ships |
| persistent-fea-cache | compute-node-infrastructure | Owns compute_cache_key.rs (P3.4) + upstream options_hash filtering contract (ElasticOptions::cacheable_hash) | M-012's drift routes back here |
| persistent-fea-cache | mesh-morphing | Explicit non-coupling: morphed meshes NOT persisted | design boundary, not gap |
| persistent-fea-cache | fea-gui-rendering | "Opening a saved project hits cache → first-frame stress contour" composes with M-011 + M-018 | same gate |
| persistent-fea-cache | structural-analysis-shells | Transitively gated; shells results would also impl PersistentlyCacheable per "generic from day one" promise (M-001), but no shells-side impl yet | informational |
| persistent-naming-v2 | multi-kernel | PRD line 70 cross-references task 2295; Manifold concrete impl (M-018) gated on that PRD | if multi-kernel deferred MeshGL walk, "first concrete impl" claim needs revisiting |
| persistent-naming-v2 | topology-selectors | Task 2699 reopen_reason cites 11 missing dispatch arms in try_eval_topology_selector; PNv2 design depends on fallback being real, but neither PRD owns it | |
| persistent-naming-v2 | structural-analysis-shells | T20 mid-surface naming sub-vocabulary (M-011) is only out-of-band Role extension shipped | demonstrates extension pattern |
| persistent-naming-v2 | mesh-morphing | Task 2939 (Stage B persistent-naming bijection check) consumes v0.2 attribute table via morph_stage_b.rs | first downstream consumer beyond resolver tests |
| pragmas | multi-kernel | M-017 (kernel scope drift) and M-016 (unread kernel_pragma) gated by this v0.2 PRD; v0.1 accepts v0.2 kernel idents but doesn't dispatch | half-finished surface |
| pragmas | migration-toolchain | M-012 (declared_version write-only) gates on this v0.2 PRD; §14.2 auto-migration tool explicitly deferred | |
| pragmas | per-purpose-tolerance | Likely consumer for block-level `#precision` (currently warned-and-ignored); related to M-011 | |
| pragmas | reify-doc-tool | Doc generator PRD is primary consumer for declared_version/solver_pragma/kernel_pragma/default_tolerance | without it M-012/M-016 stay write-only |
| reify-doc-tool | pragmas | Reads #version (M-020), #precision, #solver, #kernel; CompiledModule fields exist | if pragma PRD changes lowered shape, doc model breaks |
| reify-doc-tool | (non-PRD:) all v0.1–v0.4 PRDs adding declaration kinds | Every PRD adding a declaration kind needs ItemKind reflection (10 variants currently) | future-extension pattern |
| shadowing-warning | match-block-decls | Referenced by PRD §Background; lint's MatchArmDeclGroup carve-out (M-011); §"Notes" defers manual where{}else{} shadowing-vs-duplicate semantics | coupling, no contradiction |
| shadowing-warning | specialization-scope | Home of SubDecl.body parser feature on which M-016 gated; specialization_scope_check.rs called immediately after shadow_lint::lint_module | validates structural rules, not shadowing |
| shadowing-warning | deep-dot-chain | Sibling lint; pioneered FrameStack allocation-avoidance pattern and MAX_EXPR_DEPTH guard; PascalCase diagnostic-code wire-string pattern shared | cross-cutting lint hygiene |
| solver-hint-payloads | reify-doc-tool | Owns M-011 (stdlib docs page rendering); hint-payloads AC #3 cannot be met until that surface ships | only missing piece is stdlib-walking |
| solver-hint-payloads | compute-node-infrastructure | FICTION pattern for M-008/M-009 mirrors GR-001 — "PRD preamble claims integration done; code only stores the artefact" | fold into same disposition group |
| solver-hint-payloads | structural-analysis-fea | Same | |
| solver-hint-payloads | (non-PRD:) task 2455 (deferred) | const-vs-fn drift in M-004 bookmarked here | cross-cutting with future top-level const work |
| solver-hint-payloads | (non-PRD:) auto-param resolution / concurrent.rs:263-466 | Natural consumer site for M-008 | location noted, not followed |
| specialization-scope | match-block-decls | Owns grammar/producer side of MatchArmDeclGroup (task 2372); walker integration with spec-scope lives downstream | if grammar half lands M-010 PARTIAL → WIRED |
| specialization-scope | forall-statement-form | Owns grammar/producer side of forall; future `forall ... : sub <body>` would interact with M-011 | today's grammar restricts forall body |
| specialization-scope | (non-PRD:) GR-001 pattern parallel | M-002 and M-010 structurally identical to GR-001 — consumer half fully built and tested, producer half silently absent with no task ID | same failure mode |
| stdlib-trait-breadth | geometry-traits | M-007/M-013 (Solid + geometry-driven Physical shape) intersects findings; half_space/extrude_infinite FICTION blocks "Solid as trait-bound-bearing value" | Phase 3 group these |
| stdlib-trait-breadth | money-dimension | M-012 dimensional types transitively gates every trait declaration; also cited there | task 3115 is single choke point |
| stdlib-trait-breadth | per-purpose-tolerance | M-012 implicitly cited | |
| stdlib-trait-breadth | structural-analysis-fea | M-009/M-008 (one-slot-many-traits material composition) is pattern FEA PRDs lean on heavily once GR-001 lands | safe in isolation today |
| stdlib-trait-breadth | multi-load-case-fea | Same | |
| structural-analysis-fea | multi-load-case-fea | GR-001 transitively touches | assumes Steel_AISI_1045() evaluates to usable runtime value |
| structural-analysis-fea | structural-analysis-shells | GR-001 transitively touches; substantial code landed ahead of solid-FEA validation (MITC3+, MacNeal-Harder, ShellStress, shell_threshold/shell_force) | inversion of expected ordering flagged |
| structural-analysis-fea | mesh-morphing | GR-001 transitively touches; ElasticResult/LoadCase consumers | |
| structural-analysis-fea | a-posteriori-error-estimation | GR-001 transitively touches | |
| structural-analysis-fea | compute-node-infrastructure | Owns M-002/M-003/M-004/M-005/M-006/M-007/M-008 (P3.1-P3.6, tasks 3380/3381/3382/3379/3383/3385); FEA PRD assumes all six landed | |
| structural-analysis-fea | per-purpose-tolerance | Owns tolerance-scope machinery FEA cache + significance filter consume | |
| structural-analysis-fea | topology-selectors | Owns bracket.face("top") machinery load/support constructors target; currently opaque-pass-through | |
| structural-analysis-fea | hex-wedge-meshing | P1 hex + P1 wedge elements landed in solver crate; force_tet/require_hex_wedge knobs added to ElasticOptions | same "landed ahead" observation |
| structural-analysis-fea | (non-PRD:) Task #3117 (Field<X,Y> in param) | Umbrella for M-022 — touches all six field-slots in solver_elastic.ri | |
| structural-analysis-shells | (non-PRD:) GR-001 | Gates runtime form of ElasticResult, ShellStress, ElasticOptions, ShellForce enum; Map-tagged builtin-ctor path is operational substitute | structure-def syntax parser-only |
| structural-analysis-shells | structural-analysis-fea | Task 3426→2924 chain; M-018 routes through solve_elastic_static which has no stdlib fn decl; inherits FEA M-001 verbatim | |
| structural-analysis-shells | compute-node-infrastructure | T18 extraction cached as ComputeNode keyed on geometry hash + extraction options; depends on @optimized→ComputeNode lowering for fn context that FEA M-002 says is PARTIAL | |
| structural-analysis-shells | multi-kernel | Owns OpenVDB FFI follow-up M-025 depends on; PRD "Pre-conditions for activating" line 42 — gate is half-open | |
| structural-analysis-shells | persistent-naming-v2 | M-020 mid-surface naming structurally ready but folding into OCCT-handle-keyed TopologyAttributeTable deferred to T18; Role::MidSurfaceEdge + FeatureId::derived_mid_surface present in reify-types | cross-PRD hook landed |
| structural-analysis-shells | auto-resolution-backtracking | M-026 (`param thickness : Length = auto`) — type-arg-position `auto:` gap (B1 chain) owned there; value-position `= auto` at param-default is grammar-supported and now covered by `docs/prds/auto-binding-site-positions.md` | ownership split: value-position → auto-binding-site-positions; type-arg-position → auto-resolution-backtracking |
| structural-analysis-shells | fea-gui-rendering-shells | Sibling PRD; PRD §85 defers GUI rendering to that PRD | |
| structural-analysis-shells | mesh-morphing | PRD claims mid-surface morphs alongside body; today no mid-surface morphing code path; depends on M-018 first | |
| structural-analysis-shells | a-posteriori-error-estimation | PRD claims Z-Z indicator extends to shell elements with through-thickness sampling | not verified |
| structural-analysis-shells | hex-wedge-meshing | Sibling PRD with overlapping thin-body motivation; "partial overlap" | out of scope here |
| structural-analysis-shells | multi-load-case-fea | PRD says shells participate same way solids do; envelope reductions over Map-tagged shell ElasticResult.stress.top requires GR-001 — currently fiction | |
| structural-stability-buckling | structural-analysis-fea | Direct foundation; FEA M-001/M-002 are FICTION; buckling cannot begin until FEA lands and validates | Material starter + Load/Support drift inherited |
| structural-stability-buckling | structural-analysis-shells | Critical foundation; shells must ship first (slender = thin = shell-modeled); shell-element K_g assembly extends MITC3+ kinematics with stress-stiffness terms not present | |
| structural-stability-buckling | multi-load-case-fea | "Buckling load factor per load case is a natural envelope"; composition unspecified; presumably MultiCaseBucklingResult parallel to MultiCaseResult | compounds parametric-instantiation surface |
| structural-stability-buckling | fea-gui-rendering | Mode-shape rendering (M-013) needs deformed-shape pipeline + new animation primitive | |
| structural-stability-buckling | fea-gui-rendering-shells | Same | |
| structural-stability-buckling | compute-node-infrastructure | solve_buckling @optimized dispatch follows same M-014/M-015/M-016 chain (all FICTION); significance-filter allowlist (M-010 there hardcoded "solver::elastic_static") would need to expand | |
| structural-stability-buckling | (non-PRD:) structural-analysis-modal (unfiled) | Eigenvalue solver would be shared infrastructure; PRD does not call out generalization explicitly | |
| structural-stability-buckling | mesh-morphing | Imperfection-seeding (M-014) is special case of nodal-position morphing; cross-pollination possible but not designed | |
| topology-selectors | geometry-traits | PRD §Dependencies declares dep on Bounded argument check for moment_of_inertia/center_of_mass; diagnostic wired (M-009=PARTIAL) but no caller in topology-selectors consumes it (M-016) | disposition of geometry-traits M-006 gates user-visibility |
| topology-selectors | persistent-naming-v2 | Explicitly cited v0.2 successor (PRD §Out of scope); shipped per fused-memory 0d38a0c8 (task 2652) | observable v0.1/v0.2 coexistence |
| topology-selectors | field-source-kinds | Imported geometry selectors out of scope — same "imported geometry requires own naming surface" observation as PNv2 | deferred |
| topology-selectors | (non-PRD:) #318/#319 PRDs (older filtered selectors/point-membership) | Reference pattern; handoff with #318 "filtered list selectors over whole solid" is clean at kernel layer | |
| topology-selectors | (non-PRD:) Ad-hoc port selectors #249 (CompiledAdHocPort) | PRD §Background cites as reference implementation; quick check shows CompiledAdHocPort is its own type — may be siblings rather than shared core | Phase 3 verification candidate |
| varying-thickness-shells | composite-laminated-shells | v0.5+ composes — varying total thickness × variable ply count = union | per PRD §"Out of scope" |
| varying-thickness-shells | mesh-morphing | M-012 references | v0.3 deferred |
| varying-thickness-shells | fea-gui-rendering-shells | Thickness-display mode for varying-thickness rendering; GUI doesn't yet render shells | |
| varying-thickness-shells | imported-field-source-hdf5-csv | Needed for imported_thickness_map field producer (M-006) | |
| varying-thickness-shells | a-posteriori-error-estimation | Z-Z indicator + refinement strategy interacts with M-008 thickness-gradient refinement | |
| varying-thickness-shells | persistent-naming-v2 | Mid-surface entity naming exists in reify-shell-extract; varying-thickness adds no new naming demand | |
| warm-state-eviction | compute-node-infrastructure | Owns ComputeNode addition to NodeId; M-008 NodeId::Compute donation hook + M-007 checkout-for-ComputeNode fall-back gated on that PRD | |
| warm-state-eviction | structural-analysis-fea | Task #14 produces CgWarmState; task #16 (2924) supposed to wire through pool via @optimized | M-005 (wall-clock measurement) + M-013 (producer→pool round-trip) are structural prereqs |
| warm-state-eviction | multi-load-case-fea | Assumes FEA warm state survives across edits; until M-013 wired no warm state actually reaches pool | "expensive FEA solves stay warm" may surprise |
| warm-state-eviction | persistent-fea-cache | Same | |
| warm-state-eviction | persistent-naming-v2 | Owns path-based identity for non-Value variants; M-008 partial state (index-based NodeIds) upstream-blocked here | |

**Total rows:** 198 (158 PRD→PRD edges + 40 non-PRD references such as architecture-sections, future-unfiled PRDs, task-IDs, and spec sections retained for completeness)

---

## 2. Inbound-degree leaderboard

Counts PRD→PRD edges only — based on case-sensitive substring match of the target slug inside each other PRD's `## Cross-PRD breadcrumbs` section. Multiple mentions of the same target in a single source are counted once.

| Rank | Target PRD | Inbound refs | Citing PRDs (all) |
|---|---|---|---|
| 1 | structural-analysis-fea | 17 | a-posteriori-error-estimation, composite-laminated-shells, compute-node-infrastructure, fea-gui-rendering, fea-gui-rendering-shells, hex-wedge-meshing, imported-field-source-hdf5-csv, imported-field-source, mesh-morphing, multi-kernel, multi-load-case-fea, node-trait-composition, per-purpose-tolerance, persistent-fea-cache, stdlib-trait-breadth, structural-stability-buckling, warm-state-eviction |
| 2 | compute-node-infrastructure | 13 | a-posteriori-error-estimation, fea-gui-rendering, fea-gui-rendering-shells, freshness-4-variant, hex-wedge-meshing, multi-kernel, multi-load-case-fea, node-trait-composition, persistent-fea-cache, structural-analysis-fea, structural-analysis-shells, structural-stability-buckling, warm-state-eviction |
| 3 | structural-analysis-shells | 11 | a-posteriori-error-estimation, composite-laminated-shells, fea-gui-rendering, fea-gui-rendering-shells, mesh-morphing, multi-load-case-fea, per-purpose-tolerance, persistent-fea-cache, persistent-naming-v2, structural-analysis-fea, structural-stability-buckling |
| 4 (tie) | multi-load-case-fea | 10 | a-posteriori-error-estimation, composite-laminated-shells, compute-node-infrastructure, fea-gui-rendering, imported-field-source, stdlib-trait-breadth, structural-analysis-fea, structural-analysis-shells, structural-stability-buckling, warm-state-eviction |
| 4 (tie) | mesh-morphing | 10 | a-posteriori-error-estimation, fea-gui-rendering, fea-gui-rendering-shells, hex-wedge-meshing, multi-load-case-fea, persistent-fea-cache, persistent-naming-v2, structural-analysis-shells, structural-stability-buckling, varying-thickness-shells |
| 4 (tie) | fea-gui-rendering | 10 | a-posteriori-error-estimation, composite-laminated-shells, fea-gui-rendering-shells, hex-wedge-meshing, mesh-morphing, multi-load-case-fea, persistent-fea-cache, structural-analysis-shells, structural-stability-buckling, varying-thickness-shells |
| 7 (tie) | persistent-naming-v2 | 7 | fea-gui-rendering-shells, mesh-morphing, multi-kernel, structural-analysis-shells, topology-selectors, varying-thickness-shells, warm-state-eviction |
| 7 (tie) | multi-kernel | 7 | geometry-traits, imported-field-source-hdf5-csv, imported-field-source, per-purpose-tolerance, persistent-naming-v2, pragmas, structural-analysis-shells |
| 9 (tie) | per-purpose-tolerance | 6 | imported-field-source-hdf5-csv, multi-kernel, multi-load-case-fea, pragmas, stdlib-trait-breadth, structural-analysis-fea |
| 9 (tie) | fea-gui-rendering-shells | 6 | a-posteriori-error-estimation, composite-laminated-shells, mesh-morphing, structural-analysis-shells, structural-stability-buckling, varying-thickness-shells |

**Other notable** (below top 10):
- imported-field-source 5 — field-source-kinds, imported-field-source-hdf5-csv, multi-kernel, per-purpose-tolerance, varying-thickness-shells
- a-posteriori-error-estimation 5 — hex-wedge-meshing, mesh-morphing, multi-load-case-fea, structural-analysis-shells, varying-thickness-shells
- persistent-fea-cache 4; hex-wedge-meshing 4
- topology-selectors 3; imported-field-source-hdf5-csv 3; field-source-kinds 3
- specialization-scope 2; money-dimension 2; match-block-decls 2; geometry-traits 2; forall-statement-form 2; composite-laminated-shells 2; auto-type-param-resolution 2; auto-resolution-backtracking 2
- varying-thickness-shells 1; stdlib-trait-breadth 1; shadowing-warning 1; reify-doc-tool 1; pragmas 1; migration-toolchain 1; freshness-4-variant 1; deep-dot-chain 1
- **Zero inbound:** warm-state-eviction, structural-stability-buckling, solver-hint-payloads, node-trait-composition, kleene-logic, kinematic-constraints-v02, kinematic-constraints-toplevel (7 PRDs)

---

## 3. Reciprocal / contested edges

Pairs where BOTH directions of an edge exist in §1. These are highest-value for Phase 3 discussion.

| PRD A | PRD B | A→B mechanism | B→A mechanism | Note |
|---|---|---|---|---|
| structural-analysis-fea | multi-load-case-fea | GR-001 transitively touches multi-load-case via ElasticMaterial/ElasticResult/LoadCase | Multi-load-case is strict consumer of FEA's solve_elastic_static / @optimized / ElasticResult / Field reductions | Mutual heavy dependence; multi-load-case is the downstream, FEA reverse-cites only because GR-001 propagates upward. Asymmetric but bidirectional. |
| structural-analysis-fea | structural-analysis-shells | GR-001 transitively touches shells; "inversion of expected ordering" flagged (shells code landed ahead of solid-FEA validation) | Shells inherits FEA M-001 verbatim via task 3426→2924 chain | Strongest contested edge: each PRD audit notes the other should land first. Schedule ambiguity. |
| structural-analysis-fea | hex-wedge-meshing | P1 hex + P1 wedge already landed in solver crate ahead of FEA validation | Hex/wedge PRD assumes FEA owns the non-swept tet realization (M-018) — shared upstream gap | Both note the other's premature landing pattern |
| structural-analysis-fea | mesh-morphing | GR-001 transitively touches mesh-morphing | Mesh-morphing does NOT block on GR-001 — composes solver primitives directly | One side asserts transitive block; the other explicitly denies. Mild contradiction worth Phase 3 resolution. |
| structural-analysis-fea | a-posteriori-error-estimation | GR-001 transitively touches | A-posteriori M-002/5/11 hit same `TODO(field-in-param)` blocker as FEA's M-022 | Shared blocker, agreed |
| compute-node-infrastructure | multi-load-case-fea | Compute-node-infra notes consumer assumes LoadCase/MultiCaseResult ctors + solve_load_cases @optimized | Multi-load-case treats compute-node-infra as owner of dispatch surface (M-006) + cache-key (M-014) | Clean directional pair (consumer/owner symmetry) |
| compute-node-infrastructure | mesh-morphing | Compute-node-infra names mesh-morph as Consumer | Mesh-morphing notes morph composes solver primitives directly (does not route through @optimized) | Mild contradiction on whether morph IS a compute-node-infra consumer |
| compute-node-infrastructure | persistent-fea-cache | Compute-node-infra notes persistent-cache presupposes M-014/M-015 land | Persistent-fea-cache notes M-012 drift routes back to compute-node-infra | Clean owner/consumer pair |
| compute-node-infrastructure | structural-analysis-fea | Compute-node-infra notes FEA task #16 (2924) is stated consumer | FEA notes compute-node-infra owns M-002/3/4/5/6/7/8 (P3.1-P3.6) | Clean owner/consumer pair — most-cited owner relationship |
| fea-gui-rendering | structural-analysis-fea | FEA-GUI says FEA owns ElasticResult etc. (direct prerequisite) | FEA notes nothing back, except via GR-001 → multi-load-case chain | Mostly one-way |
| fea-gui-rendering | mesh-morphing | FEA-GUI says mesh-morph is composing target | Mesh-morphing says morph-badge visualization tracked under GUI rendering PRD | Reciprocal acknowledgement of GUI coupling |
| fea-gui-rendering-shells | structural-analysis-shells | FEA-GUI-shells notes T18-T20 unshipped; this PRD is gating consumer | Shells PRD §85 defers GUI rendering to fea-gui-rendering-shells | Reciprocal "you ship first / no you ship first" pattern |
| fea-gui-rendering-shells | varying-thickness-shells | FEA-GUI-shells notes M-008 thickness heat-map becomes meaningful when thickness varies | Varying-thickness notes thickness-display mode for varying-thickness rendering | Reciprocal feature-link |
| fea-gui-rendering-shells | mesh-morphing | FEA-GUI-shells notes mid-surface morphs alongside body | Mesh-morphing notes morph-badge tracked under GUI PRD | Reciprocal feature-link |
| fea-gui-rendering-shells | composite-laminated-shells | FEA-GUI-shells notes per-ply display extends top/mid/bottom toggle | Composite notes "composes with fea-gui-rendering-shells" for per-ply visualisation | Reciprocal feature-link |
| persistent-naming-v2 | mesh-morphing | PNv2 notes mesh-morph task 2939 is first downstream consumer beyond resolver tests | Mesh-morphing notes vertex_to_vertex documented-empty hole would surface in PNv2 audit too | Reciprocal: documented-known-hole, agreed |
| persistent-naming-v2 | structural-analysis-shells | PNv2 notes T20 mid-surface naming sub-vocab is only out-of-band Role extension shipped | Shells notes M-020 mid-surface naming structurally ready but folding into OCCT-handle table deferred to T18 | Reciprocal: PRD-hook landed, integration deferred |
| persistent-naming-v2 | multi-kernel | PNv2 notes multi-kernel owns task 2295 for Manifold concrete impl | Multi-kernel notes PNv2 task 9 owns real propagate_attributes body for ManifoldKernel | Reciprocal: each PRD says the OTHER owns the manifold propagation impl. Genuine contested ownership — Phase 3 must assign. |
| topology-selectors | geometry-traits | Topology-selectors notes dep on Bounded check but no caller consumes diagnostic | Geometry-traits notes topology-selectors PRD cites "ties to feature-tag work" §Out of scope | Reciprocal acknowledgement of selector-trait coupling |
| topology-selectors | persistent-naming-v2 | Topology-selectors cites PNv2 as v0.2 successor, shipped | PNv2 cites topology-selectors task 2699 reopen_reason — neither PRD owns the fallback | Reciprocal AND contested: neither PRD owns the missing dispatch arms |
| imported-field-source | imported-field-source-hdf5-csv | imported-field-source notes hdf5-csv is follow-on; same wire-site question | hdf5-csv extends imported-field-source as v0.2 base | Reciprocal base/extension pair |
| imported-field-source | multi-kernel | imported-field-source notes multi-kernel owns OpenVDB adapter and is natural home for dispatcher indirection | multi-kernel notes imported-field-source owns actual consumer for OpenVDB ingestion (M-011) | Reciprocal: each side says the OTHER owns the dispatcher/consumer boundary. Genuine contested ownership. |
| shadowing-warning | match-block-decls | Shadowing-warning notes match-block-decls §"Notes" defers manual where{}else{} shadowing-vs-duplicate semantics | Match-block-decls notes shadowing-warning §6.4 explicitly distinct from §8.8 trait merge | Reciprocal: coupling, no contradiction |
| shadowing-warning | specialization-scope | Shadowing-warning notes spec-scope is home of SubDecl.body parser feature on which M-016 gated | Specialization-scope notes match-block-decls grammar interaction (NOT shadowing-warning specifically) | One-way only — listed for visibility |
| shadowing-warning | deep-dot-chain | Shadowing-warning notes deep-dot-chain pioneered FrameStack pattern + diagnostic-code style | Deep-dot-chain notes shadow lint likely shares LintConfig pattern | Reciprocal: lint-hygiene infrastructure share |
| specialization-scope | match-block-decls | Spec-scope notes match-block-decls owns grammar/producer side of MatchArmDeclGroup | Match-block-decls notes MatchArmDeclGroup recursion wired into walk_specialization_scope_members | Reciprocal: producer/consumer pair |
| specialization-scope | forall-statement-form | Spec-scope notes forall owns grammar/producer side of forall | Forall-statement-form notes nothing back specifically | Mostly one-way; listed for visibility |
| auto-resolution-backtracking | auto-type-param-resolution | ARB notes ATPR likely shares M-002/M-014 as orphans | ATPR notes ARB-sibling has been audited; M-005/9/10/13 dup ARB M-001/14/2/13 — Phase 3 should dedupe | Reciprocal: explicit "Phase 3 dedupe" request from both sides |
| varying-thickness-shells | composite-laminated-shells | Varying-thickness notes composite-laminated-shells composes (varying total × variable ply count = union) | Composite notes follow-on includes varying-thickness; structural-analysis-progressive-damage seeded | Reciprocal: future-feature union |
| warm-state-eviction | persistent-fea-cache | Warm-state notes persistent-fea-cache assumes FEA warm state survives across edits | Persistent-fea-cache notes mesh-morphing exclusion (not warm-state directly) | One-way; listed for visibility |
| warm-state-eviction | multi-load-case-fea | Warm-state notes multi-load-case assumes FEA warm state survives | Multi-load-case does not specifically cite warm-state-eviction | One-way; listed for visibility |
| stdlib-trait-breadth | geometry-traits | stdlib notes geometry-traits half_space/extrude_infinite FICTION blocks "Solid as trait-bound-bearing value" | geometry-traits notes stdlib task 2347 audited stdlib trait inheritance | Reciprocal: stdlib-trait coupling, both sides explicit |

**Genuinely contested (ownership ambiguity):**
1. `imported-field-source ↔ multi-kernel` — each says the OTHER owns OpenVDB dispatcher/consumer boundary.
2. `persistent-naming-v2 ↔ multi-kernel` — each says the OTHER owns Manifold propagate_attributes / MeshGL walk.
3. `topology-selectors ↔ persistent-naming-v2` — neither owns the missing try_eval_topology_selector dispatch arms.

**Mild contradiction (gate-direction ambiguity):**
1. `structural-analysis-fea ↔ structural-analysis-shells` — both note the OTHER landed code ahead of itself. Schedule-ordering ambiguity.
2. `compute-node-infrastructure ↔ mesh-morphing` — compute-node-infra names mesh-morph as Consumer; mesh-morph explicitly denies routing through @optimized.

---

## 4. Mechanism clusters from breadcrumbs

Where ≥3 PRDs all point at the same mechanism in their narrative (i.e. independent of the formal mechanism-enumeration).

### Cluster A: GR-001 (struct-constructor runtime evaluation)

Cited in breadcrumbs by:
- composite-laminated-shells (every proposed stdlib structure hits this gap)
- field-source-kinds (immediate parent of M-016)
- fea-gui-rendering (LoadCase/MultiCaseResult constructors transitively block)
- hex-wedge-meshing (ElasticOptions ctor literal cannot produce runtime value to read force_tet)
- kinematic-constraints-toplevel (Mechanism/Snapshot/Joint are types — same shape; sidesteps via Map+kind)
- multi-load-case-fea (M-003/M-004/M-007 transitively blocked)
- pragmas (explicitly notes "not applicable to this PRD")
- specialization-scope (M-002/M-010 structurally identical to GR-001 — independent rediscovery)
- structural-analysis-fea (origin; touches multi-load-case/shells/mesh-morph/error-estimation)
- structural-analysis-shells (gates ElasticResult/ShellStress/ElasticOptions/ShellForce runtime form)
- structural-stability-buckling (Material runtime instantiation transitively blocked)
- reify-doc-tool (M-006 "structurally identical to GR-001" — promotion candidate)
- stdlib-trait-breadth (one-slot-many-traits material composition leans on GR-001 once it lands)
- topology-selectors (explicitly notes "does NOT affect this PRD")
- varying-thickness-shells (linear_taper(...) shares "compile-time annotation arg must reference runtime-evaluable construct" failure pattern)
- kleene-logic (explicitly notes "No GR-001 interaction")
- geometry-traits (explicitly notes "does NOT affect this PRD — path is clean")

**Signal:** 17 of 40 PRDs mention GR-001 in breadcrumbs. The audit confirmed GR-001 is the single most pervasive cross-PRD concern. Notably, several PRDs explicitly call out "does NOT affect this PRD" — a useful negative signal. The breadcrumb survey adds at least one parallel-rediscovery citation (`specialization-scope` M-002/M-010 — "consumer half built, producer half silently absent with no task ID" structurally identical) and one promotion candidate (`reify-doc-tool` M-006 — "task marked done whose runtime/compile-time contract is empirically absent").

### Cluster B: TODO(field-in-param, task #3117) — `Field<X,Y>` in parameter position

Cited in:
- a-posteriori-error-estimation (M-002/M-005/M-011)
- composite-laminated-shells (per-ply stress/strain/failure-index result fields)
- structural-analysis-fea (M-022 — umbrella; touches all six field-slots in solver_elastic.ri)

**Signal:** 3 PRDs converge on this. Less pervasive than GR-001 but same shape — a single language-feature gap blocking multiple downstream PRDs.

### Cluster C: ComputeNode dispatch chain (tasks 3377-3385, FEA #2924, task 3426)

Cited in:
- a-posteriori-error-estimation (M-018 transitive)
- auto-type-param-resolution (deferred type-substitution prereq for @optimized fn lowering)
- fea-gui-rendering (owns tasks 3377-3385 that 2924 chains through)
- fea-gui-rendering-shells (same)
- freshness-4-variant (Freshness::Pending reuse candidate)
- hex-wedge-meshing (force_tet/require_hex_wedge read at ComputeNode-dispatch time)
- multi-kernel (analogous "infrastructure scaffold without runtime activation" pattern)
- multi-load-case-fea (M-006/M-014 owned there)
- node-trait-composition (worked example node targets FEA solvers; @optimized is consumer)
- persistent-fea-cache (M-011 cannot land until ComputeNode dispatch ships)
- solver-hint-payloads (same FICTION pattern)
- structural-analysis-fea (assumes all six P3 tasks landed)
- structural-analysis-shells (T18 extraction caching depends on @optimized→ComputeNode lowering)
- structural-stability-buckling (solve_buckling @optimized follows same chain)
- warm-state-eviction (NodeId::Compute donation hook gated)

**Signal:** 15 PRDs touch the ComputeNode-dispatch chain. This is the second-most-pervasive cross-cutting mechanism (after GR-001) and matches the inbound-degree leaderboard (compute-node-infrastructure is #2).

### Cluster D: Multi-kernel dispatch / OpenVDB ingestion

Cited in:
- geometry-traits (deferred to v0.2)
- imported-field-source-hdf5-csv (key motivator)
- imported-field-source (owns OpenVDB sub-kernel adapter)
- per-purpose-tolerance (depends on multi-kernel dispatch wired into op execution / task 2642)
- persistent-naming-v2 (Manifold concrete impl gated)
- pragmas (kernel-scope drift; v0.1 accepts v0.2 idents but doesn't dispatch)
- structural-analysis-shells (OpenVDB FFI follow-up)

**Signal:** 7 PRDs converge. Contested ownership (cluster B in §3) is visible.

### Cluster E: Imported-geometry / imported-field naming surface

Cited in:
- field-source-kinds (sampled-field-source PRD missing — candidate cross-PRD coordination gap)
- geometry-traits (imported field source kind interacts with conformance)
- imported-field-source-hdf5-csv (Length(mm) unit-literal grammar reference)
- multi-kernel (OpenVDB unblocks `imported`)
- topology-selectors (imported geometry requires own naming scheme, deferred)

**Signal:** 5 PRDs note "imported geometry / imported field needs its own surface and that surface is deferred / unowned." This is a likely Phase 3 gap that the formal mechanism enumeration may have missed — no single PRD owns it; it's an inter-PRD gap.

### Cluster F: Linter / lint-hygiene infrastructure

Cited in:
- deep-dot-chain (linter threshold configurability)
- shadowing-warning (sibling lint, FrameStack pattern)
- specialization-scope (validation called immediately after shadow_lint::lint_module)

**Signal:** 3 PRDs — the LintConfig shared-struct candidate explicitly called out in deep-dot-chain is worth a Phase 3 decision.

### Cluster G: Grammar/producer half silently absent for declaration-form

Cited in:
- forall-statement-form (chain statement / @forall over purpose / SchemaNode / connector_sub)
- match-block-decls (MatchArmDeclGroup grammar + tree-sitter)
- specialization-scope (M-002/M-010 — consumer half built, producer half silently absent, no task ID)
- structural-analysis-shells (structure-def syntax parser-only with no consumer)

**Signal:** 4 PRDs identify the same shape — language constructs where one half (parser or evaluator) ships in isolation. This is the structural pattern the audit-brief named in its motivating example for GR-001.

### Cluster H: "PRD-marked-done while code empirically absent" pattern

Cited in:
- imported-field-source-hdf5-csv (v0.2 base PRD tasks 2665-2669 done-with-merge-commits but eval-side glue not wired)
- money-dimension (PRD-vs-task status drift — header labels 2379-2383 "in-flight" though landed)
- reify-doc-tool (M-006 task marked done whose contract is empirically absent — promotion candidate to gap-register)
- solver-hint-payloads (FICTION pattern mirrors GR-001 — compile-side stores annotation, no downstream reads)
- specialization-scope (consumer half built and tested, producer half silently absent with no task ID)

**Signal:** 5 PRDs flag PRD-vs-code state drift as a meta-pattern. Phase 3 might want a status-reconciliation sweep across the corpus, not just per-PRD gaps.

### Cluster I: Architecture-section references (§14.5 Input occurrence, §10.5 stack/patchwork, §16 open Q #8, §7.5 cancellation, §6.2-6.4 SchemaNode, §2.5 monotonic-feasible)

Cited in:
- auto-type-param-resolution (Architecture §6.2-6.4, §2.5)
- imported-field-source (arch §14.5)
- multi-kernel (Architecture §10.5, §16 open Q #8)
- node-trait-composition (Architecture §7.5)
- per-purpose-tolerance (arch §14.5)

**Signal:** 5 PRDs lean on specific Architecture-doc sections. Useful for Phase 3 to flag any §x.y that's referenced by ≥3 PRDs as load-bearing.

---

## 5. PRDs with empty breadcrumbs

Three findings files have a present-but-effectively-empty `## Cross-PRD breadcrumbs` section, or one that explicitly disclaims cross-PRD scope:

| File | Reason |
|---|---|
| migration-toolchain.md | PRD is process-only / version-gated; breadcrumbs explicitly state "No active gap" and only enumerate hypothetical future-PRD coordination if v0.3 activates this PRD. The audit-brief allows skipping the inventory for process PRDs. |

All 40 files have a Cross-PRD breadcrumbs section. None were entirely empty. The remaining 39 have at least one substantive cross-reference. The thinnest sections (besides migration-toolchain):
- kleene-logic — 4 bullets but 3 of them point at non-PRD spec/grammar drift; only one tenuous PRD-coupling
- forall-statement-form — 4 bullets, all to non-PRD-or-future-PRD targets (chain, purposes, SchemaNode, connect) — no living-PRD coupling
- migration-toolchain — process-doc skip per audit-brief

These three are the only auditors who didn't surface a single live-PRD cross-reference. Of those, kleene-logic and forall-statement-form may indicate "genuinely standalone language-feature PRDs"; migration-toolchain is explicitly process-only.

---

## Appendix: source heading variants

All 40 files used the canonical heading `## Cross-PRD breadcrumbs`. No tolerance variants needed.

## Appendix: counting method

- 198 raw rows in §1: 158 PRD→PRD directed edges + 40 non-PRD references (architecture sections, future-unfiled PRDs, task IDs, spec sections, code references).
- Inbound-degree leaderboard in §2 was verified programmatically (case-sensitive substring of target slug inside source's `## Cross-PRD breadcrumbs` section). Same source/target pair counted once even if multiple bullets reference it.
- §3 lists ~30 reciprocal pairs; 3 are genuinely contested (ownership ambiguity: PNv2↔multi-kernel, imported-field-source↔multi-kernel, topology-selectors↔persistent-naming-v2), and 2 are mild contradictions (FEA↔shells schedule order, compute-node-infra↔mesh-morph consumer-claim).
