# Audit: Persistent Topology Naming v2 (Solvespace-Style)

**PRD path:** `docs/prds/v0_2/persistent-naming-v2.md`
**Auditor:** audit-persistent-naming-v2
**Date:** 2026-05-12
**Mechanism count:** 22
**Gap count:** 12 (10 WIRED, 2 PARTIAL, 3 TODO, 6 FICTION, 1 DRIFT — gap = state != WIRED)

## Top concerns

- **The PRD's user-facing payoff — `@face(name)` resolving via `TopologyAttribute` — is FICTION at the surface DSL layer.** Per `crates/reify-expr/src/lib.rs:700`, `CompiledExprKind::AdHocSelector { .. } => Value::Undef`. There is no engine-side override; nothing in `reify-eval/src` reads the AdHocSelector variant beyond placeholder-expansion recursion. The `topology_attribute_table` is populated by per-op auto-population (real, threaded), but no production path calls `resolve_unique_by_attribute`. Tests `tests/topology_attribute_resolver_e2e.rs` call it directly; production does not. The v0.1 `resolve_unique_by_tag` is in the same state — only test-only callers. The whole selector-resolution machinery is library code with no DSL-side dispatcher.
- **Tasks 8 (Booleans) and 7b (fillet/chamfer eval-side wiring) are PENDING, not done.** Task 2656 was reset to pending 2026-05-10 (worktree reaped before merge). Task 2831 is pending. The OCCT FFI exists (`boolean_fuse_with_history`, `fillet_with_history`, `chamfer_with_history`), and the engine-side propagator `propagate_attributes_via_brepalgoapi_history` exists with comprehensive unit tests, but no `AttributeHistory::Boolean | Fillet | Chamfer` variants exist and `engine_build::execute_with_history` falls these ops through to `kernel.execute(&op).map(|h| (h, AttributeHistory::None))` (`crates/reify-kernel-occt/src/handle.rs:878`). All Boolean/fillet/chamfer ops produce result handles with NO attribute entries on the table — so even if a surface resolver existed, attribute-based selectors over Boolean/fillet/chamfer outputs would fall through to `FallbackToComputed`.
- **Task 9 Manifold MeshGL walk is a stub.** `crates/reify-kernel-manifold/src/kernel.rs:26-35` documents that `KernelAttributeHook::propagate_attributes` returns `Ok(KernelAttributeOutcome::Discarded)` with `tracing::warn!(reason="task_9_pending", …)`. Task 2657 is marked done because the *trait wiring* is done; the actual MeshGL/originalID walk is not implemented and no follow-up task exists. PRD line 70's "Manifold provides the first concrete impl" reads as done but only the dispatcher boilerplate is wired.
- **Two primitives (Cone, Torus, Tube) are omitted from the seeder.** `seed_primitive_attributes` (`crates/reify-eval/src/primitive_attribute_seed.rs:155-160`) only handles `Box | Cylinder | Sphere`; `Cone` and `Torus` exist in `GeometryOp` and are listed in the PRD's "Primitives" task (decomp item 6), but task 2574 shipped a narrower scope. Cone and Torus result handles have no attribute entries.
- **PRD line 50 absorption of v0.1 `name = "..."` into `user_label` is FICTION.** No `name = "..."` syntax exists in the parser/grammar (`reify-syntax/src/ts_parser.rs`); no compiler path sets `TopologyAttribute::user_label`. Every production-side `user_label` literal in the codebase is `None`. The PRD references a v0.1 feature that isn't there.

## Mechanisms

### M-001: `TopologyAttribute { feature_id, role, local_index, user_label, mod_history }` data model

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-types/src/geometry.rs:1924-1953` (struct + `same_parent_as`); `ModEntry` at 1820-1824; `Role` enum at 1860-1904 with per-op variants (Cap/Side/NewEdge/RevolvedFace/AxisFace/SweptFace/LoftedFace/MidSurfaceFace/MidSurfaceEdge); `FeatureId` at 1755-1788 with `RealizationNodeId → FeatureId` conversion. Task 2590 done.
- **Blocks:** none
- **Note:** Schema landed exactly per PRD lines 52-61. Closed-extension enum (no `Other(String)` escape hatch) keeps selector resolution exhaustive.

### M-002: `TopologyAttributeTable` runtime keyed by `GeometryHandleId`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-types/src/geometry.rs:1963-2006`; lifecycle in `crates/reify-eval/src/engine_build.rs:535, 789, 1025, 2249` (reset before each eval). Field on `Engine` at `crates/reify-eval/src/lib.rs:507`. Threaded through builder at `engine_build.rs:57,72,79`.
- **Blocks:** none
- **Note:** HashMap-backed, last-write-wins on `record`. Per design comment, never serialized; rebuilt per realization.

