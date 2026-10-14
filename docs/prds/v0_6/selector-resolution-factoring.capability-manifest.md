# Capability manifest — `selector-resolution-factoring` (review-all S13)

PRD: `docs/prds/v0_6/selector-resolution-factoring.md`. Authored 2026-10-12 (pass 1). Not yet decomposed.
Machine-readable twin: `selector-resolution-factoring.capability-manifest.yaml` (task ids are `null` until `commit_planning` stamps them).

**Code anchors** verified against main `8e6ffcac9d` (2026-10-12). `git diff a030ee7528..8e6ffcac9d` over every anchored file is empty. Bindings below cite symbols. Where a binding names a line, the line is dated to `8e6ffcac9d` and must be re-located at decompose.

Evidence vocabulary:
- `grep:` — the pattern is present on main.
- `probe:` — an executed command, with the captured result.
- `producer:` — an upstream leaf or task.
- `rejection-check` — not used: no leaf asserts a new rejection.

The D3 verification workflow is a decompose-time step and has not run.

Probes executed 2026-10-12 with `target/debug/reify` (built 2026-10-11 19:33):
- **P1:** `reify eval examples/topology_selectors/bottom_deck_selectors.ri` → rc 0, 34 stdout lines, no `error:` line. A second run is byte-identical.
- **P2:** `reify eval examples/modal/transient_step_response.ri` → rc 0. `CantileverStepResponse.tip` is a 1,251-sample length history and `tip_face = Selector(FaceSelector:…)` feeds `StepForce.at`. A second run is byte-identical.
- **P3:** `reify eval examples/topology_selectors/all_topology_selectors_wiring.ri` → rc 0, 13 lines. `neighbors`/`shared` are `undef` with recorded cause `OpContractViolation`, as they are today. A second run is byte-identical.

---

## α₀ — not a leaf (lead ruling Q2)

Performed by the pass-2 decompose session (2026-10-12) as three writes per check: 11 checks, of which 9 also have producer-metadata copies. The record is PRD §7 and the "Delivered-check path correction" section of each affected `.md` twin. Every widened check replays with its verdict unchanged at `54222c4dcd`.

## α — `selector_resolve`: one compose, `NormalCone`, one table walk; SCC dissolved

| Capability | Evidence | Verdict |
|---|---|---|
| Two composition copies exist (the thing α removes) | grep: `SelectorNode::Union(children) => {` occurs twice in `topology_selectors.rs`, in `resolve_with_attributes` and `resolve_against_carried_topology` | PASS |
| `ByNormal` predicate duplicated | grep: `dot3\([^)]*\)\.clamp\(-1\.0, 1\.0\)` matches twice in `topology_selectors.rs` (`faces_by_normal`, `resolve_leaf_against_carried`) | PASS |
| three attribute-table walks | grep: `(attr.local_index, kernel_handle.id)` ×3 in `resolve_leaf` | PASS |
| carried resolver needs only face normals | grep: `CarriedTopology::face_normals(&self) -> &[(GeometryHandleId, [f64; 3])]` is `pub`; `resolve_leaf_against_carried` reads nothing else | PASS |
| `nodes_for_faces` reads only `carried.boundary()` | grep: body of `topology_selectors::nodes_for_faces` | PASS |
| cycle edges are exactly the three in PRD §3 | grep: `use crate::compute_targets::result_topology::CarriedTopology` (topology_selectors), `use crate::topology_selectors::{…7 names}` (selector_vocabulary_v2), `crate::topology_selectors::resolve` (compute_targets/bc_resolve); the S3 skeptic's Tarjan run reproduced the 3-SCC | PASS |
| external surface to re-point is bounded | grep: integration tests `harness_topology_selector/*`, `r3b_carried_topology_selector_resolution.rs`, `topology_attribute_local_features_e2e.rs` (`role_is_face`); production `bc_resolve`, `geometry_ops` (`resolve`, `resolve_with_attributes`, `region_query_capability`, `region_selector_display_name`), `modal_ops` | PASS |
| infra wrapper pattern exists | grep: `tests/infra/test_flake_density_report.{py,sh}` plus its manifest row | PASS |
| behavioural oracle | probe P1, P2; `r3b_carried_topology_selector_resolution.rs::two_way_kernel_vs_carried_parity` exists | PASS |

## β — geometry_ops lower layers (args, pose, query, kinematic)

