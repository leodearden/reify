# Capability manifest — occt-crate-internals (S11, review-all-reify-20261010)

PRD: `docs/prds/v0_6/occt-crate-internals.md`. Machine-readable twin: `docs/prds/v0_6/occt-crate-internals.capability-manifest.yaml` (schema v1, validated with dark-factory `shared.capability_manifest.parse_capability_manifest`: 10 tasks, 31 capabilities). Task ids are unstamped; `commit_planning` stamps them at decompose time.

**Evidence** taken at main `8e6ffcac9d` (2026-10-12). The OCCT crate is unchanged since the record's `a030ee7528`. Behavioural probes used `target/debug/reify` (built 2026-10-11, after the last crate change) with `LD_LIBRARY_PATH=/opt/reify-deps/tbb-pin:/usr/lib/x86_64-linux-gnu:/opt/reify-deps/lib`. All three fixtures parse under the grammar gate: `tree-sitter parse --quiet`, isolated cache, rc=0.

Delivered checks were evaluated against main at authoring time. Premise rows hold now. Deliverable rows are "not yet" by construction, and each currently fails in the non-vacuous direction: an `absent` check sees its pattern present today, and a `present` check sees it absent.

| leaf | capability | binding (evidence) | verdict | delivered_check |
|---|---|---|---|---|
| α | handle-test-module-reaches-only-public-api | record h14 row for handle.rs (test module): reaches only `super::OcctKernelHandle` + reify_ir | PASS | grep `^mod tests` absent in `src/handle.rs` |
| α | harness-layout-contract-and-cap | `tests/infra/test_harness_kloc_cap.sh` CAP_LINES=20000. harness_occt has 12,292 module lines and harness_occt_measurement 9,275, so a new unit is likely | PASS | grep `CAP_LINES=20000` present |
| α | test-count-instrument | `scripts/cargo-test-tally.py` exists | PASS | path present |
| β | public-fixture-constructors-exist | 18 pub `*_for_test` constructors behind `test-fixtures`; the self-dev-dep enables the feature | PASS | grep `pub fn store_rect_face_for_test` present in `src/lib.rs` |
| β | private-duplicate-fixture-api-deleted | the cfg(test) `store_raw` is the duplicate. Gap: a rename evades the pattern; the f60 re-measurement is the cover | PASS | grep `fn store_raw` absent in `src` |
| γ1 | kernel-owns-history-dispatcher | premise: today `fn execute_with_history` exists only in handle.rs, stubs.rs, reify-ir, reify-geometry and test kernels; the op match sits in `spawn()` with `.as_f64()` reads | PASS | grep `pub fn execute_with_history` present in `src/lib.rs` |
| γ1 | length-tripwire-observable-on-test-thread | `reify_ir::arm_length_tripwire_assert` is a thread-local RAII arm and panics under debug_assertions only; it does not cross the handle's worker thread | PASS | grep `pub fn arm_length_tripwire_assert` present |
| γ1 | op-args-module-split | h14 row: partition symptom "none" | PASS | path `src/op_args.rs` present |
| γ1 | adoption-ratchet-member | INV candidate 5 shape = `version_id_discipline_gate.rs`; added as a module of the existing harness_occt unit | PASS | path `tests/harness_occt/inline_pattern_ratchet.rs` present |
| γ2 | one-sweep-history-decoder | `decode_six_buffer_history` precedent; 3 `sweep_op_history_face_modified` call sites | PASS | grep `fn decode_sweep_history` present in `src` |
| γ2 | one-selection-mapper | 4-5 "does not belong to" sites, two algorithms | PASS | grep `fn map_selection_to_indices` present |
| γ2 | one-subshape-extractor | three near-identical `extract_*` bodies | PASS | grep `fn extract_subshapes` present |
| ζ | centroid-misroute-reproduces | fixture 1 prints `c_moved = point(0 m, 0 m, 0 m)`; `c_plain` is about 1e-19 m, so the square is centred and the post-fix target is (0.01 m, 0, 0) | PASS | path `tests/prd-gate/fixtures/occt_internals_centroid_translated_profile.ri` |
| ζ | guard-bypass-via-translate-reproduces | fixture 2: `v_direct` undef with "got Compound"; `v_moved` = 1.9511740277818842e-6 m^3. The rejection mechanism is observed firing (G6 branch 4) | PASS | path `tests/prd-gate/fixtures/occt_internals_fillet_translated_disjoint_union.ri` |
| ζ | extruded-wire-passes-guard-reproduces | fixture 3: a = 0.0002 m^2, v = 0 m^3, vf undef with OCCT's "no suitable edges"; the post-fix kind is not asserted | PASS | path `tests/prd-gate/fixtures/occt_internals_fillet_extruded_wire.ri` |
| ζ | repr-policy-type | premise: `brep_kind_of_shape` and `store_classified` exist | PASS | grep `enum ReprPolicy` present in `src` |
| ζ | implicit-solid-store-deleted | the implicit-Solid `store` helper must go. Gap: another hard-coded helper evades it; BT-ζ1 is the cover | PASS | grep `fn store\(&mut self, shape` absent in `src` |
| ζ | sweep-pose-guard-upstream | producer: task 7248 upstream (pending) | PASS | manual (add_dependency) |
| ζ | docs-truth-chunk-lines | geometry.md and measurement.md say nothing about kind preservation; no fence added | PASS | manual (prose) |
| ε | get-shape-erasure-fix-upstream | producer: task 7379 upstream (pending, carries fk-baae0532b743) | PASS | manual (add_dependency) |
| ε | shape-store-type | h14 row: execute/query/fixtures pass h13 only over a pub(crate) ShapeStore | PASS | grep `pub\(crate\) struct ShapeStore` present in `src/shape_store.rs` |
| ε | warm-state-disposition | **Q2** lead ruling: (a) no-op or (b) ShapeStore persist/restore | OPEN | manual |
| ι | partition-names-fixed-for-downstream | PRD §3 names the files; S10 converts `query()` inside `query.rs` | PASS | path `src/execute.rs`, `src/query.rs` present |
| θ | one-call-primitive | `send_request_blocking` / `send_request_async` precedent; the kernel lives on the one spawn thread | PASS | grep `fn call<` present in `src/handle.rs` |
| θ | meshing-service-alpha-upstream | producer: task 8284 upstream (in-progress) | PASS | manual (add_dependency) |
| θ | stub-doc-states-real-contract | stub doc claims "same public API surface"; 24/50 and 16/33 measured | PASS | manual (prose) |
| δ | cxx-vec-of-shared-struct-precedent | `StepTreeRecords { products: Vec<StepProductRecord>, .. }` already crosses this bridge | PASS | grep `products: Vec<StepProductRecord>` present |
| δ | flat-accessors-retired | 37 declarations today; the ratchet row (≤ 8) is the count cover | PASS | grep `fn sweep_op_history_face_modified` absent in `src/ffi.rs` |
| δ | typed-record-struct | D8 names `FfiHistoryRecord` | PASS | grep `struct FfiHistoryRecord` present in `src/ffi.rs` |
| δ | golden-parity-boundary-test | contract C1 BT-δ1/δ2; serde_json is a dev-dep | PASS | manual |
| ω | terminal-stamp | overlay decompose-close obligation | PASS | manual |

**Not bound here:** fk-030602c2fd62 (Q1, default re-home to S10) and fk-baae0532b743 (filed #7379, referenced as ε's upstream producer, not re-filed).
