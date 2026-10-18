# PRD — OCCT crate internals: one op lowering, ShapeStore, ReprPolicy, one channel request, typed history buffers

**Status:** draft for lead ruling (pass 1 of 2), program `review-all-reify-20261010`, stream **S11** (wave 1, agent mode, size M, risk medium). Program contract: `plans/review-all-program-reify-20261012.md` (§Streams row S11, §Resolved decisions "Uncontested block"). Approach **B + H-lite** (§5): contracts plus two-way boundary tests on the two seams that matter (the persistent-naming history transport and the result-repr policy); bare B elsewhere. gap-fill 2026-10-16: κ #9300, λ1 #9304, λ2 #9306, μ #9305.
**Invariants:** none of the eight promoted invariants is owned by S11. Every leaf that extracts a shared primitive ships its adoption-ratchet row (INV candidate 5, repo-wide; §4 D9). Leaf ε touches INV-GEO-3's OCCT arm under Q2.
**Code anchors** verified against main `8e6ffcac9d` (2026-10-12). `git diff a030ee7528..8e6ffcac9d -- crates/reify-kernel-occt crates/reify-ir/src crates/reify-eval/src/engine_build.rs crates/reify-eval/src/topology_selectors.rs crates/reify-test-support` is empty, so every finding anchor in the record still holds. Main moves fast: cite by symbol, re-locate lines at implementation time.

---

## 1. Consumer and user-observable surface (G1, G2)

**Consumers.**
- **Design authors**, through the kernel's result-repr policy (leaf ζ). Three behaviours they see today are wrong, each measured with `target/debug/reify eval` on main `8e6ffcac9d`:
  - `centroid(translate(rectangle(20mm, 20mm), 10mm, 0mm, 0mm))` prints `point(0 m, 0 m, 0 m)`. The translated profile is stamped `Solid`, so `Centroid` takes the volume path. Fixture: `tests/prd-gate/fixtures/occt_internals_centroid_translated_profile.ri`.
  - `fillet(union(a, b), 1mm)` over two disjoint boxes is rejected ("requires a BRepKind::Solid input shape, got Compound"). `fillet(translate(union(a, b), ..), 1mm)` is accepted and yields 1.95e-6 m^3, because `translate` re-stamps the Compound as `Solid`. Fixture: `tests/prd-gate/fixtures/occt_internals_fillet_translated_disjoint_union.ri`.
  - `fillet(extrude(line_segment(..), 10mm), 1mm)` passes the Solid guard (the extrude result is stamped `Solid`, though `volume` reads 0 m^3) and fails deep inside OCCT ("There are no suitable edges for chamfer or fillet"). Fixture: `tests/prd-gate/fixtures/occt_internals_fillet_extruded_wire.ri`.
- **The next agent to change the OCCT kernel.** Every finding in this stream is a next-change cost: adding a history-aware op, exposing a kernel method through `&dyn GeometryKernel`, adding a history record kind, adding a field to `OcctKernel`, or adding a kernel test. §2 lists the cost per finding.
- **S10 (wave 2, kernel interface)**, which the program doc lists as consuming "S11 (ShapeStore, op lowering)". S10 builds its typed SI quantity decode on S11's single per-op argument decode (γ1). It writes capability sub-trait accessors over S11's `call<R>` channel primitive (θ). It converts `query()` replies inside S11's `query.rs` (ι).

**In-engine seam.** No new seam. Every mechanism is internal to the §3.1 op-execute seam (`docs/prds/v0_3/engine-integration-norm.md`): `GeometryKernel::execute` / `execute_with_history` / `query` as implemented by `OcctKernelHandle` over `OcctKernel`. The production caller is `Engine::execute_realization_ops` in `engine_build.rs`, which calls `execute_with_history`. That is why the history lowering (γ1) is on the production path.

## 2. Findings this stream owns (record `plans/review-all-reify-20261010.json`)

