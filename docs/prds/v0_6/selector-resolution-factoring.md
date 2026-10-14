# Selector resolution factoring — one compose over `SelectorNode`, a layered `geometry_ops`, typed sketch operands, a cost-safe warm pool

**Status:** authored 2026-10-12 (pass 1 of the review-all program `review-all-reify-20261010`, stream **S13**); lead rulings applied and decomposed 2026-10-12 (pass 2). The leaf rows carry real task ids.
**Milestone:** v0_6.
**Approach:** bare B, regression-oracled by the two-way boundary tests that already exist (rationale: §6 D1).
**Code anchors** verified against main `8e6ffcac9d` (2026-10-12). The finding anchors were taken at `a030ee7528` (2026-10-10). `git diff a030ee7528..8e6ffcac9d` over every file named below is **empty**. Main moves fast: cite by symbol, and re-locate lines at implementation time.
**Program:** `plans/review-all-program-reify-20261012.md` (§Streams row S13; ruling "Uncontested block"). Finding record: `plans/review-all-reify-20261010.json`.

---

## 1. Goal

The next agent who changes selector resolution, `geometry_ops`, the 2D sketch emitter or the warm pool should face one place per decision, not two:

- **One composition rule.** Adding a composite `SelectorNode` (e.g. a symmetric difference) is one arm in one function. Making another leaf resolvable from carried topology (task 4122 named `ByArea` and others as honest declines) is one closure arm. Today each needs an edit to two resolvers plus a third copy of the predicate (fk-25474a437b00).
- **A `geometry_ops` that can be changed by seam.** Moving a helper must not break hundreds of tests that pin the factoring rather than the behaviour (fk-e984283444b1). The 13,242-line file splits along its measured layering.
- **One statement of the sketch slot-kind rule.** Adding a sketch constraint or widening a slot is one edit. The emitter matches on types and has no drift guards to keep in step (fk-b4af214cc9fe).
- **A warm pool that cannot silently drop cost.** No public re-donate or checkout path resets `cost_per_byte` to 0.0 (fk-bef6f2b4851c).

**What users see:** nothing changes. Every `.ri` program evaluates, builds and checks exactly as before. §8 names the observable oracles that prove it.

## 2. Consumers (G1)