| Capability | Evidence | Verdict |
|---|---|---|
| D4 cycle measurement | probe: symbol cross-reference over the production region (cut at `mod tests;`) found compile↔selector_eval (`resolve_selector_to_list`, `resolve_parent_geometry_handle_arg`, `vec3_component_si` / `decode_plane`), kinematic↔selector_eval (`eval_arg_value` / `kernel_distance`, `scalar_got_label`), pose↔selector_eval (`FrameSubShapeKind` / `construct_frame_from_kernel`), and query→selector_eval (`dispatch_point3_length_reply`, `resolve_*_arg`) with selector_eval→query (`CapabilityRoute`, `route_capability`); placed_walk→pose only | PASS |
| S3 moves five predicates out first | producer: S3's predicate-move leaf (`is_geometry_query_call`, `is_geometry_consumer_call`, `is_feature_datum_projection_call`, `consumes_geometry_or_selector`, `is_symbolic_eval_wired_selector_ctor`), id supplied by the lead in pass 2 | PENDING (cross-stream) |
| dispatch-table guard survives a move | grep: `lock_dispatch_table!` ×7 and `geometry_ops/dispatch_table_uniqueness.rs` | PASS |

## γ — `selector_eval/`

| Capability | Evidence | Verdict |
|---|---|---|
| calls `selector_resolve` | producer: α | PASS (upstream) |
| size forces a directory | probe: lines from `resolve_symbolic_selector_target` to `cap_kind_translation`, minus the D4 re-homed decoders, ≈ 3,400 | PASS |

## δ — `compile/`

| Capability | Evidence | Verdict |
|---|---|---|
| seam-level oracle reaches `compile_geometry_op` from integration tests | grep: `pub mod geometry_op_characterization_probe` in `reify-eval/src/lib.rs`; `crates/reify-eval/tests/compile_geometry_op_characterization.rs` | PASS |
| compile sits above selector_eval | probe (β): compile → selector_eval edges only after the D4 re-homing | PASS |

## ε — seam tests; `geometry_ops/tests.rs` deleted; `pub(crate)` ratchet

| Capability | Evidence | Verdict |
|---|---|---|
| precedent: crate-level `#[cfg(test)]` module | grep: `mod registry_drift_tests;` in `reify-eval/src/lib.rs` | PASS |
| privacy enforces seam-only reach from outside the subtree | Rust visibility: `pub(super)` items of `geometry_ops::<child>` are not nameable from `crate::geometry_ops_seam_tests` | PASS |
| S2 contract holds today | probe: `grep -c` of every placed_walk symbol in `geometry_ops/tests.rs` = 0 | PASS |
| baseline test count | probe: `grep -c '#\[test\]' geometry_ops/tests.rs` = 542 | PASS |

## ζ — `ValidatedSketch`

| Capability | Evidence | Verdict |
|---|---|---|
| `validate` has one caller | grep: `system.validate()?` in `SystemBuilder::add_sketch` only | PASS |
| drift guards to delete | grep: `unresolved_entity_ref(` ×4 calls and `unresolved_constraint_ref(` ×5 calls in `solvespace.rs`, plus their 2 fn definitions | PASS |
| corpus oracle exists | grep: `crates/reify-constraints/tests/sketch_solve_tests.rs`, 46 `#[test]`, 50 `solve_sketch` uses | PASS |
| public seam unchanged | grep: `pub fn solve_sketch(system: &SketchSystem) -> SketchSolveResult` is present before and after, so its sidecar check is `kind: manual` (a preservation check is vacuous under the polarity gate). #6682's `sketch-path-survives` check also greps it | PASS |

## η — `WarmStatePool`

| Capability | Evidence | Verdict |
|---|---|---|
| `donate_preserving_lru` has no production caller | grep: only in docs (`engine_edit.rs` doc comments) and `warm_pool.rs` tests | PASS |
| `checkout_with_lru_stamp` production use is `checkout()` only | grep: `checkout()` body; other callers are `engine_edit` tests (4) and `warm_pool.rs` tests | PASS |
| the cost-carrying pair exists | grep: `pub fn checkout_with_lru_stamp_and_cost`, `pub fn donate_preserving_lru_with_cost` | PASS |
| behavioural oracle | grep: `crates/reify-eval/tests/warm_state_donation.rs` | PASS |

## θ — dropped (lead ruling Q1; S8 / #6010 own `from_name`)

## ι — PRD close

| Capability | Evidence | Verdict |
|---|---|---|
| terminal vocabulary + freeze header shape | overlay "PRD terminal status"; exemplar `docs/prds/v0_6/data-carrying-enums.md` header | PASS |
