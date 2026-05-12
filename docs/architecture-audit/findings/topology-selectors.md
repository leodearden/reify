# Audit: Topology-Selector Function Family

**PRD path:** `docs/prds/topology-selectors.md`
**Auditor:** audit-topology-selectors
**Date:** 2026-05-12
**Mechanism count:** 18
**Gap count:** 8

## Top concerns

- **Eval-side dispatch for 11 of the 14 topology-selector names is `None`-stubbed.** Task 2699 wired *compile-time types* for `edges`, `faces`, `edges_by_length`, `faces_by_area`, `faces_by_normal`, `edges_parallel_to`, `edges_at_height`, `adjacent_faces`, `shared_edges`, `center_of_mass`, `moment_of_inertia` — but the eval-side dispatch in `crates/reify-eval/src/geometry_ops.rs::try_eval_topology_selector` only handles the three task-2324 names (`closest_point`, `is_on`, `angle_between_surfaces`). All 11 others fall through to `_ => return None`, leaving cells at `Value::Undef`. The deferred work is named (task 2691) and the in-source comment is candid (`geometry_ops.rs:1646-1661`), but every PRD §Worked-example, every §Acceptance bullet beyond the three task-2324 names, and the entire mass-property triplet are *unreachable from `.ri` source today*. PRD reads as if v0.1 is shipped; the runtime says half-shipped.
- **PRD §Scope.4 "re-route the four already-shipped filtered selectors through feature-tag resolution" is a non-event.** The four `*_with_tags` variants exist (`edges_at_height_with_tags`, `edges_by_length_with_tags`, `faces_by_area_with_tags`, `edges_parallel_to_with_tags`) and populate `FeatureTagTable`, but they are pure-Rust pubs in `reify-eval`, *not* the path users hit when they write `edges_at_height(...)` from `.ri`. The `.ri` call routes through `try_eval_topology_selector` which today returns `None` (per gap above); there is no behaviour-change of "stale → `undef` + warning" visible from a user program. The PRD's "behaviour change: same results when topology stable, undef/diagnostic instead of stale results when topology changes" is not realised in any user-facing call path.
- **PRD's headline survival-across-edits test does not exist.** PRD §Worked examples show "edges_at_height(...) on a chamfered solid should return the same chamfer-bottom edges as before the chamfer was re-parameterized, by matching feature tags rather than absolute Z" and §6 lists "feature-tag survival across a fillet edit (selector returns same result after a parameter tweak)" as a test. Across all three test files (`feature_tag_e2e.rs`, `feature_tag_selector_tests.rs`, `feature_tag_tests.rs`) there is no test that performs a parameter edit and verifies selector stability. The `resolve_unique_by_tag` 1-match / 0-match / N-match unit tests pin the resolver primitive, but the "tag survives across an edit" *integration* contract is untested. This is the PRD's primary persistent-naming claim.
- **DRIFT from PRD's `(source_line, step_kind, sub_index)` tuple shape** to `(source_span, step_kind, sub_index)` (`crates/reify-types/src/geometry.rs:1693-1701`). The shipped tag stores a full `SourceSpan` (start+end byte offsets) rather than a line number; the rustdoc explicitly cites this as a deliberate departure ("`source_span` stores the full `SourceSpan` rather than a line number so that consumers with access to the source text can derive a line/column via `byte_offset_to_line_col(source, span.start)`"). Behavioural impact is minor (richer location info, identical equality semantics within a realization), but readers of the PRD will look for a field that isn't there. Also: `sub_index` stability is fragile under op insertion/reordering, and this is explicitly flagged in the rustdoc but **not** in the PRD.

## Mechanisms