### M-003: `BRepAlgoAPI_*` history propagation (`propagate_attributes_via_brepalgoapi_history`)

- **State:** PARTIAL
- **Failure mode:** F1 (compile-time contract → no runtime backing for the ops it was built for)
- **Evidence:** `crates/reify-eval/src/topology_attribute_propagation.rs:114-186`; comprehensive unit tests at lines 1083+ (over 30 cases). However, NO production caller invokes it for Boolean ops because no `AttributeHistory::Boolean` variant exists. The only production callers (`populate_attribute_history` at `engine_build.rs:155`) dispatch only Extrude/Revolve/Sweep/Loft.
- **Blocks:** task 2656 (Booleans) is the integration; task 3376 (Boolean polish) is sequenced after.
- **Note:** Built and unit-tested, but the integration glue that would make Boolean ops go through `boolean_fuse_with_history` + this propagator does not exist in `execute_with_history` (`crates/reify-kernel-occt/src/handle.rs:832-880`).

### M-004: Per-op attribute population — Extrude

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `populate_extrude_attributes` at `topology_attribute_propagation.rs:404`; routed via `AttributeHistory::Extrude` (`crates/reify-kernel-occt/src/handle.rs:840-852`); engine call at `engine_build.rs:175-184`. Task 2573 done. E2E coverage `tests/topology_attribute_extrude_revolve_e2e.rs`.
- **Blocks:** none
- **Note:** Caps (Top/Bottom) + Side faces + NewEdge boundary edges per PRD Role vocabulary.

### M-005: Per-op attribute population — Revolve

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `populate_revolve_attributes` at `topology_attribute_propagation.rs:487`; routed via `AttributeHistory::Revolve` (`handle.rs:853-860`). Task 2573 done. Task 2636 closed the full-2π provenance gap.
- **Blocks:** none
- **Note:** Cap(Start)/Cap(End)/RevolvedFace/AxisFace (AxisFace declared but not yet emitted per `geometry.rs:1875-1878`).

### M-006: Per-op attribute population — Sweep

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `populate_sweep_attributes` at `topology_attribute_propagation.rs:554`; routed via `AttributeHistory::Sweep` (`handle.rs:861-867`). Task 2619 done. PRD §"OCCT integration notes" called this out as needing custom history mappers — done via templated FFI helpers.
- **Blocks:** none
- **Note:** Cap(Start)/Cap(End)/SweptFace roles emitted.

### M-007: Per-op attribute population — Loft

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `populate_loft_attributes` at `topology_attribute_propagation.rs:639`; multi-parent routing via `AttributeHistory::Loft` (`handle.rs:868-874`); engine wraps via `populate_loft_op` at `engine_build.rs:327-352`. Task 2619 done.
- **Blocks:** none
- **Note:** Cap(Start)/Cap(End)/LoftedFace; section-index propagation via custom `BRepOffsetAPI_ThruSections::GeneratedFace(edge)` mapper.

### M-008: Per-op attribute population — Primitives (Box, Cylinder, Sphere, Cone, Torus, Tube)

- **State:** PARTIAL
- **Failure mode:** F1 (Cone/Torus/Tube enumerated in PRD task 6 but not seeded)
- **Evidence:** `seed_primitive_attributes` at `primitive_attribute_seed.rs:155-252` handles only `Box | Cylinder | Sphere`. `Tube` is a GeometryOp primitive (`engine_build.rs:418`) but not seeded. PRD decomp task 6 lists "Box, cylinder, sphere, cone, torus" — Cone and Torus are absent from the impl. Task 2574 marked done with title "(box, cylinder, sphere, cone, torus)" — title overstates scope.
- **Blocks:** none filed
- **Note:** A model that uses a `Cone` or `Torus` primitive will produce a result handle whose faces/edges carry no `TopologyAttribute` entries → selector resolution will hit `FallbackToComputed`.