| Mechanism | Consumer on main today | Next-change consumer |
|---|---|---|
| `selector_resolve::compose` + live resolver | `geometry_ops` `resolve_selector_to_list` and `resolve_selector_target` callers (the `ResolveSelector` coercion path); `compute_targets::bc_resolve` (FEA boundary conditions) | a new composite node; a new leaf |
| carried-topology resolver over face normals | `modal_ops::resolve_location_value` (`displacement_at`, `StepForce.at`) | a carried leaf beyond `ByNormal` (task 4122's declines) |
| `NormalCone` / `normal_within` | `faces_by_normal` and the carried `ByNormal` leaf | any further normal-cone leaf |
| `CarriedTopology::nodes_for_faces` | `modal_ops` | meshing-service ε #8288 (switches it to `face_closure_nodes`) |
| `geometry_ops` child modules | the ~93 crate call sites into `geometry_ops::` (engine_build 36, engine_eval 11, invariants 13, registry_drift_tests 10, dynamics_ops 7, …) | S2's placed_walk extraction; S4's `cell_eval_ctx` routing (12 `eval_ctx_with_meta` sites); S8's registry-keyed selector dispatch (#6010); S10's typed query replies |
| `ValidatedSketch` | `solve_sketch` → `SystemBuilder::add_sketch` | constrained-2d-sketch δ #5509 and η #5512, which consume `solve_sketch` (signature unchanged) |
| cost-carrying warm-pool API | `PendingWarmSeedsGuard` (edit_source) | S4's single warm-edit path on `edit_param`'s phase methods |

No new in-engine seam is introduced (overlay G1 engine-integration sub-check). Everything here refactors the existing op-execute and ComputeNode paths.

## 3. Background and measured state (as of `8e6ffcac9d`)

**Selector resolution.** `topology_selectors.rs` has 4,252 lines; its production half ends at the first `#[cfg(test)]`, around line 1,708. It holds two resolvers:
- `resolve_with_attributes`, the live kernel resolver;
- `resolve_against_carried_topology`, the kernel-free resolver.

Each carries its own `Union`/`Intersect`/`Difference` arms: 54 and 52 stripped lines, 45 identical. `resolve_leaf_against_carried` re-states the `faces_by_normal` predicate inline, and its own doc calls the copy "byte-for-byte". The PRD `value-eval-geometry-addressing.md` §7.2 asserts two-way parity "by construction", but that parity in fact depends on two copies staying identical.

`resolve_leaf`'s `ByRole`, `CreatedByFeature` and `SplitByFeature` arms repeat one collect → filter → `sort_unstable` → map table walk three times; the shared walk is the `(attr.local_index, kernel_handle.id)` map.

The resolver also sits inside a reify-eval module SCC. The S3 skeptic's Tarjan run reproduced `{compute_targets, selector_vocabulary_v2, topology_selectors}`. S3 α's ratchet (#8673) expects its baseline row to be the wider `{compute_targets, dynamics_ops, modal_ops, selector_vocabulary_v2, topology_selectors}`, and that tool's output is authoritative. Three edges tie the selector modules in:
- `resolve_leaf` makes 12 `crate::selector_vocabulary_v2::` references;
- `selector_vocabulary_v2`'s `use crate::topology_selectors::{…}` imports 7 names back;
- `topology_selectors` imports `crate::compute_targets::result_topology::CarriedTopology` (for the carried resolver and `nodes_for_faces`), while `compute_targets::bc_resolve` calls `topology_selectors::resolve`.

**geometry_ops.** `geometry_ops.rs` has 13,242 lines and 209 fns. Its child `geometry_ops/tests.rs` has 34,226 lines and 542 `#[test]`, behind a `use super::*`. The tests call 41 private fns (130 sites) and 25 `pub(crate)` fns (479 sites), for example `literal_f64` ×274, `compile_geometry_op` ×184 and `try_eval_topology_selector` ×94.

The h14 measurement in the record names 7 partitions: args, compile, query, kinematic, selector_eval, pose and placed_walk. **Re-measured here, that partition has four cross-partition cycles.** The record's "none" symptom for compile is wrong. §6 D4 records the cycles and the symbol re-homing that breaks each one. The compile partition runs to about 4,200 lines and selector_eval to about 3,400 even after re-homing, so each needs its own child directory (§6 D5).

`geometry_ops/tests.rs` has **zero** calls into the placed_walk symbols. Those are tested from `engine_build.rs` and from `crates/reify-eval/tests/{sub_placement_surfacing,achieved_repr_tol,…}.rs`.

**Sketch emitter.** `SketchConstraint::operands` and `SketchEntity::defining_refs` in `sketch.rs` are documented as "the single statement of which slot takes which kind". They drive `SketchSystem::validate`, which is `pub(crate)` and whose only caller is `SystemBuilder::add_sketch`.

`add_sketch` and `emit_sketch_constraint` (about 294 lines) then re-resolve every operand through per-kind lookups over `HashMap<SketchEntityId, EmittedEntity>`, with 9 drift-guard call sites:
- `unresolved_entity_ref` ×4;
- `unresolved_constraint_ref` ×5.

Both guards are `tracing::error!` plus `debug_assert!(false)`.

The skeptic weakened the finding: the labels are not exact `Display` copies, and the `--profile both` debug lane would catch a drift. The duplication stands.

**Warm pool.** `WarmStatePool` has 7 public constructors, 4 donate variants and 3 checkout variants. Two variants reset `cost_per_byte` to 0.0:
- `donate_preserving_lru` has no production caller. Only docs mention it: `PendingWarmSeedsGuard::drain_into_cache_or_repool`'s doc names it, while the code calls the `_with_cost` variant.
- `checkout_with_lru_stamp` has one production caller: `checkout()` delegates to it (skeptic correction). Its other callers are 4 sites in `engine_edit`'s test module and 8 in `warm_pool.rs`'s own tests.

Two more defects: the `contains()` doc contrasts it with a `retrieve` method that does not exist, and `test_events_cap` is a `#[cfg(test)]` field on the production struct.

## 4. Sketch of approach

### 4.1 `selector_resolve` — a new module above `topology_selectors` and `selector_vocabulary_v2`

`crates/reify-eval/src/selector_resolve.rs` is a `pub mod`. It holds:

```rust
/// Fold a selector tree with one composition rule: Union = first-seen-order dedup,
/// Intersect = first child's order filtered by membership in every other child,
/// Difference = a minus b; every combinator dedups by GeometryHandleId (K3).
pub(crate) fn compose<F>(selector: &SelectorValue, leaf: &mut F) -> Result<Vec<GeometryHandleId>, QueryError>
where F: FnMut(SelectorKind, &GeometryHandleRef, &LeafQuery) -> Result<Vec<GeometryHandleId>, QueryError>;

pub fn resolve(...)                      // unchanged signature (table-free wrapper)
pub fn resolve_with_attributes(...)      // unchanged signature; compose + live leaf closure
pub fn resolve_against_face_normals(     // the carried mode; compose + ByNormal closure, honest decline otherwise
    selector: &SelectorValue,
    face_normals: &[(GeometryHandleId, [f64; 3])],
) -> Result<Vec<GeometryHandleId>, QueryError>;
```

Further moves into `selector_resolve`, all from `topology_selectors`'s resolver partition:
- `resolve_leaf` (the live leaf arms);
- `role_is_face`;
- `extremal_axis_from_index` and `extremal_sense_from_max`;
- `extract_by_kind`;
- `region_query_capability` and `region_selector_display_name`.

The three attribute-table arms call one `fn attribute_table_matches(table, pred) -> Vec<GeometryHandleId>`, which owns the `(local_index, id)` canonical sort once.

These stay in `topology_selectors.rs`: the leaf predicates (`faces_by_normal`, `edges_by_length`, …), the query batching (`filter_by_value`, `query_per_subshape`), `SubKind`/`make_sub_handle`, and the `parse_*` decoders, which S10 later deletes. The file gains:

```rust
/// A validated normal cone: a unit target direction and an angular tolerance in [0, π].
pub(crate) struct NormalCone { /* target: [f64;3] (unit), tol_rad: f64 */ }
impl NormalCone {
    pub(crate) fn new(fn_name: &'static str, dir: [f64; 3], tol_rad: f64) -> Result<Self, QueryError>; // validate_angular_tol + normalize3, today's messages verbatim
    pub(crate) fn normal_within(&self, id: GeometryHandleId, raw_normal: [f64; 3]) -> Result<bool, QueryError>; // degenerate-normal error verbatim, then acos(clamp(dot)) <= tol
}
```

`faces_by_normal` and the carried `ByNormal` closure both call `NormalCone`. The predicate then has one home, and the §7.2 parity holds because both paths execute the same function.

`nodes_for_faces` becomes `CarriedTopology::nodes_for_faces(&self, faces)` in `compute_targets/result_topology.rs`, beside the `boundary()` it reads. `modal_ops` calls `resolve_against_face_normals(selector, carried.face_normals())`.

Resulting edges: `selector_resolve` → `topology_selectors` and → `selector_vocabulary_v2`; `selector_vocabulary_v2` → `topology_selectors`; `compute_targets` → `selector_resolve`. `topology_selectors` imports neither `selector_vocabulary_v2` nor `compute_targets`. The 3-module SCC is gone: heuristic 9, appropriate nesting, and heuristic 13.

### 4.2 `geometry_ops` — six children along the re-measured layering, placed_walk left for S2

`geometry_ops.rs` becomes the parent. It holds the `pub(crate) mod` declarations plus the placed_walk partition and `realization_is_aux`, which S2 extracts later. Children are listed bottom → top. Each imports only from those listed before it, never from the parent file (heuristic 13: no reach-back):

1. `geometry_ops/args.rs`: named- and positional-argument decoding, plus the `DEGENERATE_*`/`GEOMETRY_EPSILON` constants. Re-homed in: `decode_plane`, `decode_axis`, `unit_vector3`, `point3_components`, `eval_arg_value`, the `resolve_*_arg` helpers and the `*_got_label` helpers (D4).
2. `geometry_ops/pose.rs`: the pose and frame value math (`construct_frame_from_kernel` … `push_pose_diagnostic_deduped`). Re-homed in: `FrameSubShapeKind` (D4).
3. `geometry_ops/query.rs`: conformance, geometry and scalar query dispatch. Re-homed in: `CapabilityRoute`, `route_capability`, `gate_query_capability`, and the `dispatch_*` reply decoders (D4).
4. `geometry_ops/kinematic.rs`: kinematic and swept-kinematic queries, `KinematicHelper`, `kernel_distance`.
5. `geometry_ops/selector_eval/`: a directory module. It holds `try_eval_topology_selector` with `TopologySelectorHelper`, the symbolic path, the feature and self datum projections, `resolve_selector_to_list`, `dispatch_shared_edges`/`dispatch_filtered_subhandles`, and `try_eval_ad_hoc_selector`. It is split into children measured at implementation time (D5).
6. `geometry_ops/compile/`: a directory module. `mod.rs` holds `compile_geometry_op` and `resolve_geom_ref_impl`. One child per op family (`primitive`, `modify`, `transform`, `pattern`, `sweep`, `curve`, `profile`) holds its fns **and** its `*_COMPILERS` table with its `lock_dispatch_table!`. Curated-selector argument hydration lives in compile, which sits above selector_eval: `prehydrate_inline_curated_selector_args`, `InlineSelectorMemo`, `CuratedEdgeLabels` and `resolve_curated_edges_p2`.

Visibility rule: an item is `pub(crate)` only if production code outside `geometry_ops` calls it. Items shared between siblings are `pub(super)`; everything else is private.

Helper-level tests move into each child's own `#[cfg(test)] mod tests;`. These test files follow the same heuristic 14 rule: a test file over the alarm partitions along its child's partition.

Tests that call helpers of more than one child are not moved. They are rewritten to drive the seam the engine uses: `compile_geometry_op`, the `try_eval_*` entry points, or an `Engine` fixture.

### 4.3 Seam tests that can only see seams

The seam-level remainder of `geometry_ops/tests.rs` moves to a crate-level `#[cfg(test)] mod geometry_ops_seam_tests;`, declared in `lib.rs` beside `mod registry_drift_tests;` (precedent). Because it sits outside the `geometry_ops` subtree, Rust privacy lets it see only `pub(crate)` items. A private helper or a `pub(super)` sibling helper is therefore a compile error, not a review judgement.

The one remaining way to reach an internal is to widen an item to `pub(crate)` for a test. That widening is held by the adoption ratchet, which caps the `pub(crate)` count under `geometry_ops/` (§5). `geometry_ops/tests.rs` is deleted.

### 4.4 `ValidatedSketch` — the slot table resolves to types once

`SketchSystem::validate(&self) -> Result<ValidatedSketch<'_>, SketchBuildError>` stays `pub(crate)`. `ValidatedSketch` holds entities and constraints in declaration order (O9), with every operand already a typed reference: `PointRef`, `LineRef`, `CurveRef` (circle or arc), `ArcRef`, and `AnchorRef` (point or line). Each reference is an index into a per-kind table, and only `validate` can construct one, through a private field.

The slot table *is* that conversion. Each `SketchConstraint`/`SketchEntity` arm resolves its operands through a typed accessor (`ix.point(a)?`, `ix.line(l)?`, `ix.curve(c)?`, `ix.arc(a)?`, `ix.point_or_line(e)?`). The accessor called is the statement of the slot's kind, so `operands()`/`defining_refs()` cease to exist as a second table (heuristic 11).

`add_sketch` emits from per-kind `Vec<Slvs_hEntity>` tables indexed by those references. Every lookup is total by construction, so `EmittedEntity`'s `as_point`/`as_curve` kind checks are deleted. So are `unresolved_entity_ref`, `unresolved_constraint_ref` and their 9 call sites (heuristic 10, one mechanism; heuristic 12, no kind labels as strings).

Unchanged:
- every `SketchBuildError` variant and the first-error order (entities before constraints; literals, then references' kinds, then repeats);
- `solve_sketch`'s signature and `SketchSolveResult`.

### 4.5 `WarmStatePool` — no public path drops cost

These are deleted:
- `donate_preserving_lru`;
- `checkout_with_lru_stamp`; `checkout()` delegates to `checkout_with_lru_stamp_and_cost` instead;
- the `#[cfg(test)] test_events_cap` field and its `cfg` branch in `events_cap_effective`.

The events cap becomes an ordinary `events_cap: usize` field, set through one `pub(crate) fn with_limits(budget_bytes: Option<usize>, events_cap: usize)`. `with_budget` delegates to it with `MAX_BUFFERED_EVENTS`, and the release-only trim test uses it.

The stale docs are fixed:
- `contains()`'s `retrieve` contrast;
- `drain_into_cache_or_repool`'s doc and the step-(14b) block comment in `edit_source` that name `donate_preserving_lru`.

The test callers in `engine_edit`'s test module and in `warm_pool.rs`'s tests re-point to the `_and_cost`/`_with_cost` pair. `donate()` (cost 0.0) stays: it is the documented "unknown cost, evict first" default for non-Compute nodes and has production callers (§9).

## 5. Adoption ratchet (INV candidate 5, extract-with-ratchet)

One new infra member is added: `tests/infra/test_selector_resolution_ratchet.py`. It is stdlib `unittest`, Python per the infra policy, and is discovered through its thin `tests/infra/test_selector_resolution_ratchet.sh` wrapper. The same diff adds its `run-all-classification.manifest` row (`pool`), per the overlay's drift-guard rule.

It counts patterns in **production regions only**. Each `.rs` file is cut at its first top-level `#[cfg(test)]`, and test-only files (`**/tests.rs`, `**/tests/**`, `geometry_ops_seam_tests*`) are excluded. Each count is pinned at its post-extraction value:

| count | pattern (scope `crates/reify-eval/src/`) | ceiling | lands in |
|---|---|---|---|
| set-composition arms | `SelectorNode::Difference\(\s*a\s*,\s*b\s*\)` (two bound operands, which a first-leaf walk's `Difference(a, _)` never has) | 1 (in `selector_resolve::compose`) | α |
| normal-cone predicate | `dot3\([^)]*\)\.clamp\(-1\.0,\s*1\.0\)` over `topology_selectors.rs`, `selector_resolve.rs`, `selector_vocabulary_v2.rs` | 1 (in `NormalCone::normal_within`) | α |
| attribute-table walk | `\(attr\.local_index,\s*kernel_handle\.id\)` | 1 (in `attribute_table_matches`) | α |
| `geometry_ops` crate surface | `^\s*pub\(crate\)\s+(fn\|struct\|enum\|const\|static\|type\|mod)\b` under `geometry_ops.rs` and `geometry_ops/**` | the count at ε's landing | ε |

A count above its ceiling fails red, naming the file, line and pattern. A count below its ceiling also fails red, with a message to lower the ceiling in the same diff. That makes the ratchet monotone and keeps it from going stale, matching the INV-7 shrink rule. The ceilings live as named constants in the `.py`, their one home.

## 6. Resolved design decisions

- **D1: bare B, oracled by the existing H.** This PRD changes no contract. The H component the original selector PRDs built is the regression oracle and stays untouched:
  - the `r3b_carried_topology_selector_resolution.rs` two-way parity test (kernel versus carried node-set);
  - `selector_boundary_gate.rs`;
  - the `selector_vocabulary_v2_*` e2e suites.
  
  Integration tests are edited for import paths only. A test whose *assertions* change is a scope breach that the reviewer blocks.
- **D2: the carried mode takes face normals, not `&CarriedTopology`.** `resolve_against_face_normals` lets `selector_resolve` import nothing from `compute_targets`. `nodes_for_faces` moves onto `CarriedTopology`. Keeping either in `topology_selectors` or `selector_resolve` re-closes the cycle through `bc_resolve`.
- **D3: `NormalCone` owns validation.** The carried leaf and `faces_by_normal` share the validated target as well as the inner predicate. A future normal-cone leaf cannot skip `validate_angular_tol` (heuristic 10, at construction). The error strings are kept verbatim; `modal_ops` surfaces them.
- **D4: the h14 partition's membership is corrected to break four cycles; the seven partition names are kept.** The re-measurement (symbol-name cross-reference over the production region at `8e6ffcac9d`) found:
  - compile ↔ selector_eval: compile calls `resolve_selector_to_list`, `resolve_parent_geometry_handle_arg` and `vec3_component_si`; selector_eval calls `decode_plane`.
  - kinematic ↔ selector_eval: kinematic calls `eval_arg_value`; selector_eval calls `kernel_distance` and `scalar_got_label`.
  - pose ↔ selector_eval: pose uses `FrameSubShapeKind`; selector_eval calls `construct_frame_from_kernel`.
  - query → selector_eval, through `dispatch_point3_length_reply` and the `resolve_*_arg` helpers, with selector_eval → query through `CapabilityRoute`/`route_capability`.
  
  The fix moves the argument decoders down to args, `FrameSubShapeKind` to pose, and the shared reply decoders and capability gate to query. It also places compile **above** selector_eval. The layering in §4.2 is that result. The implementer re-runs the measurement on the moved tree and records any residual edge in the commit message (heuristic 14 protocol).
- **D5: compile and selector_eval are directories.** At about 4,200 and 3,400 lines, a single child file would itself be an alarm. compile partitions by op family, with each family's `*_COMPILERS` table beside its fns; the `lock_dispatch_table!` + `dispatch_table_uniqueness.rs` guard keeps working, re-pointed. selector_eval partitions by the record's sub-clusters (symbolic, datum projection, live helper dispatch, ad-hoc) into whatever the implementer's measurement shows passes heuristic 13. `try_eval_topology_selector`'s roughly 800-line body moves whole. Its reshaping is S8's (registry-keyed dispatch, #6010).
- **D6: test placement follows visibility.** Tests that call only one child's internals move into that child's tests. Tests that call only `pub(crate)` seams move to `geometry_ops_seam_tests`. Tests that span children's internals are rewritten to a seam. This rule is mechanical, which answers the finding's "no way to tell factoring tests from behaviour tests": a test that needs two modules' internals was a factoring test.
- **D7: placed_walk stays in the parent for S2.** This PRD owns the G4 seam "geometry_ops/tests.rs rewrite boundary shared with S2's placed_walk extraction". The contract is in §7.
- **D8: `ValidatedSketch` borrows.** `ValidatedSketch<'a>` borrows the `SketchSystem`, so seeds and spans are not copied and the system cannot change between validation and emission (heuristic 8). It stays `pub(crate)`. `solve_sketch` remains the crate's only sketch seam.
- **D9: the warm pool's minimal deletion.** The scope is the two cost-dropping wrappers plus the test seam. Collapsing the 7 constructors and renaming the `_and_cost` pair are not in scope. S4 reshapes the warm-edit path, and a rename now would be churn S4 redoes.

## 7. Cross-PRD relationship and seam owners (G4)

| Seam | Owner | This PRD's role | Contract |
|---|---|---|---|
| `geometry_ops/tests.rs` rewrite boundary vs S2's placed_walk extraction | **S13** (program G4 table) | owner | After ε: `geometry_ops.rs` = the `pub(crate) mod` declarations + the placed_walk partition (`walk_placed_realizations`, `PlacedPreFilter`, `ExportBody`, `root_template_indices`, `reachable_template_indices`, `non_final_realization_indices`, `surface_export_bodies`, `surface_subtree`, `realization_is_aux`) and nothing else. No geometry_ops test module references a placed_walk symbol (zero today; ε keeps it zero). placed_walk imports `pose` downward only. S2 moves the partition verbatim to `geometry_ops/placed_walk.rs`, or to a crate-level module, with `realize_sub_override_handles` passed in; S2 touches no S13 test module. |
| `registry_drift_tests.rs` pins on geometry_ops predicates | S3 | consumer | S3 β #8687 moves the nine predicates (into `expr_classify.rs`, `realization_inputs.rs` and `structural_query.rs`, and adds `SymbolicSelectorCtor` in `expr_classify.rs`, while geometry_ops keeps `TopologySelectorHelper` with an exhaustive map). S13-β depends on #8687. β and γ re-point the remaining `crate::geometry_ops::` paths in `registry_drift_tests.rs` as **path-only** edits, with no assertion changes (lead ruling Q4: sanctioned). |
| intra-crate module-SCC ratchet (INV-ARCH-1, INV candidate 7) | S3 (+S7) | consumer | S3 α #8673 builds `scripts/assert-module-scc.py` and `scripts/module-scc-baseline.json`. S13-α depends on #8673 and, in its own diff, lowers the reify-eval row with `print`: `selector_vocabulary_v2` and `topology_selectors` leave it. Any residual `{compute_targets, dynamics_ops, modal_ops}` cycle the tool reports is recorded in α's commit message, not broken by α. |
| `run-all-classification.manifest` rows for a new wrapper | S19 | consumer | α adds one `pool` row in the format S19 owns. |
| `eval_ctx_with_meta` → `cell_eval_ctx` | S4 | upstream of S4 (S4 is wave 2) | The 12 geometry_ops `eval_ctx_with_meta` call sites move into the children unchanged. S4 edits them in their new homes. |
| `edit_source` / `PendingWarmSeedsGuard` deletion | S4 | upstream of S4 | η leaves `drain_into_cache_or_repool` calling `donate_preserving_lru_with_cost`. S4's `edit_source` deletion takes the guard and its tests with it. |
| typed `GeometryQuery` replies (`parse_*` deletion) | S10 | upstream of S10 | `parse_xyz_value`, `parse_bbox_*` and `parse_flat_number_object` stay in `topology_selectors.rs`. The `dispatch_*` reply decoders move to `geometry_ops/query.rs`, where S10 replaces them. |
| `lib.rs` partition | S1 | upstream of S1 | ε adds one `#[cfg(test)] mod geometry_ops_seam_tests;` line beside `mod registry_drift_tests;`. |
| compute targets → new analysis crate | S5 | upstream of S5 | `CarriedTopology::nodes_for_faces` moves with `result_topology.rs`, and `bc_resolve` calls `crate::selector_resolve::resolve`; S5 decides the crate edge. |
| `value-eval-geometry-addressing.md` §7.2 two-way parity | that PRD (shipped R3b) | preserved | The parity becomes structural (one predicate). The `r3b_*` test is the oracle. |
| `topology-selector-value-type.md` "single executor" (`resolve`, task 4118) | that PRD | preserved | `resolve` moves to `selector_resolve`; it is still the single executor. |
| meshing-service ε #8288 (pending): `nodes_for_faces` reads `face_closure_nodes` | meshing-service PRD | coordinate | α moves `nodes_for_faces` to `CarriedTopology::nodes_for_faces`. #8288's `topology-selectors-read-face-closure` check was widened to include `compute_targets/result_topology.rs` at pass 2 (three writes, §7 note). A cross-reference amends #8288's item 7. |
| solver-unification ξ #6682 (pending): retire the 3D path in `solvespace.rs`, KEEP the sketch half | geometry-algebra-solver-unification PRD | coordinate | ζ rewrites the sketch half #6682 lists as "KEEP, UNTOUCHED". The two edit disjoint halves of one file, so the file lock serialises them and neither depends on the other. Whichever lands second reads the other's shape; #6682's KEEP list must stop naming `EmittedEntity`/`unresolved_*` once ζ lands (cross-reference). ζ preserves `pub fn solve_sketch`, which #6682's `sketch-path-survives` check greps. |
| constrained-2d-sketch δ #5509 / η #5512 (pending) | constrained-2d-sketch PRD | upstream, signature-preserving | They consume `solve_sketch(&SketchSystem) -> SketchSolveResult`, which is unchanged. |

**Delivered checks pinned to files this PRD moves.** Other PRDs' sidecars carry `kind: grep` checks whose `paths` name exactly `crates/reify-eval/src/geometry_ops.rs` or `crates/reify-eval/src/topology_selectors.rs`. The producers' `metadata.delivered_checks` carry copies. Once a pattern's code moves into a child file, a `present` check reads absent and fires DEP_CAPABILITY_NOT_DELIVERED against a delivered capability (the esc-6739-1 class). An `absent` check goes vacuous.

Measured at `8e6ffcac9d` across `docs/prds/**/*.capability-manifest.yaml`, each pattern was located in the file. 11 checks are affected, because their match leaves the parent file:
- angle-dimension-completion α #6179 (`CURVATURE`, pending);
- angle-units-surface-convergence γ #5779 and δ #5780 (`angle_spec\(\)`);
- angle-units-surface-convergence ε #5781 ×2 (absent checks, which would go vacuous);
- implicit-solids ι #8401 (`"sdf_from_solid"`, pending; not yet present);
- placement-relations-belt γ #5439 (`fn try_eval_kinematic_query`, pending);
- units-length-gate-completion δ #5745 (`fn point3_components`), ζ #5747 (`DimensionVector::DIMENSIONLESS`) and #5623 (`fn required_length_arg`);
- meshing-service ε #8288 (`face_closure_nodes\(` on `topology_selectors.rs`, pending).

Not affected: gui-on-demand-measurement ζ #6745 and precision-nominal γ1 #6171 ×2 and δ #6168. Their matches sit in `surface_subtree`, in the placed_walk region, which stays in the parent. Every `modal_ops.rs`, `engine_edit.rs` and `solvespace.rs` check also keeps its pattern.

Pre-existing and not caused here: units-length-gate-completion #5658's check `fn eval_all_args_to_f64` already matches nothing on main, since the fn is now `eval_all_args_to_values` (cross-reference). **Done at pass 2 (2026-10-12, lead ruling Q2, so no leaf):** the decompose session widened all 11 checks with the three writes (sidecar `.yaml`, `.md` twin section "Delivered-check path correction", and producer `metadata.delivered_checks` on #6179, #5779, #5780, #5781, #8401, #5439, #5745, #5747 and #8288). #5623 and #5658 carry no metadata copy, so only their sidecar rows changed. #5658's stale pattern was corrected to `fn eval_all_args_to_values`. Every widened check replays with its verdict unchanged at main `54222c4dcd`. #5781's two `absent` checks exclude `geometry_ops/**/tests.rs` and `geometry_ops/**/tests/**`, because a negative-assertion test quotes the banned string.

## 8. Decomposition plan

Leaf labels are `S13-<greek>`; task ids are assigned at decompose. Every leaf's regression oracle includes the unchanged-behaviour oracles below. Each is observed deterministic run-to-run on the debug binary built 2026-10-11, probed 2026-10-12 (`reify eval`, exit 0):
- **O1:** `reify eval examples/topology_selectors/bottom_deck_selectors.ri`. Its `mass` depends on live kernel resolution of `edges_parallel_to`/`edges_at_height` for curated fillets; 34 stdout lines, no `error:`.
- **O2:** `reify eval examples/modal/transient_step_response.ri`. `StepForce.at` is a `faces_by_normal` selector resolved against carried topology; `tip` is a finite 1,251-sample history.
- **O3:** `reify eval examples/topology_selectors/all_topology_selectors_wiring.ri`.

The acceptance for each leaf below includes: O1–O3 stdout and stderr byte-identical before and after the leaf's diff on the same host. Diffs are within one run session, never against a committed golden. `cargo test -p reify-eval -p reify-constraints` must also be green.

- **S13-α₀ (not a leaf; done at pass 2).** The delivered-check path widening for the 11 checks of §7 was performed by the decompose session before filing (lead ruling Q2). Its record is the §7 note and the "Delivered-check path correction" section in each affected `.md` twin.
- **S13-α #8747: `selector_resolve` module: one `compose`, `NormalCone`, one attribute-table walk; SCC dissolved** (§4.1, §5 rows 1–3). Edits `topology_selectors.rs`, the new `selector_resolve.rs`, `selector_vocabulary_v2.rs` (doc links only), `compute_targets/result_topology.rs`, `compute_targets/bc_resolve.rs`, `modal_ops.rs`, `geometry_ops.rs` (call-site paths only), `lib.rs` (`pub mod selector_resolve`), integration-test import paths, and the ratchet member, its wrapper and its manifest row.
  - *Signal:* O1–O3 identical, plus `r3b_carried_topology_selector_resolution.rs::two_way_kernel_vs_carried_parity` green with unchanged assertions.
  - *Structural signal:* `topology_selectors.rs`'s production region contains no `selector_vocabulary_v2` and no `compute_targets` reference. The three ratchet counts equal 1.
  - *Structural signal (INV-ARCH-1):* `cargo test -p reify-build-utils --test module_scc_ratchet` is green, with the reify-eval row of `scripts/module-scc-baseline.json` lowered in α's own diff (`assert-module-scc.py print`): `selector_vocabulary_v2` and `topology_selectors` are no longer members, and any residual `{compute_targets, dynamics_ops, modal_ops}` cycle the tool reports is recorded in the commit message.
  - *Prereqs:* #8673 (S3 α: the INV-ARCH-1 tool and baseline). *Keys:* fk-25474a437b00. *INV:* candidate 5 (ratchet member `test_selector_resolution_ratchet`, with a `pool` row added to `tests/infra/run-all-classification.manifest` in today's format); INV-ARCH-1 (lowers its baseline row).
- **S13-β #8748: geometry_ops lower layers: `args`, `pose`, `query`, `kinematic` children** (§4.2 children 1–4, D4 re-homing, helper tests moved per D6). *Signal (intermediate):* O1–O3 identical, plus `dispatch_table_uniqueness` and `registry_drift_tests` green (path-only edits). It unlocks γ. *Prereqs:* α (both edit `geometry_ops.rs`, so the chain avoids lock thrash) and **#8687** (S3 β: the nine predicates move below the SCC; geometry_ops keeps `TopologySelectorHelper` behind an exhaustive `SymbolicSelectorCtor` map). *Keys:* none (intermediate to ε).
- **S13-γ #8749: geometry_ops `selector_eval/` child** (§4.2 child 5, D5). Calls `crate::selector_resolve` (from α). *Signal (intermediate):* O1–O3 identical, plus the `harness_topology_selector` suite green with unchanged assertions. It unlocks δ. *Prereqs:* β. *Keys:* none.
- **S13-δ #8750: geometry_ops `compile/` child, one family per file with its dispatch table** (§4.2 child 6, D5). It subsumes pending #7381 ("split geometry_ops.rs into per-op-family modules"), which was cancelled at pass 2 with `x_superseded_by` = [#8748, #8749, #8750, #8751]. *Signal (intermediate):* O1–O3 identical, plus `compile_geometry_op_characterization.rs` (through `geometry_op_characterization_probe`) and `dispatch_table_uniqueness` green. It unlocks ε. *Prereqs:* γ. *Keys:* none.
- **S13-ε #8751: seam tests that can only see seams; `geometry_ops/tests.rs` deleted; `pub(crate)` ratchet** (§4.3, D6, §5 row 4, §7 S2 contract).
  - *Signal:* `crates/reify-eval/src/geometry_ops/tests.rs` does not exist. `geometry_ops_seam_tests` compiles with no `use super::*` reach into `geometry_ops` internals; that is structural, since it sits outside the subtree.
  - The total `#[test]` count across the geometry_ops child test modules plus `geometry_ops_seam_tests` is ≥ 542, minus any duplicates the commit message names one by one.
  - O1–O3 identical. The ratchet's fourth count is pinned.
  - *Prereqs:* δ. *Keys:* fk-e984283444b1. *INV:* candidate 5 (fourth count of the same member).
- **S13-ζ #8752: `ValidatedSketch`: typed operands, emitter matches on types, drift guards deleted** (§4.4, D8).
  - *Signal (intermediate; the user-facing sketch surface is #5508/#5509, still pending):* `crates/reify-constraints/tests/sketch_solve_tests.rs` (46 tests) green with unchanged assertions. For every corpus system, the `SketchSolveResult` and the `Malformed(SketchBuildError)` variants and fields are `==` before and after (closed-form, data-in path, so equality is the right regime, as in #6682's B14).
  - `fn unresolved_constraint_ref` and `fn unresolved_entity_ref` are absent from `solvespace.rs`.
  - *Consumer:* #5509, #5512. *Prereqs:* none. *Keys:* fk-b4af214cc9fe (weakened; kept at weakened weight per ruling).
- **S13-η #8753: `WarmStatePool`: no public path drops cost; cfg(test) field becomes a constructor parameter** (§4.5, D9). *Signal:* `crates/reify-eval/tests/warm_state_donation.rs` green with unchanged assertions. `pub fn donate_preserving_lru(`, `pub fn checkout_with_lru_stamp(` and `#[cfg(test)]` on a `WarmStatePool` field are all absent. `contains()`'s doc names no `retrieve`. *Consumer:* S4's warm-edit path. *Prereqs:* none. *Keys:* fk-bef6f2b4851c (weakened).
- **S13-ι #8754: PRD close.** It stamps the Status `SHIPPED` with the landed leaf ids, adds the AS-AUTHORED freeze paragraph and the LIVE/AS-AUTHORED map, and applies the same header to the `.capability-manifest.md`. *Signal:* the committed header. *Prereqs:* every other S13 leaf (a cancelled one counts as satisfied).

Dependency summary: #8673 → α → β → γ → δ → ε; #8687 → β; ζ and η are independent; everything → ι. θ (`TopologySelectorHelper::from_name`) was dropped by lead ruling Q1; S8 and #6010 own it.

## 9. Out of scope

- `donate()`'s cost-0.0 default (production callers; documented semantics), the 7 `WarmStatePool` constructors, and the `_and_cost` naming (D9).
- `try_eval_topology_selector`'s body and the two selector name maps: S8 / #6010 (lead ruling Q1: θ dropped from S13).
- Splitting `solvespace.rs`, recorded as h14 `stays_whole:false` with 4 partitions. #6682 deletes its 3D half first; the remainder is then re-measured.
- `topology_selectors.rs`'s in-file test module size, and the `parse_*` decoders (S10).
- The three "first leaf" walks: `region_query_capability::first_leaf_query`, `region_selector_display_name::first_leaf_with_kind`, and `geometry_ops::first_leaf_target`. All three move with this PRD; they are not merged (lead ruling Q5: leave).
- Any `.ri` language surface. The docs-truth gate is not triggered: no grammar, builtin, diagnostic or CLI behaviour changes.

## 10. Open questions (tactical)

1. **Residual-edge recording.** If β–δ's re-measurement finds an edge D4 did not predict, the implementer moves the symbol down a layer when it is a pure helper. Otherwise they record the edge in the commit message and the task record. *Decide during:* β/γ/δ.
2. **selector_eval's child names.** These are chosen by the implementer's heuristic 13 measurement (D5). *Decide during:* γ.

## 11. Lead rulings on the pass-1 questions (2026-10-12; program doc §"Resolved decisions — pass 1 open questions")

- **Q1** → (a): θ is dropped. fk-07c78c4ca710 is S8's key, and deferred #6010 already moves the helper map. S13 files no held leaf.
- **Q2** → (a): the pass-2 decompose session performed the delivered-check path widening (the three writes per check) before filing. It is not a leaf; see the §7 note.
- **Q3** → (a): D4/D5 accepted. Code wins over the record's "none" (4 cycles measured).
- **Q4** → (a): path-only edits to `registry_drift_tests.rs` by β/γ are sanctioned.
- **Q5** → (b): the three first-leaf walks stay as recorded in §9.