### M-001: `GEOMETRY_TOPOLOGY_SELECTOR_NAMES` recogniser + `topology_selector_result_type` per-name return-type table

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/units.rs:169-186` (14-name `&[&str]` const); `:218-240` (per-name `match` returning `Type::point3(Type::length())` / `Type::Bool` / `Type::angle()` / `Type::List(Box::new(Type::Geometry))` / `Type::tensor(2, 3, MomentOfInertia)`); unit tests `:608-720` (one assertion per name, plus a coverage test `topology_selector_result_type_for_task_2699_names_matches_table`); task 2699 (per fused-memory `c718581f`).
- **Blocks:** none
- **Note:** Single source of truth for "is this a topology-selector function" at compile-time. Names share a list for classification; per-name dispatch lives in `topology_selector_result_type` and the eval-side post-process — same pattern used by kinematic-query and conformance-query helpers. Satisfies PRD §Scope.5 + Task 8 in full.

### M-002: Eval-side dispatch `try_eval_topology_selector` for `closest_point` / `is_on` / `angle_between_surfaces` (task 2324)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/geometry_ops.rs:1687-1761` (`try_eval_topology_selector` body: 4-step arg-shape guard, per-helper resolver + `GeometryQuery::{ClosestPointOnShape, PointOnShape, SurfaceAngle}` dispatch); call sites in `crates/reify-eval/src/engine_build.rs:648/898/1502/2170` (`post_process_topology_selectors` invoked from `build`, `build_snapshot`, `tessellate_realizations`); `try_eval_topology_selector_*` unit tests (`geometry_ops.rs:5585-6294`, ~20 tests); integration tests in `crates/reify-eval/tests/topology_selector_runtime.rs` (399 lines).
- **Blocks:** none (for these three names)
- **Note:** Sibling pattern to `try_eval_conformance_query` (geometry-traits PRD M-014). The `_ => return None` fallback at `:1705` is the deferred-dispatch hook for the 11 task-2699 names (see M-003).

### M-003: Eval-side dispatch for the 11 task-2699 selector names (`edges`, `faces`, `edges_by_length`, `faces_by_area`, `faces_by_normal`, `edges_parallel_to`, `edges_at_height`, `adjacent_faces`, `shared_edges`, `center_of_mass`, `moment_of_inertia`)

- **State:** FICTION
- **Failure mode:** F1 (PRD assumes mechanism; code stubs to `Value::Undef`)
- **Evidence:** `crates/reify-eval/src/geometry_ops.rs:1701-1706` (the `match function.name.as_str()` has arms only for the three task-2324 names; `_ => return None`); explicit acknowledgement in the module comment at `:1646-1661` ("Task 2699 added 11 names ... They fall through the `_ => return None` arm ... cell stays at the `Value::Undef`"); deferred to task 2691 (per fused-memory `79f9d700`, `fd616da2`). The `.ri` examples `all_topology_selectors_wiring.ri` and `block_inertia.ri` carry header notes ("the `i` cell stays at Value::Undef until ..."). `#[ignore]`-gated tests in `crates/reify-eval/tests/topology_selector_smoke_tests.rs:101,173,241` document the future contract.
- **Blocks:** PRD §3 (mass-property triplet user-facing eval), §4 (topology-relational selectors user-facing eval), §Worked examples (`block_inertia`, `fillet_top_edges` both rely on this), PRD §Acceptance (`cargo test -p reify-eval -- topology_selectors covers all eleven stdlib functions end-to-end`); ad-hoc port selectors / feature-tag survival end-to-end story.
- **Note:** The kernel-side path (M-005 through M-009) is fully wired — every `GeometryQuery` variant the 11 selectors need exists, has tests, and works under OCCT. Only the `match function.name.as_str()` body is missing arms. This is the single largest gap in the PRD's surface area: by mechanism count (11/14 ≈ 79%) the runtime side is unshipped despite the PRD's "11 + 4-re-expose" framing suggesting completeness.