| key | anchor | sev | verdict | short id | one-line cost | leaf |
|---|---|---|---|---|---|---|
| fk-e6bdef1ef0ae | `handle.rs::OcctKernelHandle` | medium | confirmed | f70 | The production history path lowers ops inside the actor's `spawn()` loop, reads Extrude/Fillet/Chamfer lengths with `as_f64` (skips the units-length tripwire, non-canonical errors). The doc names a dispatcher that does not exist. | γ1 |
| fk-d9cc4b340a69 | `lib.rs::OcctKernel` | medium | confirmed | f59 | (a) 3 verbatim sweep-history decodes plus a loft variant, (b) 2 revolve-validation copies, (c) 5 selection-to-index copies, (d) 3 near-identical `extract_*`, (e) about 13-17 open-coded finite-positive checks. | γ2 (with γ1) |
| fk-9eb90a5a82ff | `handle.rs::OcctRequest` | medium | confirmed | f71 | 30 request variants, 43 inherent methods, 16 trait forwards: 4-7 hand-written sites per channel-routed capability. | θ |
| fk-10c48f52f01c | `lib.rs::tests` | medium | confirmed | f60 | A 10,013-line in-file test module (244-245 tests, at least 83 reach privates) beside a public test-fixtures API: two fixture APIs over one store. | β (with α) |
| fk-68d69d7d9ace | `ffi.rs::ffi` | low | confirmed | f92 | 37 flat `Vec<u32>` history accessors described by 21 "flat groups of" comments, 41 call sites. | δ |
| fk-d22418138fa2 | `stubs.rs::OcctKernel` | low | weakened | f64 | The stub doc claims "the same public API surface" (24/50 and 16/33 methods). Skeptic: no production caller of the missing methods, so drift fails loudly in an OCCT-free build. | θ |
| fk-ba1ea4167dbe | `lib.rs::OcctKernel.execute` | **high** | weakened | f65 | Result `BRepKind` is chosen by exit path. About 29 arms fall through to a tail that stamps `Solid`. The skeptic's counts differ (29/6, not 27/8), but the structure and the three behaviours in §1 reproduce. | ζ |
| fk-030602c2fd62 | `lib.rs::OcctKernel.query` | medium | weakened | f66 | Point, vector and bbox replies are JSON strings parsed by 10 hand-rolled eval-side parsers. **Reassigned to S10** (Q1 ruled (a): S10's typed-reply leaf switches producer and parsers atomically). | S10 |
| fk-baae0532b743 | `lib.rs::OcctKernel.get_shape` | medium | confirmed | f67 | 51 query plus 5 export/tess sites erase `get_shape`'s cause into `InvalidHandle`. **Already filed as #7379.** | #7379 (external prerequisite of ε) |
| fk-69bd53399b08 | `lib.rs::OcctKernel.warm_state` | low | confirmed | f68 | About 250 lines of warm-start machinery with 0 production callers, and it shapes every field decision. | ε |

Weakened findings, re-read under the freshness clause:
- **f65.** The arm-count discrepancy is immaterial. All seven transform arms fall through, and the three §1 probes show the user-visible consequence. The leaf is built on the probes, not on the counts.
- **f64.** The skeptic is right that the cost is a loud compile error, not a silent failure, so θ only corrects the doc (§4 D7).
- **f66.** Reassigned to S10 by the Q1 ruling. S11 ι only moves `query()` into `query.rs`.

**h14 measurement (record `h14_measurements`, no key).** `lib.rs` has `stays_whole:false` with 8 partitions. `op_args.rs` and `history_decode.rs` have no h13 symptom and split now (γ1, γ2). The execute, query, fixtures and history partitions are clean only over a `ShapeStore` type (ε then ι). `handle.rs` has 4 partitions: its transport is clean once the op dispatch leaves `spawn()` (γ1), and its history/fixture method groups are clean only over one `call<R>` primitive (θ). Its 1,700-line test module reaches only the public API and moves out (α).

## 3. Sketch of approach — the target structure

```
crates/reify-kernel-occt/src/
  lib.rs            crate root: mod wiring + re-exports, OCCT_AVAILABLE, boolean pass counter,
                    `pub struct OcctKernel { store: ShapeStore, .. }` and its thin pub facade
  shape_store.rs    pub(crate) ShapeStore: shapes, reprs, next_id, sub-shape caches, parent map;
                    insert(shape, ReprPolicy), get(id), repr_of, sub_shapes(parent, kind),
                    indices_of(parent, selection, kind), parent_of, clear  (ε)
  op_args.rs        extract_f64, extract_length_f64, validate_positive_finite, validate_uv_finite,
                    validate_pipe_start_tangent, validate_revolve_params, validate_extrude_distance,
                    AXIS_MAG_SQ_MIN, ANGLE_ABS_MIN  (γ1)
  history_decode.rs one decoder per history type (boolean, local-feature, sweep, loft)  (γ2, reshaped by δ)
  execute.rs        free fns over &mut ShapeStore: one per-op builder returning (shape, ReprPolicy),
                    shared by execute and execute_with_history  (ζ policy, ι file)
  history.rs        the *_with_history primitives over &mut ShapeStore  (ι)
  query.rs          query() and its helpers over &ShapeStore  (ι)
  fixtures.rs       cfg(feature = "test-fixtures") constructors over the ShapeStore API  (ι)
  handle.rs         transport only: Job = Box<dyn FnOnce(&mut OcctKernel) + Send>, call<R>, call_async<R>,
                    one-line inherent methods, GeometryKernel/WarmStartable impls  (θ)
  stubs.rs          OCCT-free mirror, doc states its real contract  (θ)
  ffi.rs + cpp/     one shared records struct per op-history type  (δ)
tests/
  harness_occt*/    every test that needs only the public surface (α, β), plus
                    inline_pattern_ratchet.rs (γ1 creates; γ2, ε, θ, δ add rows)
```

Data flow after the stream: `OcctKernelHandle::execute_with_history(op)` runs `call(move |k| k.execute_with_history(&op))`. `OcctKernel::execute_with_history` picks the history-aware primitive per op. The primitive decodes its arguments once through `op_args.rs`, builds the shape plus records, and stores it once through `ShapeStore::insert(shape, policy)`. The plain `execute` path calls the **same** per-op builder and drops the records.

## 4. Resolved design decisions

These follow the program ruling for S11 ("ShapeStore; execute_with_history owns the op lowering; the split op lowering folded as one medium entry") and the record's proposals. Where the record offered two options, the choice and the reason are stated.

- **D1. One op-to-history lowering, owned by `OcctKernel` (f70, f59b).** `OcctKernel::execute_with_history(&mut self, op: &GeometryOp) -> Result<(GeometryHandle, AttributeHistory), GeometryError>` is a new inherent method. It routes Extrude, Revolve, Sweep, Loft, Union, Difference, Intersection, Fillet and Chamfer to their history primitives, and every other op to `execute` with `AttributeHistory::None`. Length fields are read **only** through `extract_length_f64(v, op, field)`, so the history path fires the tripwire and emits `reify_ir::non_numeric_kernel_field_message`. The actor's `ExecuteWithHistory` arm becomes a one-line forward (θ later removes it entirely). The `OcctKernelHandle::execute_with_history` doc becomes true. The handle's typed f64 convenience wrappers (`extrude_with_history(profile, f64)` and siblings, used only by in-crate tests) call the typed inherent primitive directly. They must not wrap a bare `Value::Real` into a `GeometryOp`, because that would fire the length tripwire on a test-only path.
- **D2. One per-op argument decode, shared by both entry points (f59b, f59e).** Each op family has one validator in `op_args.rs`: `validate_revolve_params`, `validate_extrude_distance`, the loft section check, and finite-positive through `validate_positive_finite`. The `execute` arm and the `*_with_history` primitive both call it. There is no second copy of a message literal. S10 later replaces `extract_length_f64` with typed SI newtypes at this one home. That is a consumer of D2, not a conflict with it.
- **D3. One decoder per history type, one sub-shape extractor, one selection mapper (f59a, c, d).** `history_decode.rs` gains `decode_sweep_history` (used by extrude, revolve and sweep) and `decode_loft_history`, beside the existing `decode_six_buffer_history`. One `extract_subshapes(parent, SubShapeKind)` serves `extract_edges`, `extract_faces` and `extract_vertices`; they stay as public one-liners. One `map_selection_to_indices(parent, selection, kind, label)` with **one** algorithm serves fillet_edges, chamfer_edges, chamfer_asymmetric_edges, draft_faces and shell_solid_faces. Linear-position and HashMap lookups agree on the result, so the implementer picks one and pins it with the existing curated-selection tests. INV-GEO-2 (stable `extract_*` per parent) is preserved; `handle_stability_conformance.rs` is the cover.
- **D4. ReprPolicy: no default tail (f65).** `enum ReprPolicy { Fixed(BRepKind), Inherit(GeometryHandleId), Classify }`. Every `execute` arm evaluates to an `ArmOutcome`, either `New { shape, policy }` or `Stored(GeometryHandle)` (the delegating arms). The store runs once after the match. `OcctKernel::store` (the implicit-Solid helper) is deleted, so a new arm cannot compile without choosing a policy. Policy table:
  - **Inherit(target):** Translate, Rotate, Mirror, Scale, RotateAround, AffineApply, ScaleNonUniform, and ApplyTransform (already inherits today).
  - **Classify** (via `brep_kind_of_shape`): the binary booleans (already classify), LinearPattern, CircularPattern, LinearPattern2D, ArbitraryPattern, Extrude, ExtrudeSymmetric, ExtrudeInfinite, Revolve, Sweep, Pipe, Loft, SweepGuided and LoftGuided (already classify), and every remaining constructive arm whose result type depends on its input (Thicken, OffsetSolid, ZoneSlab, Draft, Shell, Fillet and Chamfer results).
  - **Fixed:** the solid primitives (`Fixed(Solid)`: Box, Cylinder, Sphere, Tube, Cone, Wedge, Torus, HalfSpace), and the profile and curve constructors that return `Wire` or `Face` early today.

  The `*_with_history` primitives store through the **same** per-op policy. This is "both entry points of each op", the shape task #8582 asks for.
- **D5. One Solid-input precondition for fillet and chamfer (f65).** The all-edges branches of the `execute` Fillet and Chamfer arms stop calling `ffi::fillet_all_edges` / `ffi::chamfer_all_edges` directly. They route through the same guard as `run_local_feature_with_history`, so the same op has one precondition on every path. The guard's comment about `fillet(translate(union(a, b)))` becomes true; a test pins it (fixture 2).
- **D6. ShapeStore (f65, f66, f67, f68 per the program remedy).** `pub(crate) struct ShapeStore` owns `shapes`, `reprs`, `next_id`, `extracted_{edges,faces,vertices}` and `parent_handle`. Its interface is `pub(crate)` and narrow: insert, get (the IsNull chokepoint), repr_of, sub_shapes, indices_of, parent_of, clear, len. `OcctKernel` holds `store: ShapeStore` plus only non-store state. `OcctKernel::reset` becomes `store.clear()` behind one exhaustive destructure, which lives in `shape_store.rs`. The query-side error mapping from #7379 (`shape_for_query` and its export/tess siblings) becomes `ShapeStore` methods. Warm start is **reduced to the trait no-op** (Q2 ruled (a)): `warm_state` returns `None` and `with_warm_state` is a no-op. The `inventory` registration stays. The persistence logic, the `WarmState`/`WithWarmState` forwarding, `warm_start_failures` with its accessor test (#5112), and the two warm-state exhaustive destructures go. INV-GEO-3's OCCT arm is reworded as stateless.
- **D7. One channel request (f71) and the stub contract named (f64).** The channel carries `Box<dyn FnOnce(&mut OcctKernel) + Send>`. `OcctKernelHandle::call<R: Send + 'static>(&self, f) -> Result<R, ChannelDied>` and its async twin are the only send/recv sites. Every inherent method, including history wrappers, test-fixture wrappers and reset, becomes a one-line `call`, and the `spawn()` loop is one arm. Each method maps `ChannelDied` to its own error type (`GeometryError::OperationFailed("kernel thread died")` and siblings, as today). The `stubs.rs` module doc is corrected to the real contract. The stub mirrors what code compiled without OCCT reaches: `OCCT_AVAILABLE`, `OcctKernelHandle::spawn`, and the `GeometryKernel` / `WarmStartable` impls. Its other inherent methods are conveniences with no parity claim. No stub code changes: no gate compiles the OCCT-free configuration (INV-GEO-4 note), so a code change there could not be verified.
- **D8. Typed history buffers across the bridge (f92).** Each op-history opaque type (`BooleanOpHistory`, `LocalFeatureOpHistory`, `LoftOpHistory`, `SweepOpHistory`) exposes **one** records accessor returning a cxx shared struct. Record lists are `Vec<FfiHistoryRecord { parent_index, parent_subshape_index, result_subshape_index }>` and `Vec<FfiDeletedRecord { .. }>`, not flat `Vec<u32>` groups, plus the per-type counters and cap lists. The result shape keeps its own take function. Target: at most 8 history FFI declarations (4 records plus 4 take). The C++ side builds the struct once per op. Precedent: `StepTreeRecords` already carries `Vec<StepProductRecord>` across this bridge (G3 PASS).
- **D9. Adoption ratchet (INV candidate 5).** γ1 creates `crates/reify-kernel-occt/tests/harness_occt/inline_pattern_ratchet.rs`, modelled on `crates/reify-eval/tests/version_id_discipline_gate.rs`. It is a comment- and string-aware, line-oriented scan of the crate's production source. It keeps one committed baseline row per superseded inline pattern and asserts that the live count is at most the baseline (one-directional). Seeded-violation self-tests prove each row both flags and clears. Each extracting leaf names its row (§8). This is a construct lint over code, never over comments (`tests/infra/README.md` → "Assert behaviour, not source prose").
- **D10. Tests drive public seams (f60).** A kernel test that needs only `OcctKernel` / `OcctKernelHandle` public methods plus the `test-fixtures` constructors lives under `tests/harness_occt*`. The in-file module keeps only tests that must see a private invariant (IsNull injection; the exhaustive-destructure / state-inventory tests), and those use the same fixture constructors rather than `store_raw(ffi::ffi::make_x())`. Where a private-reach test can be re-expressed on a public fixture constructor, it is re-expressed and moved. New harness modules follow the harness-layout contract (`tests/infra/test_harness_kloc_cap.sh`: `#[path]` declarations, the 20,000-line cap per harness unit, a split under rule (a) rather than a cap bump), so the ~6-7k moved lines likely need a new `harness_occt_<subsystem>.rs` unit.

## 5. Contracts and boundary tests (G5: persistent-naming is a load-bearing seam)

**C1, history transport (δ).** For every op family, the `AttributeHistory` records decoded by the kernel (`BooleanOpHistoryRecords`, `LocalFeatureOpHistoryRecords`, `SweepOpHistoryRecords`, `LoftOpHistoryRecords`, the `reify_ir` types, unchanged) are identical before and after the FFI reshaping.

| boundary test | precondition | postcondition |
|---|---|---|
| BT-δ1 golden capture | δ's first commit, before any FFI edit: a fixture corpus covering fuse, cut, common, fillet (all and curated), chamfer (all, curated, asymmetric), extrude, revolve (partial and full), sweep, and loft (closed and open sections) | the decoded records serialised (serde_json dev-dep) to a committed golden file |
| BT-δ2 parity | after the FFI change | the same corpus decodes to byte-identical golden JSON |
| BT-δ3 consumer side | `reify-eval` persistent-naming consumers (`topology_attribute_propagation` tests, the existing `*_op_history_integration.rs` modules) | green and unchanged |

**C2, result repr (ζ).** `repr_of(result)` equals the D4 policy for every `execute` arm and for both entry points of each history-aware op.

| boundary test | precondition | postcondition |
|---|---|---|
| BT-ζ1 policy table (kernel side) | one fixture per input kind (Face, Wire, Solid, disjoint Compound) through each transform; face and wire profiles through each sweep; overlapping and disjoint pattern instances | `repr_of` matches D4, asserted through the public `OcctKernel::execute` and `execute_with_history` |
| BT-ζ2 consumer side, Centroid | fixture 1 | `c_moved` = point(0.01 m, 0 m, 0 m) within 1e-9 m |
| BT-ζ3 consumer side, local-feature guard | fixtures 2 and 3 | both `moved` (fixture 2) and `f` (fixture 3) are rejected with "local-feature operation requires a BRepKind::Solid input shape"; `v_direct` is unchanged |
| BT-ζ4 no regression | the examples corpus (`examples_smoke` gate) | green. A disjoint `pattern(...)` that a corpus file fillets would now reject; ζ lists any such hit and dispositions it |

## 6. Pre-conditions and coordination with existing tasks

The task numbers below come from `search_tasks` / `get_task`, 2026-10-12. Decomposition re-runs contract §8 dedup before filing.

- **#7379** (pending, medium) "stop get_shape callers erasing the real GeometryError" carries `fk-baae0532b743`. It is **not re-filed**. It is an external prerequisite of ε: ε moves its helpers into `ShapeStore`, and ε's ratchet row holds its `|_| ..InvalidHandle` count at 0.
- **#8582** (pending, low) "Extrude/ExtrudeSymmetric/ExtrudeInfinite/Revolve results are stamped Solid even for a wire profile" is **subsumed by ζ** (D4 Classify on both entry points). Decomposition either cancels it naming ζ as the subsuming leaf, or wires it so ζ's landing closes it. A subsumption is not an acceptance, so the task gets no `x_acceptance_reason`.
- **#7248** (pending, routed) is the sweep pose guard, plus plain Sweep classifying via `brep_kind_of_shape` on both entry points. ζ **depends on #7248**, so the sweep arms are policy-converted after its guard lands rather than conflicting with it.
- **#8284** (in-progress, meshing-service α) edits `handle.rs`, `ffi.rs`, `lib.rs` and `cpp/`. θ and δ **depend on #8284**. Every other leaf rebases normally.
- **#7438** (pending) threads an `OpControl` into `OcctRequest::{Execute, ExecuteWithHistory}`. Its step 3 is written against the request enum θ removes. **Q3 ruled (a):** γ1 and θ land first. #7438 carries a note that its control is passed to `OcctKernel::execute_with_history` inside the `call` closure, plus the edge #7438 → θ.
- **#7688** (pending, low) adds a volume-provenance method to `OcctKernelHandle`. After θ it is one `call`. Decomposition adds an edge #7688 → θ, or annotates it.
- **#8561** (pending, low), which removes the mesh-morph edge and `projector_impl.rs`, is independent.

## 7. Cross-PRD relationship and seam owners (G4)

| shared artefact | owner (program §Seam ownership) | S11's role |
|---|---|---|
| OcctKernelHandle public surface vs stubs.rs mirror | **S11** | θ edits (D7) |
| occt_wrapper.cpp accessor set | **S11** | δ edits (D8) |
| GeometryKernel trait (27 methods) | S10 | consumed unchanged. No S11 leaf changes a trait signature or default |
| reify-test-support MockGeometryKernel | S10 | **not edited.** ζ changes OCCT's repr policy only; mirroring it in the mock is a cross-reference to S10 |
| reify_ir::kernel_validation (`check_length_field`, message formatters) | no G4 row; S10's typed-SI ruling retires the tripwire later | consumed unchanged by γ1 |
| `reify_ir::GeometryQuery` reply shape and the eval-side JSON parsers | S10 ("GeometryQuery returns typed replies") | **not edited**; fk-030602c2fd62 belongs to S10 (Q1) |
| `docs/invariants.md` INV-GEO-3 row | row format S24; content by the stream that changes the mechanism | ε rewords the OCCT arm as stateless (Q2) |
| `crates/reify-mcp/src/tools/chunks/geometry.md` (docs-truth) | no G4 row | ζ adds the transform-preserves-kind and one-solid fillet/chamfer lines (§8 ζ) |

S11 has no upstream seam (wave 1). Downstream, S10 consumes γ1 (one decode home), θ (`call<R>`) and ι (`query.rs`). §11 gives the cross-reference lines.

## 8. Decomposition plan

Every lib.rs-touching leaf serialises on that file. The order puts the high-severity behaviour fix (ζ) right after its one prerequisite (γ1). The tests move (α, β) runs before the store refactor (ε), so the refactor never has to rewrite private-reach tests.

```
γ1 ──► ζ ──────────────┐
 │                     ▼
 ├──► γ2 ──────────► ε ──► ι ──► ω
 │     │             ▲
 │     └──► δ        │ (#7379)
α ──► β ─────────────┘
α, γ1 ──► θ          (#7248 ──► ζ ; #8284 ──► θ, δ)
```

Every implementation leaf below also depends on nothing outside this list except the external edges shown.

Priorities (severity of the stamped key; enablers take medium): ζ high. α, β, γ1, γ2, ε, ι, θ medium. δ and ω low.

### α (#8648) — Move public-surface tests out of lib.rs and handle.rs
- **Keys (x_finding_key):** none. This is the prerequisite half of β's key.
- **Files:** `crates/reify-kernel-occt/src/lib.rs` (`mod tests`), `src/handle.rs` (`mod tests`), new modules under `crates/reify-kernel-occt/tests/harness_occt*`.
- **Work:** move every in-file test whose body reaches only public API (`OcctKernel` / `OcctKernelHandle` pub methods, the `test-fixtures` constructors, `reify_ir`) into harness modules: about 160 of lib.rs's tests (rule: no `ffi::ffi::`, `store_raw`, `insert_null_shape`, `.get_shape(`, private fields or private helpers in the body or its local helpers), and handle.rs's whole test module, which reaches only `super::OcctKernelHandle`. Module paths and names are preserved. Satisfy the harness-layout contract in the same diff (D10).
- **Signal:** `cargo nextest list -p reify-kernel-occt` reports the same total test count before and after, captured once each per `scripts/cargo-test-tally.py`. `handle.rs` has no `mod tests`. `tests/infra/test_harness_kloc_cap.sh` and `scripts/check-harness-baseline-registration.sh --from-git` are green.

### β (#8659) — One fixture API: in-file tests rebuilt on the test-fixtures constructors
- **Keys (x_finding_key):** fk-10c48f52f01c.
- **Deps:** α.
- **Work:** re-express every remaining private-reach test on the `*_for_test` constructors (adding a constructor where a needed shape has none, for example a null-topology fixture if IsNull injection can be made public without weakening the guard), then move it to the harness. The in-file module keeps only tests that must observe a private invariant, each listed with its reason in the commit message. The cfg(test) private `store_raw` duplicate API is deleted. `insert_null_shape` survives only if the IsNull test stays in-file.
- **Signal:** total test count unchanged (same tally rule). The in-file `mod tests` holds exactly the kept-list tests named in the commit message. `git grep -n 'store_raw' crates/reify-kernel-occt` is empty. f60's measurement re-run (tests reaching privates) is 0 outside the kept list.

### γ1 (#8652) — `OcctKernel::execute_with_history` owns the op lowering; one argument decode (op_args.rs)
- **Keys (x_finding_key):** fk-e6bdef1ef0ae. It also delivers the D2 half of γ2's key.
- **Files:** `src/lib.rs`, new `src/op_args.rs`, `src/handle.rs`, new ratchet module.
- **Work:** D1 and D2. Split `op_args.rs` (h14 partition, no symptom: it imports only `reify_ir` and `crate::ffi::ffi::Point3`). Create the D9 ratchet with rows (i) open-coded finite-positive checks outside `validate_positive_finite` = 0; (ii) `.as_f64()` in `handle.rs` non-test code = 0; (iii) occurrences of each op-family validation message literal = 1.
- **Signal:** a harness test drives `OcctKernel::execute_with_history` **on the test thread** under `reify_ir::arm_length_tripwire_assert()` with a non-LENGTH `Extrude.distance`, `Fillet.radius` and `Chamfer.distance`. Each panics with the canonical `non_length_kernel_field_message` (today the history path never consults the tripwire). The panic arm is `#[cfg(debug_assertions)]`, because the assertion compiles out in release and the gate runs both profiles. The error-text equality below holds in both profiles. With a non-numeric value, the history-path error text equals `execute`'s (`non_numeric_kernel_field_message`). `git grep -n 'fn execute_with_history' crates/reify-kernel-occt/src/lib.rs` has one hit. The ratchet module is green and its self-tests prove each row flags and clears.

### γ2 (#8666) — One decoder per history type, one sub-shape extractor, one selection mapper (history_decode.rs)
- **Keys (x_finding_key):** fk-d9cc4b340a69.
- **Deps:** γ1.
- **Work:** D3. Split `history_decode.rs` (h14 partition, no symptom). Ratchet rows: (iv) `sweep_op_history_face_modified(` call sites = 1; (v) `does not belong to` membership loops = 1 (inside `map_selection_to_indices`); (vi) the three topology getter FFI calls appear only inside `extract_subshapes`.
- **Signal:** all five f59 measurements re-run at target: sweep decode copies 1, revolve validation copies 1 (from γ1), selection mapping copies 1, `extract_*` bodies 1, open-coded finite-positive 0. The existing sweep, revolve, loft and curated-selection history tests are green and unchanged.

### ζ (#8669) — Every execute arm declares its result repr (ReprPolicy); one fillet/chamfer precondition
- **Keys (x_finding_key):** fk-ba1ea4167dbe. **Subsumes task #8582.**
- **Deps:** γ1, #7248.
- **Files:** `src/lib.rs`, kernel harness tests, `crates/reify-mcp/src/tools/chunks/geometry.md`.
- **Work:** D4 and D5, plus contract C2 (§5). Correct the `run_local_feature_with_history` reach comment so it matches the code. Docs-truth (overlay): add one line each to the geometry chunk's transform and modify sections: "a transform keeps its input's kind (a translated profile is still a face, so `centroid`/`area` measure the face)" and "`fillet`/`chamfer` need one solid, also after a transform: a disjoint `union` or a swept wire is rejected". No reify fence is added, so `harness_doc_chunks/fence_gate.rs` counts are unchanged. No exemplar-corpus or cheatsheet change: no new idiom (overlay rule 2 is conditional). The discoverability line names the behaviour in intent terms. If a Rust test reads a `tests/prd-gate/fixtures/occt_internals_*.ri` fixture, register it in `_RUST_COUPLED_RI_FIXTURES` in the same diff (PG-DRIFT).
- **Signal (user-observable, `reify eval`):** fixture 1 prints `Probe.c_moved = point(0.01 m, 0 m, 0 m)` (x within 1e-9 m). Fixture 2 leaves `v_moved` undef with the "requires a BRepKind::Solid input shape, got Compound" error, the same as `v_direct`. Fixture 3 reports "local-feature operation requires a BRepKind::Solid input shape" instead of OCCT's "no suitable edges" message. `git grep -n 'fn store(' crates/reify-kernel-occt/src` is empty. BT-ζ1..ζ4 are green.

### ε (#8682) — Extract ShapeStore with a pub(crate) interface
- **Keys (x_finding_key):** fk-69bd53399b08.
- **Deps:** β, γ2, ζ, #7379.
- **Files:** `src/lib.rs`, new `src/shape_store.rs`, `src/handle.rs` (warm-state forwards), `docs/invariants.md` (INV-GEO-3 description cell).
- **Work:** D6. Move the IsNull chokepoint, `ReprPolicy` resolution, sub-shape caches, parent map and #7379's error-mapping helpers into `ShapeStore`. Warm start is reduced to the trait no-op (D6). Ratchet row (vii): `.map_err(|_| QueryError::InvalidHandle` / `ExportError::InvalidHandle` / `TessError::InvalidHandle` = 0.
- **Signal:** `OcctKernel`'s field list is `store` plus non-store state only. `git grep -n 'let Self {' crates/reify-kernel-occt/src/lib.rs` is empty: the exhaustive destructures live in `shape_store.rs`. `reify eval examples/spec-shape-physical.ri` output is byte-identical before and after. `impl WarmStartable for OcctKernel` has a `None` / no-op body. `git grep -n 'fn warm_start_failures' crates/reify-kernel-occt` is empty. The INV-GEO-3 row names the OCCT arm as stateless. The `WarmStartableRegistry` coextension assertion stays green.

### ι (#8684) — Partition lib.rs over ShapeStore: execute.rs, history.rs, query.rs, fixtures.rs (h14)
- **Keys (x_finding_key):** none. This is the record's h14 measurement row for `lib.rs`. fk-030602c2fd62 belongs to S10 (Q1); its typed-reply leaf converts `query()` inside `query.rs`.
- **Deps:** ε.
- **Work:** move execute's per-op builders, the `*_with_history` primitives, `query` with its helpers (`face_outward_unit_normal`, `analytic_*_datum_to_value`, `volume_measurement` and the inherent geometric queries such as `shapes_intersect`, `min_clearance` and `closest_point_on_shape`), and the test-fixtures constructors into free functions or modules over `&ShapeStore` / `&mut ShapeStore`. `OcctKernel`'s pub methods stay as one-line facades, so `tests/` and out-of-crate tests are unaffected. Each new file must pass h13: no `impl OcctKernel` outside `lib.rs`, no `use super::*` reach-back, and only downward imports of `shape_store`, `op_args`, `history_decode` and `ffi`. The commit message records the measured partition.
- **Signal:** `git grep -n '^impl OcctKernel\|^impl.* for OcctKernel' crates/reify-kernel-occt/src` lists only `lib.rs`. No new file imports anything from `crate::` that is defined in `lib.rs` except the `OcctKernel` type itself. The commit message carries the h14 re-measurement of `lib.rs` (prose home, named partition and per-partition h13 symptom), not a line target. Total test count unchanged.

### θ (#8672) — One closure request behind `call<R>`; handle.rs is transport; the stub contract named
- **Keys (x_finding_key):** fk-9eb90a5a82ff, fk-d22418138fa2.
- **Deps:** γ1, α, #8284.
- **Files:** `src/handle.rs`, `src/stubs.rs` (doc only), ratchet module.
- **Work:** D7. Ratchet row (viii): `reply.send(` / `.send(` channel sites in `handle.rs` at most 2 (`call` and `call_async`). Re-measure `handle.rs` after the collapse with the h14 protocol: prose home first, then the named partition. If the measurement says it stays whole, record that in the commit message (h14 stays-whole rule). Otherwise split along the record's 4 partitions, each passing h13 over `call<R>`.
- **Signal:** `OcctRequest` is gone (or has exactly one variant). The `spawn()` loop has one arm. Every `impl GeometryKernel for OcctKernelHandle` method is one expression. The moved handle tests from α are green unchanged. The `stubs.rs` module doc no longer claims "the same public API surface". Exposing a new inherent `OcctKernel` method through the handle is one one-line method plus one trait forward (shown by #7688 or #7438 if either lands after).

### δ (#8676) — One cxx shared struct per op-history type replaces the flat accessors
- **Keys (x_finding_key):** fk-68d69d7d9ace.
- **Deps:** γ2, #8284.
- **Files:** `src/ffi.rs`, `cpp/occt_wrapper.cpp`, `cpp/occt_wrapper.h`, `src/history_decode.rs`, harness golden test.
- **Work:** D8 under contract C1 (BT-δ1 first, then the change, then BT-δ2). Ratchet row (ix): `fn [a-z_]+_op_history_[a-z_]+` declarations in `ffi.rs` at most 8.
- **Signal:** BT-δ2 golden parity green. `git grep -cE 'fn [a-z_]+_op_history_[a-z_]+' crates/reify-kernel-occt/src/ffi.rs` is at most 8 (37 today). `history_decode.rs` has one decoder per type with no `chunks_exact`.

### ω (#8688) — PRD close
- **Deps:** every leaf above.
- **Work:** the overlay's decompose-close stamp. Status `SHIPPED` with the landed leaf ids, the AS-AUTHORED freeze paragraph and the LIVE / AS-AUTHORED map, and the same header on the capability manifest.
- **Signal:** the committed header.

## 9. Out of scope

- Typed `GeometryQuery` replies, the eval-side JSON parsers, and the surface/curve-kind strings: S10, which owns fk-030602c2fd62 (Q1).
- Typed SI quantity fields in `GeometryOp` and retiring the warn-only tripwire: S10. γ1 only gives S10 one decode home to replace.
- `GeometryKernel` trait reshaping (capability sub-traits) and `MockGeometryKernel`: S10.
- Wiring a production consumer for geometry-kernel warm start (Q2 ruled (a): reduce): needs an engine-side owner, not this crate.
- Any OCCT-free stub **code** change (D7): no gate compiles that configuration.
- Prose re-homing and file:line citations inside the crate's comments: S23 (wave 4), after this stream has moved the files. S11 deletes reviewer-answering prose only where its own leaves delete the code it describes (for example the 21 "flat groups of" comments in δ, or warm-start justifications under Q2(a)).

## 10. Open questions

**Design-level questions: all ruled by the lead on 2026-10-12** (program doc §"Resolved decisions — pass 1 open questions", S11).

1. **Q1 → (a).** fk-030602c2fd62 moves to S10's typed-reply leaf, so the producer and the eval parsers switch atomically there. S11 ι only moves `query()` into `query.rs`.
2. **Q2 → (a).** OCCT warm start is reduced to the trait no-op. The inventory registration stays, and INV-GEO-3's OCCT arm is reworded as stateless (leaf ε).
3. **Q3 → (a).** γ1 and θ land first. #7438 carries a note that the control goes to `execute_with_history` inside the `call` closure, plus the edge #7438 → θ.

**Tactical, decided during implementation.**
- The exact name of the new harness unit or units for α/β (`harness_occt_kernel*`), chosen to stay under the kLOC cap.
- Whether the null-topology fixture can be public under `test-fixtures` without weakening the IsNull guard (β).
- The single algorithm for `map_selection_to_indices` (γ2): linear position vs HashMap.
- Whether `ReprPolicy::Inherit` should fall back to `Classify` when the source repr is unknown (today `apply_transform_to_handle` falls back to `Solid`). Suggested: `Classify`.

## 11. Finding-key coverage

| key | disposition in this PRD |
|---|---|
| fk-e6bdef1ef0ae | γ1 |
| fk-d9cc4b340a69 | γ2 (γ1 delivers its D2 half) |
| fk-9eb90a5a82ff | θ |
| fk-10c48f52f01c | β (α is its prerequisite half) |
| fk-68d69d7d9ace | δ |
| fk-d22418138fa2 | θ |
| fk-ba1ea4167dbe | ζ |
| fk-030602c2fd62 | reassigned to S10 (Q1) |
| fk-baae0532b743 | filed #7379 (external prerequisite of ε; not re-filed) |
| fk-69bd53399b08 | ε (warm start reduced to the trait no-op, Q2) |

## Gap-fill amendments (2026-10-16)

Rulings from `plans/gap-fill-deliberation-reify-20261016.md` (Leo, attended sitting). G1 applies: ζ #8669, ε #8682, θ #8672 and ι #8684 are in #8612's dependency closure, so substantive widenings take follow-on form (new leaves after them) and the closure leaves get only sentence-sized changes or notes. The new leaves' capability rows are in the manifest (κ, λ1, λ2, μ blocks). ω #8688 (PRD close) should also wait on the four new leaves; that edge is outside this applier's write scope and is left to the lead.

- **K4-D5** (§K4-D5; contradiction 9; fk-d09e31d2ef9d, fk-2281cf9afc51). ζ #8669 (sentence-sized): `brep_kind_of_shape`'s `_ => BRepKind::Solid` becomes an informative `Err` naming the unrecognised kind. New leaf **κ #9300** after ε #8682: a cxx shared enum `FfiShapeKind` returned by `shape_kind(&OcctShape)` (exhaustive C++ switch; `TopAbs_SHAPE` is a ContractViolation; `From<FfiShapeKind> for BRepKind` with no wildcard; `shape_type_name` deleted), and `ShapeStore` `Entry { shape, repr }` with `get(id) -> (&OcctShape, BRepKind)`, deleting `repr_of`'s Option, both `unwrap_or(BRepKind::Solid)` fallbacks (fuse_all, apply_transform_to_handle) and the `None => {}` arm in run_local_feature_with_history. This refines D4 (the Classify decode becomes typed) and D6 (one store entry) once κ lands; §10's tactical "Inherit falls back to Classify when the source repr is unknown" becomes unrepresentable under κ. Contradiction 9: the warm-state desync check dies with ε (Q2), as the survey says; the other three repr-absent sites are κ's.
- **K4-D6** (§K4-D6; contradiction 10; fk-4fdd3e5ffbc1, fk-4325229a297a, fk-11b120086a30). Owning streams, follow-on form. New design decision **D11 — typed FFI fault**: `wrap_occt_call` encodes one envelope `FfiFault { kind: Contract | Native | Internal, op, detail }`, decoded once in Rust by `From<cxx::Exception>`; one C++ `live_topology` accessor replaces the hand-copied IsNull guards; user text renders from the S10 `GeometryError` variant (`detail`), with `op` a debug field. New leaves: **λ1 #9304** (envelope, decoder, `live_topology`; absorbs #5346, including its empty-message type-name fallback and the doubled-error-line fix); **λ2 #9306** after ι #8684, S10 α₆ #9301, λ1 and μ (production `map_err(|e| OperationFailed(e.to_string()))` sites become `?`; `ShapeStore::get` and the actor return the typed variants; `RUST_GUARD_MARKER` deleted; ratchet row for the to-string map_err shape = 0); **μ #9305** after θ #8672 (θ is not widened): D7's actor gains one `chan_died(op)` reading an `ActorState { Running, Panicked { payload }, Shutdown }` cell, with `catch_unwind` per request and a ratchet row (the "kernel thread died" literal at most once in handle.rs). The `GeometryError` variants themselves are S10's (α₆ #9301, `kernel-interface-typed-seam.md`). #5895 and #5898 are re-scoped onto `KernelFailure.op`. Contradiction 10: no conflict with S14 Q2 / S10 R5 — these variants have producers.
- **K4-D7** (§K4-D7; fk-c1ca74571d29, fk-1343f7f7e1ca). Two low standalone tasks under S11's seam, not leaves of this PRD: **#9307** (after δ #8676 and ι #8684) extracts `cpp/step_io.cpp` (owning the renamed STEP mutex) and a test-fixtures-only `cpp/occt_fixtures.cpp` over a private `occt_common.h`, with the 26 `*_for_test` declarations in ffi.rs gated; **#9308** (after γ1 #8652) moves the AXIS_MAG_SQ_MIN and ANGLE_ABS_MIN Rust/C++ pairs into `floor_constants.rs` with generated C++ literals and one compile-time order assert per pair (CPP_DIR_MAG_MIN documented as a different quantity). #8308 (pending; STEP-import TU split alone) overlaps #9307 — its manifest-repoint constraint is ported into #9307; its disposition is the lead's.
- **Uncontested block — #8676 note.** δ #8676 carries a note on survey chain 7's `CorrespondenceLoss` enum (stop `TopologyCorrespondenceDropTally` rendering C++ counter names as user text); no finding key, not a widening of δ.