### M-009: Per-op attribute population — Booleans (Union, Difference, Intersection)

- **State:** FICTION
- **Failure mode:** F1 (PRD decomp task 8 assumes propagation exists; runtime falls through to `AttributeHistory::None`)
- **Evidence:** No `AttributeHistory::Boolean` variant in `geometry.rs:1354-1368`; `execute_with_history` default arm at `handle.rs:878` returns `AttributeHistory::None` for booleans. `boolean_fuse_with_history` FFI exists (`reify-kernel-occt/src/handle.rs:820`) and is integration-tested (`tests/boolean_op_history_integration.rs`), and the propagator `propagate_attributes_via_brepalgoapi_history` exists with extensive coverage, but the wiring between them is absent. Task 2656 PENDING (worktree reaped 2026-05-10).
- **Blocks:** 2656 (parent), 3376 (post-merge polish, also pending).
- **Note:** This is the highest-leverage gap — Booleans are central to the PRD's modification-history postfix mechanism (cutting splits a face → ModEntry). Without this, the `same_parent_as` clustering and `AmbiguousAfterSplit` resolution paths cannot fire for the most common split source.

### M-010: Per-op attribute population — Fillet / Chamfer (eval-side wiring)

- **State:** FICTION
- **Failure mode:** F1 (PRD decomp task 7 assumes propagation exists)
- **Evidence:** OCCT FFI exists (`fillet_with_history`, `chamfer_with_history` in `reify-kernel-occt/src/handle.rs:820-831` + integration tests `tests/fillet_with_history_integration.rs`, `tests/chamfer_with_history_integration.rs`); no `AttributeHistory::Fillet | Chamfer` variant; engine_build dispatches Fillet/Chamfer to default arm returning `AttributeHistory::None` (`handle.rs:878`). Task 2655 done (FFI only — scope narrowed via `info added 2026-05-01`); Task 2831 PENDING.
- **Blocks:** 2831 (eval-side wiring), and transitively any feature relying on fillet/chamfer split-children naming.
- **Note:** `populate_attribute_history` (`engine_build.rs:155`) lacks Fillet/Chamfer arms. The "canonical mod_history generators" per task-7 description never emit ModEntries in production.

### M-011: Per-op attribute population — Mid-surface (derived geometry)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `populate_mid_surface_attributes` at `crates/reify-shell-extract/src/mid_surface_naming.rs:134`; MidSurfaceFace + MidSurfaceEdge roles in `geometry.rs:1893-1903`; `FeatureId::derived_mid_surface` derivation. Task 3033 done. Cross-PRD breadcrumb: shells PRD T20.
- **Blocks:** none
- **Note:** Out-of-band extension to the v0.2 vocabulary, used by the Shells PRD. Demonstrates the closed-extension pattern works.

### M-012: Modification-history threading (`mod_history` `ModEntry` append on splits)