### M-004: Three new OCCT FFI entry points — `closest_point_on_shape`, `point_on_shape`, `surface_angle`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-kernel-occt/cpp/occt_wrapper.cpp:2923-2952` (`closest_point_on_shape` via `BRepExtrema_DistShapeShape` + on-shell fallback for `dist < 1e-10`); `:2954-2977` (`point_on_shape` with tolerance, default = `DEFAULT_POINT_ON_SHAPE_TOLERANCE_M ≈ Precision::Confusion()`); `:2625-2640` (`surface_angle` via `face_outward_unit_normal` × 2 + arccos of dot); cxx bridge in `crates/reify-kernel-occt/src/ffi.rs`; rust-side dispatch in `crates/reify-kernel-occt/src/lib.rs:2425-2442` (`GeometryQuery::{ClosestPointOnShape, PointOnShape, SurfaceAngle}` arms).
- **Blocks:** none
- **Note:** PRD §Scope.1 ships in full. The PRD spec text says "`closest_point_on_shape` (already prototyped for `closest_point` in #319 — re-export under the new name)" but the OCCT wrapper actually carries it as `closest_point_on_shape` directly — name aligns. `is_on` user-facing name maps to underlying `point_on_shape` kernel method (per PRD §"Rename note (task 3201)" §c).

### M-005: OCCT FFI mass properties — `query_centroid` / `query_moment_of_inertia` / `query_inertia_tensor` via `BRepGProp::VolumeProperties` (+ density)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-kernel-occt/cpp/occt_wrapper.cpp:2868-2890` (`query_centroid` + `query_face_centroid`); `:2979-2986` (`query_moment_of_inertia` along an axis via `GProp_GProps::MomentOfInertia(axis)`); `:2988-3023` (`query_inertia_tensor` via `MatrixOfInertia()` + symmetric-averaging defensive); rust dispatch in `crates/reify-kernel-occt/src/lib.rs:2234-2275` (`MomentOfInertia` arm → `Value::Real`; `CenterOfMass` arm → `Value::String(centroid_json)`; `InertiaTensor` arm → `Value::List(Value::List)`).
- **Blocks:** none (kernel-side)
- **Note:** PRD §Scope.1 implementation choice matches PRD ("`BRepGProp_VolumeProperties` with density"). One subtle correctness note: PRD signature `center_of_mass(solid, density)` takes density, but the kernel implementation deliberately ignores density (centroid of uniform-density solid is density-independent). This is documented in-code (`lib.rs:2243-2244`) and pinned by `CenterOfMass_density_independence` tests; behaviourally correct, but the PRD signature reads as if density mattered. Soft-DRIFT, not flagged separately because it is documented and tested.

### M-006: OCCT FFI topology-relational — `adjacent_faces` / `shared_edges`

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-kernel-occt/cpp/occt_wrapper.cpp:3523-3578` (`adjacent_faces(shape, face_index)` walks shape's face → edges → other-faces-on-edge); `:3582-3623` (`shared_edges(shape, face_a, face_b)` intersects edges of two faces with same-face short-circuit); rust dispatch in `crates/reify-kernel-occt/src/lib.rs:2276-2320`; integration tests in `crates/reify-kernel-occt/tests/topology_selectors_integration.rs` (9 tests: every-face-has-4-adjacent, symmetric adjacency, shared-edges symmetric and equals 1 for adjacent box faces, fused-two-box stress, invalid-face-index error path).
- **Blocks:** none (kernel-side); blocked from user-facing eval by M-003
- **Note:** PRD §Scope.2 ships in full. Note the kernel-side API takes `(shape, face_index)` *integer* — not `(solid, face: Surface)` as the PRD signature suggests. The shipped path expects callers to have an extracted face *handle*, not an index, so the `try_eval_topology_selector` route (when written) will need a `GeometryHandleId → face_index` adapter or a parallel kernel method that takes a handle directly. Soft-DRIFT, pending eval wiring.

### M-007: Filtered selectors (re-exposed under feature-tag naming): `edges_by_length`, `faces_by_area`, `edges_parallel_to`, `edges_at_height` — pure-Rust over `&mut dyn GeometryKernel`

- **State:** WIRED (as a Rust pub) / PARTIAL (under PRD's user-facing claim)
- **Failure mode:** DRIFT — implementation is real but unreachable from `.ri` source for the same reason as M-003.
- **Evidence:** `crates/reify-eval/src/topology_selectors.rs:188-710` (4 base selectors + 4 `*_with_tags` variants); integration tests in `crates/reify-eval/tests/topology_filtered_selectors.rs` (229 lines, OCCT-gated); coverage in `crates/reify-eval/tests/feature_tag_selector_tests.rs` (573 lines, mock-kernel-driven, three of the four `*_with_tags` variants pinned).
- **Blocks:** PRD §Scope.4 "behaviour change: same results when topology stable, `undef`/diagnostic instead of stale results when topology changes" — invisible to user programs today, see M-003.
- **Note:** Per PRD §Background these were originally shipped under #318. Re-exposure under feature-tag naming is partly done (with-tags variants exist) but the eval-side `.ri` call path doesn't route through them.

### M-008: `FeatureTag` IR field on `CompiledRealization` (parallel-array invariant)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/types.rs:862-866` (`pub feature_tags: Vec<reify_types::FeatureTag>` with "**Invariant**: `feature_tags.len() == operations.len()`" rustdoc); `crates/reify-compiler/src/geometry.rs:1072-1102` (`derive_feature_tags` — exhaustive `match` over all 7 `CompiledGeometryOp` variants, assigning `StepKind::{Primitive, Boolean, Modify, Transform, Pattern, Sweep, Curve}`); call sites at `crates/reify-compiler/src/entity.rs:1776, 1804, 2894`; `crates/reify-compiler/tests/feature_tag_tests.rs` (282 lines: parallel-array invariant, step-kind classification, multi-op sequencing, sub_index ordering).
- **Blocks:** none
- **Note:** Satisfies PRD §Scope.3 "extend the per-op compiler pass that builds `CompiledGeometryOp` to attach a `feature_tag` to each produced face/edge handle". The exhaustive `match` is the deliberate single-source-of-truth for "new variant → forces compile error here" (mirroring `ModifyKind::ALL`).

### M-009: `FeatureTag` struct definition + `FeatureTagTable` runtime metadata table

- **State:** PARTIAL
- **Failure mode:** DRIFT — PRD says `(source_line, step_kind, sub_index)`; shipped is `(source_span: SourceSpan, step_kind, sub_index)`. Documented in-code as deliberate.
- **Evidence:** `crates/reify-types/src/geometry.rs:1693-1701` (`pub struct FeatureTag { pub source_span: SourceSpan, pub step_kind: StepKind, pub sub_index: u32 }`); `:1709-1736` (`FeatureTagTable { entries: HashMap<GeometryHandleId, FeatureTag> }` with `record` / `lookup` / `len` / `is_empty`); rustdoc at `:1683-1692` explicitly flags the source_span departure as well as the `sub_index` fragility under op insertion/reordering as a follow-up stability concern.
- **Blocks:** none
- **Note:** PRD §Scope.3 says "Tag storage: append-only on the runtime shape's metadata table (one `Vec<FeatureTag>` per `ShapeId`)". The shipped design is a `HashMap<GeometryHandleId, FeatureTag>` (single tag per id, not a `Vec`). This is a soft DRIFT because the v0.1 semantics never produce more than one tag per id, but if a future op produces a shape that reuses an existing handle id, the `HashMap::insert` would silently overwrite (`record`'s rustdoc: "the most recent tag wins"). Not exercised today but a latent surprise.

### M-010: `resolve_unique_by_tag` resolver primitive

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/src/topology_selectors.rs:780-815` (resolver: dedup via `HashSet`, fold candidates, exactly-one return; ambiguity → diagnostic + `None`); pinned by 4 unit tests at `:1481-1700` (`resolve_unique_by_tag_one_match_returns_some_with_no_diagnostics`, `_zero_matches_emits_warning_and_returns_none`, `_multiple_matches_emits_warning_and_returns_none`, `_duplicate_candidate_does_not_inflate_match_count`).
- **Blocks:** end-to-end PRD §6 "stale-tag path emits `W_TOPOLOGY_TAG_STALE` exactly once" — resolver pinned but not exercised from a `.ri` program via the user-facing call path (see M-003).
- **Note:** PRD task 6 update note (lines 184-189): "Implemented (task 2332): ... resolver building-block `resolve_unique_by_tag` ... Re-routing existing filter selectors through the resolver is tracked separately under task 5 (task 2329 in the queue)". So this is acknowledged as a building block — the resolver itself is real, the re-routing through user-facing call sites is the unfinished piece (M-007's DRIFT).

### M-011: `DiagnosticCode::TopologyTagStale` (PRD-prose mnemonic `W_TOPOLOGY_TAG_STALE`)

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-types/src/diagnostics.rs:385` (variant definition); `:355-384` (rustdoc documents canonical message form, primary/secondary label structure, and the four `*_with_tags` populators); unit tests at `:1290-1320` (round-trip variant equality, serde PascalCase pin). Round-trip from the resolver in M-010 emits this diagnostic.
- **Blocks:** none (diagnostic + resolver path is wired)
- **Note:** PRD §6 satisfied at the resolver layer. As with M-010, no integration test exercises this through a user-facing call (the `.ri`-source → `edges_at_height(...)` after a topology change → `W_TOPOLOGY_TAG_STALE` round-trip), so the PRD's "stale-tag diagnostic test: edit a profile so the tagged feature disappears → selector returns `undef`, exactly one warning, source span points back to original selector call site" acceptance bullet is **not** end-to-end pinned in v0.1.

### M-012: Runtime auto-population of `FeatureTagTable` during `execute_realization_ops` for top-level (per-realization-op) tags

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-eval/tests/feature_tag_e2e.rs:67-110` (`engine_build_records_top_level_feature_tag_for_box_realization` — `Engine::build()` populates `feature_tag_table()` non-empty with the expected per-realization-op tag); `Engine::feature_tag_table()` accessor.
- **Blocks:** none
- **Note:** This is the "step-5" wiring promised by the test header; without it the resolver M-010 would have an empty table at runtime.

### M-013: Auto-population of `FeatureTagTable` for *extracted* sub-shapes (edges/faces of a parent) via `*_with_tags` variants

- **State:** PARTIAL
- **Failure mode:** F4 (mechanism partially implemented; one of the four with-tags variants is not test-covered)
- **Evidence:** `crates/reify-eval/src/topology_selectors.rs:730-750` (`edges_at_height_with_tags`); `:230-250` (`edges_by_length_with_tags`); `:309-330` (`faces_by_area_with_tags`); `:631-680` (`edges_parallel_to_with_tags`); test file `feature_tag_selector_tests.rs` covers three of the four (`edges_by_length`, `faces_by_area`, `edges_parallel_to`) — `edges_at_height_with_tags` is covered in `feature_tag_e2e.rs` instead (OCCT-gated). All four variants record per-sub-shape tags by inheriting `step_kind` + `source_span` from a `parent_tag` and assigning `sub_index = enumerate index`.
- **Blocks:** none locally; blocks M-003 closure indirectly (a future eval wiring of `.ri` `edges_at_height(...)` must call the `_with_tags` variant to populate the table for the resolver).
- **Note:** PRD §Scope.4 path is real at the Rust level. There is no `faces_by_normal_with_tags` despite `faces_by_normal` being mentioned alongside the other three in the resolver-population rustdoc (`crates/reify-types/src/diagnostics.rs:374-377` lists only the four). PRD §Scope.4 lists "the four already-shipped filtered selectors" as `edges_at_height`, `edges_parallel_to`, `edges_by_length`, `faces_by_area` — exactly these four, so `faces_by_normal` not having a tagged variant matches PRD scope, but the user-facing v0.1 surface (`GEOMETRY_TOPOLOGY_SELECTOR_NAMES` registers `faces_by_normal`) creates an asymmetry: 5 user-facing filter selectors exist but only 4 have tagged variants.

### M-014: 3-arg `fillet(solid, edges: List<Curve>, radius)` stdlib binding (required by PRD §Worked example `fillet_top_edges`)

- **State:** FICTION
- **Failure mode:** F1 (PRD assumes mechanism in worked example; compiler binds only 2-arg form)
- **Evidence:** `crates/reify-compiler/src/geometry_modify.rs:114-126` (only `"fillet" => compile_modify_2arg("fillet", ModifyKind::Fillet, "radius", ...)` — no 3-arg arm); `examples/topology_selectors/fillet_top_edges.ri:14-21` (in-source header documents the gap: "the current compiler only wires 2-arg `fillet(solid, radius)` ... switching to the 2-arg form would fillet ALL edges and defeat the example's purpose"); `crates/reify-eval/tests/topology_selector_smoke_tests.rs:170-173` `#[ignore]` annotation `"pending 3-arg fillet(solid, edges, radius) stdlib binding"`.
- **Blocks:** PRD §Worked-example `fillet_top_edges` — neither compile-with-stdlib nor eval is reachable for this example today.
- **Note:** This is *not* one of the eleven §3.9 selector helpers, but the PRD §Worked examples reference it as if it exists. The example header acknowledges the gap. No task ID surfaced for adding the 3-arg form.

### M-015: Re-use of `Tensor<2, 3, MomentOfInertia>` stdlib type from existing tensor work

- **State:** WIRED
- **Failure mode:** N/A
- **Evidence:** `crates/reify-compiler/src/units.rs:230-237` (`moment_of_inertia` returns `Type::tensor(2, 3, Type::Scalar { dimension: DimensionVector::MOMENT_OF_INERTIA })`); `crates/reify-types/src/dimension.rs` (`DimensionVector::MOMENT_OF_INERTIA` const).
- **Blocks:** none
- **Note:** PRD §Scope.5 ("Re-use `Tensor<2, 3, MomentOfInertia>` from existing tensor work") is satisfied. The compile-time type round-trips; the runtime tensor *value* is gated on M-003 dispatch landing.

### M-016: `Bounded` dependency from `geometry-traits.md` PRD (mass-property selectors require `Bounded` arguments per "Dependencies" §)

- **State:** FICTION
- **Failure mode:** F2 (cross-PRD dependency asserted but not realised at the relevant call sites)
- **Evidence:** PRD §Dependencies: "this PRD's selectors require `Bounded` arguments for mass properties; the `Bounded` diagnostic must exist first". Cross-check: `crates/reify-compiler/src/conformance/mod.rs:289-318` (the `emit_geometry_unbounded` diagnostic is wired — see geometry-traits audit M-009), but **no consumer in topology-selectors.md applies it**. `topology_selector_result_type` for `center_of_mass`/`moment_of_inertia` (`units.rs:230-237`) has no `Bounded`-bound check; the `try_eval_topology_selector` arm for these names doesn't exist (M-003). The `mass-property` half of the dependency therefore relies on a wired-but-unconnected diagnostic.
- **Blocks:** PRD §3 mass-property triplet end-to-end correctness when an `Unbounded` solid (e.g. `half_space()`) is passed to `moment_of_inertia` — *but* the gap is academic in v0.1 because no `Unbounded` primitives currently exist in the codebase (see geometry-traits audit M-006, also FICTION). The chain is: half_space FICTION + Bounded-check-on-mass-props FICTION → composed gap is invisible.
- **Note:** Cross-PRD breadcrumb: the chain is wired enough that closing geometry-traits M-006 would make this gap user-observable.

### M-017: Topology-selector example coverage in `crates/reify-eval/tests/topology_selector_smoke_tests.rs` (Task 7)

- **State:** PARTIAL
- **Failure mode:** DRIFT vs PRD §Acceptance / §Task 7
- **Evidence:** `crates/reify-eval/tests/topology_selector_smoke_tests.rs:46-57` (`all_topology_selectors_wiring_compiles_with_stdlib`), `:71-89` (`block_inertia_compiles_with_stdlib_no_errors`); three `#[ignore]`-gated tests `:101, 173, 241` cover the eval-deepening path; PRD §Task 7 wording is "Worked-example smoke tests — the two examples from this PRD (`block_inertia`, `fillet_top_edges`) shipped as `.ri` example files under `examples/topology_selectors/` and exercised by the eval test harness". The eval portion is `#[ignore]`-gated rather than active for both worked examples. Tests do exist; they just don't run.
- **Blocks:** PRD §Acceptance ("`cargo test -p reify-eval -- topology_selectors` covers all eleven stdlib functions end-to-end through `compile_with_stdlib`") — partly satisfied (compile-time, all eleven names), missing for the eval-side coverage. PRD §Acceptance for `moment_of_inertia(box(L, W, H), ρ)` returning the analytic `(1/12) * m * (W² + H²)` tensor: the assertion lives inside an `#[ignore]`-gated test (`topology_selector_smoke_tests.rs:101+`) so the analytic accuracy isn't exercised in CI.
- **Note:** Honest authoring — the gates document exactly which mechanism unblocks them ("pending eval-side dispatch for moment_of_inertia in `try_eval_topology_selector`"), and the third example `all_topology_selectors_wiring.ri` was added beyond PRD §Task 7 to extend compile-time coverage to the full §3.9 surface. Mismatch is between PRD §Acceptance ("covers all eleven stdlib functions end-to-end") and "covers compile-time wiring; eval gated".

### M-018: Out-of-scope: imported geometry, `closest_point` between two surfaces

- **State:** WIRED (as out-of-scope text)
- **Failure mode:** N/A
- **Evidence:** PRD §Out of scope explicitly excludes Solvespace-style full attribute-persistent naming (cross-ref to v0.2 persistent-naming-v2, which is largely shipped per fused-memory: `0d38a0c8`), imported geometry, and `closest_point` between two surfaces. Cross-PRD: `topology_attribute_resolver.rs` exists in `crates/reify-eval/src/` and `TopologyAttributeStale` lives alongside `TopologyTagStale` (`diagnostics.rs:419`).
- **Blocks:** none
- **Note:** Informational. The v0.1 → v0.2 coexistence is acknowledged: `FeatureTagTable` (v0.1) and `TopologyAttributeStale` (v0.2) live alongside one another in the type and diagnostic surfaces. Audit reader should know the long-term direction is *attributes*, with feature-tags as the v0.1 building block.

## Cross-PRD breadcrumbs

- **`geometry-traits.md`** — PRD §Dependencies declares topology-selectors depends on `geometry-traits` for the `Bounded` argument check on `moment_of_inertia` / `center_of_mass`. The `Bounded` diagnostic is wired (geometry-traits audit M-009 = PARTIAL) but **no caller in topology-selectors actually consumes it** — see M-016. Disposition of geometry-traits M-006 (Unbounded primitives, also FICTION) gates whether this is ever user-visible.
- **`persistent-naming-v2.md`** — explicitly cited as the v0.2 successor (PRD §Out of scope), shipped per fused-memory `0d38a0c8` (task 2652) with `TopologyAttribute` model, attribute-lookup-primary resolver. There is observable v0.1/v0.2 coexistence in `crates/reify-types/src/geometry.rs:1738-` (v0.2 attribute-based primitives start here, with comments referencing migration off `FeatureTagTable`).
- **`field-source-kinds.md`** — out-of-scope here per PRD ("Imported geometry — selectors against imported BREP shapes are out of scope; they require their own naming scheme") — same observation as the persistent-naming-v2 PRD: imported geometry requires its own naming surface, deferred.
- **GR-001 (struct-ctor runtime eval)** — Does NOT affect this PRD. Topology selectors take `Solid`/`Surface`/`Point3<Length>` arguments — no structure-constructor calls in any of the PRD's signatures or worked examples.
- **#318 / #319 PRDs** (older filtered selectors / point-membership) — not audited, but referenced as the reference pattern. The handoff between #318's "filtered list selectors over a whole solid" and this PRD's "selectors that relate to a specific feature" is clean at the kernel layer.
- **Ad-hoc port selectors (#249, `CompiledAdHocPort`)** — PRD §Background cites #249 as the "reference implementation for feature-tag plumbing"; this PRD reuses the machinery so that "selectors here become first-class sibling functions, not new IR variants". Worth Phase 3 verification: does the actual feature-tag plumbing (`derive_feature_tags` in `geometry.rs:1072`) actually share code with the ad-hoc-port path, or are they parallel? Quick check shows `CompiledAdHocPort` is its own type in `crates/reify-compiler/src/types.rs` — they may be siblings rather than a shared core.