- **State:** PARTIAL
- **Failure mode:** F1 (engine-side mechanism exists but only the BRep-history propagator emits ModEntries; Booleans + fillets/chamfers don't go through it in production)
- **Evidence:** `propagate_attributes_via_brepalgoapi_history` at `topology_attribute_propagation.rs:114-186` correctly counts children-per-parent via `count_children_per_parent`, assigns `split_index 0,1,2,…`, and appends `ModEntry { splitting_feature_id, split_index }` (lines 125-185). `same_parent_as` helper exists (`geometry.rs:1947-1952`). Task 2653 done.
- **Blocks:** Real-world ModEntry production blocked by M-009 (Booleans pending) and M-010 (fillet/chamfer eval-side pending). For Extrude/Revolve/Sweep/Loft, splits aren't a typical outcome.
- **Note:** The mechanism is correctly implemented; it just has no real-world ops driving it in production today.

### M-013: Selector resolution — attribute lookup (`resolve_unique_by_attribute`)

- **State:** FICTION
- **Failure mode:** F1 (PRD line 35: "Selector resolution becomes attribute lookup" — but no surface DSL caller invokes the resolver)
- **Evidence:** `resolve_unique_by_attribute` at `topology_attribute_resolver.rs:147-288`. The only call sites are tests (`tests/topology_attribute_resolver_e2e.rs`, internal unit tests). Grep across production code finds zero non-test callers. `CompiledExprKind::AdHocSelector` evaluates to `Value::Undef` (`crates/reify-expr/src/lib.rs:700`); engine_purposes.rs:661 only recurses for placeholder expansion, doesn't compute a value. Task 2652 marked done — but "done" here means the library function exists, not that it's wired into surface evaluation.
- **Blocks:** the entire user-facing payoff of the PRD.
- **Note:** Identical state to the v0.1 path: `resolve_unique_by_tag` is also test-only. Both schemes have a working library and a missing DSL-side dispatcher. The v0.1 `closest_point`/`is_on`/`angle_between_surfaces` are wired via `try_eval_topology_selector` (`geometry_ops.rs:1687`), but those are auxiliary queries, not selector resolution.

### M-014: Imported-geometry fallback (`FallbackToComputed`)

- **State:** PARTIAL (library)
- **Failure mode:** F1 (resolver supports it; surface DSL never invokes the resolver)
- **Evidence:** `resolve_unique_by_attribute` returns `AttributeResolution::FallbackToComputed` when no candidate carries an entry (`topology_attribute_resolver.rs:158-173`). Library-side correct.
- **Blocks:** depends on M-013.
- **Note:** Same shape as M-013 — mechanism is implemented but unreachable from user code.

### M-015: User-label preference rule (PRD line 62)

- **State:** PARTIAL (library)
- **Failure mode:** F1 (logic exists; `user_label` is always `None` in production data)
- **Evidence:** `resolve_unique_by_attribute` `user_label` branch at `topology_attribute_resolver.rs:222-251`. Every production-side write of `TopologyAttribute::user_label` literal is `None` (verified across `primitive_attribute_seed.rs`, `topology_attribute_propagation.rs`, `mid_surface_naming.rs`). No syntactic mechanism (`name = "..."`) is parsed by the grammar — PRD line 50's "absorbs v0.1 `name = "..."` syntax" is FICTION-adjacent (no such v0.1 syntax exists in the parser).
- **Blocks:** none filed; would block user-controlled face naming UX.
- **Note:** Coupled to M-013; even if surface resolution wired up, the user-label slot is never populated. Could be considered DRIFT (PRD describes a non-existent v0.1 feature) but classifying as PARTIAL because the runtime mechanism is real and tested via mocks.

### M-016: Local-index reassignment diagnostic (PRD task 4)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `detect_local_index_reassignment_diagnostics` at `topology_attribute_propagation.rs:978`; wired into engine at `engine_build.rs:1953`; constant `LOCAL_INDEX_REASSIGNMENT_TOLERANCE_M` exported. Task 2654 + 3367 + 3369 done. Per-realization scan bounded by 3369; doc polish by 3367.
- **Blocks:** none
- **Note:** Disjoint from `TopologyAttributeAmbiguousAfterSplit` via the `mod_history.is_empty()` filter so the two codes don't double-warn.

### M-017: `KernelAttributeHook` trait + engine-side dispatcher

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** Trait at `reify-types/src/geometry.rs:1440`; `KernelAttributeOutcome { Propagated | Discarded | FellThrough }` at 1401; default `attribute_hook() = None` on `GeometryKernel` (1494-1500-ish). Engine dispatcher `propagate_via_kernel_attribute_hook` at `kernel_attribute_hook.rs:81`. Wired into `execute_realization_ops` at `engine_build.rs:1804-1816`. Tasks 2657 + 2875 done. Three-layer test contract per kernel_attribute_hook.rs:26-44 (unit + cross-crate + engine wiring).
- **Blocks:** none
- **Note:** Trait surface and dispatcher are production-quality; the concrete Manifold impl (M-018) is a stub.

### M-018: Manifold `KernelAttributeHook` MeshGL walk (PRD line 70's concrete impl)

- **State:** FICTION
- **Failure mode:** F1 (PRD claims Manifold provides the first concrete impl; the impl returns `Discarded`)
- **Evidence:** `crates/reify-kernel-manifold/src/kernel.rs:26-35` documents stub status; PRD line 70 says "Manifold (when 2295 lands) provides the first concrete impl using its existing `originalID` + per-triangle `faceID` + `MeshGL` merge-vector pattern". Hook returns `Ok(KernelAttributeOutcome::Discarded)` + `tracing::warn!(reason="task_9_pending", …)`. The trait wiring + manifold3d FFI are in place (task 3093 done); the MeshGL walk implementation is not. No follow-up task tracks it.
- **Blocks:** any cross-kernel attribute preservation via Manifold path.
- **Note:** Task 2657 done provenance comments emphasize the trait surface is stable so only the body changes — but no body has been written and no task tracks writing it. Bookmark-style limbo without a bookmark.

### M-019: Selector vocabulary v2 — direction, extremal, type, combinators, walks, history, attribute primitives (DSL surface)

- **State:** FICTION
- **Failure mode:** F1 (PRD task 10 lists 7 sub-vocabularies; Rust library exists; none registered in DSL surface)
- **Evidence:** `crates/reify-eval/src/selector_vocabulary_v2.rs` exposes public Rust functions: `intersect/union/complement/except` (combinators), `faces_perpendicular_to`, `extremal_by_bbox/centroid`, `faces_by_surface_kind`, `edges_by_curve_kind`, `geom_universal`, `created_by_feature`, `split_by_feature`, `has_user_label`, `user_label_eq`, `adjacent_to_face`, `ancestor_faces_of_edge`, `siblings_of_face`, `owner_body_of`. None are registered in `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` (`crates/reify-compiler/src/units.rs:166-186`) or any compiler dispatch path. `grep`-search across `reify-compiler/src`, `reify-syntax/src`, `compiler/stdlib/*.ri`, `prj/`, `examples/` finds zero references to these names. Task 2658 done — but again "done" means library exists.
- **Blocks:** entire selector vocabulary v2 user-facing utility.
- **Note:** Same anti-pattern as M-013: the implementation work landed in the eval library, but the surface-language wiring task was never identified or filed.

### M-020: V0.1 selector functions runtime dispatch (`edges`, `faces`, `edges_by_length`, `faces_by_area`, `faces_by_normal`, `edges_parallel_to`, `edges_at_height`, `adjacent_faces`, `shared_edges`, `center_of_mass`, `moment_of_inertia`)

- **State:** FICTION
- **Failure mode:** F1 (PRD line 35 says "Computed selectors still exist as a fallback"; per task 2699 reopen reason, NONE of the 11 v0.1 selectors have eval dispatch)
- **Evidence:** Task 2699 (marked done after a phantom-done unblock cycle) carries a 2026-05-09 `reopen_reason`: "eval-side dispatch arms absent — 11 topology-selector names (edges, faces, edges_by_length, faces_by_area, faces_by_normal, edges_parallel_to, edges_at_height, center_of_mass, moment_of_inertia, adjacent_faces, shared_edges) are not present in geometry_ops.rs try_eval_topology_selector (lines 1687–1761)". Confirmed by grep: `try_eval_topology_selector` only handles `closest_point | is_on | angle_between_surfaces` (`geometry_ops.rs:1701-1706`). The "REMAINING WORK" section of task 2699's details still flags this gap; the done-state is a reconciler artifact.
- **Blocks:** the entire imported-geometry fallback (PRD line 68's "Computed selectors still exist as a fallback for cases where attribute-based naming is impossible").
- **Note:** Cross-PRD: this is technically a separate PRD's gap (`topology-selectors.md`) but this PRD's "Selector resolution unified" decision depends on it.

### M-021: Persistent-cache attribute preservation across edits

- **State:** TODO
- **Failure mode:** F1 (PRD assumes attribute IDs survive parameter changes — i.e. `topology_attribute_table` persists or is reconstructed identically)
- **Evidence:** `engine_build.rs:1025, 2249` resets `topology_attribute_table = TopologyAttributeTable::default()` on every eval and rebuilds from scratch. Comment at `engine_build.rs:1684-1687` documents "topology_attribute_table already has an entry for cached handle" assertion when a `RealizationCache` hit short-circuits ops re-execution. This is the right shape — but no test pins that `feature_id` + `(role, local_index)` for the *same source on the same handle* stays stable across two evals of the same model. Memory entry "topology_attribute_table is never modified by edit_param" (graphiti 2026-04-29) suggests stability is asserted somewhere but I did not find the pinning test.
- **Blocks:** none filed; integrity-of-resolution-across-edits is implicitly assumed.
- **Note:** TODO classification because the mechanism (deterministic reconstruction) is the design path, but its stability invariant is not pinned by a test I could find.

### M-022: AdHocSelector engine-side evaluator (`@face("name")` → handle)

- **State:** FICTION
- **Failure mode:** F1 (Task 250 marked done; surface still returns `Value::Undef`)
- **Evidence:** `crates/reify-expr/src/lib.rs:700`: `CompiledExprKind::AdHocSelector { .. } => Value::Undef`. Comment: "Ad-hoc selector evaluation is handled by the engine (Task 250), which has access to the geometry kernel." But grep across `reify-eval/src` finds zero variants of "the engine handles it" — only `engine_purposes.rs:661` for placeholder-expansion recursion. `crates/reify-eval/tests/m10_combined.rs:619-634` test is `#[ignore = "ad-hoc selector resolution not yet wired up; see reify-expr/src/lib.rs:511"]`. Task 250 is marked done but the test still ignores; the architect's added info at `2026-04-09T19:17` flags the blocker on Task 249. Despite task-250 done state, the runtime behavior remains: `@face("top")` evaluates to `Value::Undef`.
- **Blocks:** M-013, M-014, M-015, M-019 — every selector-resolution surface that depends on going from `@face/@edge` syntax to a runtime handle.
- **Note:** The single root cause for the entire PRD's user-facing utility being non-functional. The "task done" status is misleading — there is a test that explicitly waits for this to land.

## Cross-PRD breadcrumbs

- **`docs/prds/v0_2/multi-kernel.md`** — PRD line 70 ("Multi-kernel propagation via `KernelAttributeHook` trait") cross-references task 2295. The Manifold concrete impl (M-018) is gated on that PRD; if multi-kernel deferred the MeshGL walk, this PRD's claim about "Manifold provides the first concrete impl" needs revisiting in Phase 3.
- **`docs/prds/topology-selectors.md`** — task 2699's reopen_reason cites 11 missing dispatch arms in `try_eval_topology_selector`. Those selectors are PRD line 35's "computed selectors still exist as a fallback" for imported geometry. The persistent-naming-v2 design depends on that fallback being real, but neither PRD owns it.
- **`docs/prds/v0_4/structural-analysis-shells.md`** line 81 (T20) — mid-surface naming sub-vocabulary (M-011) is the only out-of-band Role extension shipped. Demonstrates the extension pattern.
- **`docs/prds/v0_3/mesh-morphing.md`** — task 2939 (Stage B persistent-naming bijection check) consumes the v0.2 attribute table via `crates/reify-eval/src/morph_stage_b.rs`. First downstream consumer beyond resolver tests; depends on M-002 (table existence) and M-012 (mod_history threading).
- **PRD §"Deferred to v0.3+"** bookmarks (tasks 2560 adjacency keys, 2561 hash compaction) are correctly deferred. No audit work on those.

## Summary

The PRD is in a peculiar state: the **plumbing is uniformly first-class** (data model, propagation, kernel-hook dispatch, fragility diagnostics, mid-surface integration, mesh-morphing bijection consumer all WIRED), but the **endpoints — Boolean/fillet/chamfer producers and surface-language consumers — are FICTION**. Tasks 2-7a/9/10 are marked done; tasks 7b/8 are pending. Several "done" tasks (2657, 2658, 2699, 250) are done in a library-only sense — the trait/function exists, but the surface-DSL or production-FFI wire that would make the library do useful work is missing. Phase 3 should weigh "done means library landed" vs "done means user-observable behavior" — this PRD trips that distinction five times (M-013, M-015, M-018, M-019, M-022). The reconciler-driven done flips on tasks 2699 and 2657 obscured the gap.
