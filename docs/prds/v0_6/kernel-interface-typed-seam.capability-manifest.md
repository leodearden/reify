# Capability manifest — kernel-interface-typed-seam (S10, review-all-reify-20261010)

PRD: `docs/prds/v0_6/kernel-interface-typed-seam.md`. Machine-readable twin: `docs/prds/v0_6/kernel-interface-typed-seam.capability-manifest.yaml` (schema v1, validated with dark-factory `shared.capability_manifest.parse_capability_manifest`). Task ids are stamped by `commit_planning` at decomposition.

**Evidence** taken at main 274a212e0a (2026-10-13). Behavioural probes used `target/debug/reify` (built 2026-10-12) with `LD_LIBRARY_PATH=/opt/reify-deps/tbb-pin:/usr/lib/x86_64-linux-gnu:/opt/reify-deps/lib`. All five `tests/prd-gate/fixtures/kernel_interface_*.ri` fixtures parse under the grammar gate (`tree-sitter parse --quiet`, session-isolated cache, rc=0).

Premise rows hold now. Deliverable rows are "not yet" by construction, and each mechanical check is RED at authoring time in the non-vacuous direction: an `absent` check sees its pattern present today, a `present` check sees it absent (verified by the generator's polarity pass).

**Stamped 2026-10-14 by `commit_planning`:** α₁ #8930, α₂ #8931, α₃ #8932, α₄ #8933, α₅ #8934, β₁ #8935, β₂ #8936, β₃ #8957, γ #8958, δ₁ #8959, δ₂ #8960, δ₃ #8961, δ₄ #8962, δ₆ #8963, δ₇ #8964, ε₁ #8965, ε₂ #8966, ε₃ #8967, ε₄ #8968, ε₅ #8969, ζ₁ #8970, ζ₂ #8971, ζ₃ #8972, ζ₄ #8973, η₁ #8974, η₂ #8975, η₃ #8976, η₄ #8977, η₅ #8978, η₆ #8979, θ₁ #8980, θ₂ #8981, θ₃ #8982, θ₄ #8983, θ₅ #8985, ι₁ #8987, ι₂ #8989, ι₃ #8992, ι₄ #8993, ι₅ #8995, ι₆ #8998, ω #9000. The polarity gate reported nine `absent_overbroad` warnings, none blocking. Six match lines in `crates/reify-ir/src/geometry.rs` or `value.rs`, which the warned leaf itself splits (α₂, α₅ via α₄) or sits transitively after the split of (ε₅, ζ₁, η₄ after α₂). Three are on the broad-sweep leaves β₂/β₃, which declare `files: []` by design, so every match is in the sweep's own footprint.

| leaf | capability | binding (evidence) | verdict | delivered_check |
|---|---|---|---|---|
| α₁ | scc-reproduces | scripts/assert-module-scc.py print at main 274a212e0a (2026-10-13): reify-ir row [[boundary_attachment, expr, geometry, value], [completeness, ranked]]; value.rs imports geometry::{Role, FeatureId, GeometryHandleId, FaceSurfaceKind, EdgeCurveKind}; geometry.rs imports value::{SampledField, Value}. | PASS | manual |
| α₁ | residual-simulated | seat G simulated the move with the real tool on a scratch copy (--root): the row becomes [[completeness, ranked], [expr, value]]; {expr, value} is a type recursion via Value::Closure{body: CompiledExpr} (L10). | PASS | manual |
| α₁ | id-module-exists | new top-level module importing only reify_core | PASS | path `crates/reify-ir/src/geometry_id.rs` present |
| α₂ | mesh-error-cycle | MeshContractViolation::into_geometry_error(self, kernel) builds GeometryError while GeometryError::MeshContractViolation holds MeshInvariant/MeshViolationCounts/MeshWitness; From cannot carry the kernel arg, so GeometryError::from_mesh_violation(kernel, v) replaces it (premise correction 10). | PASS | manual |
| α₂ | facade-needed | 11 names used through reify_ir::geometry:: are not re-exported at the crate root (ParentRole, GEOMETRY_OP_DESCRIPTORS, GeometryOpDiscriminants, descriptor_for, MeshContractMode, MeshInvariant, MeshViolationCounts, MeshWitness, MeshContractViolation, WeldRemap, WeldednessReport); 23 outside files use the path. | PASS | manual |
| α₂ | include-str-pin | producer: task 8533 (in progress) adds crates/reify-geometry/tests/forwards_every_kernel_method.rs with include_str!("../../reify-ir/src/geometry.rs"); α₂ depends on #8533, #8284 and #7379 (each edits geometry.rs) and repoints the include. #8284's delivered check is widened to crates/reify-ir/src by the decompose session (L9). | PASS | manual |
| α₂ | bridge-deleted | the mesh->error upward call is deleted | PASS | grep `fn into_geometry_error` absent in `crates/reify-ir/src` |
| α₂ | mesh-child-exists | lower-strata child module | PASS | path `crates/reify-ir/src/geometry/mesh.rs` present |
| α₃ | registration-placement | KernelRegistration's factory returns Box<dyn GeometryKernel>; placing it beside CapabilityDescriptor would close capability -> kernel -> op -> capability, so it lives in kernel.rs. | PASS | manual |
| α₃ | kernel-child-exists | the trait's new home | PASS | path `crates/reify-ir/src/geometry/kernel.rs` present |
| α₄ | selector-cycle | GeometryHandleRef::from_geometry_handle(v: &Value) (value.rs) takes Value, so selector.rs would import from value while value imports selector; it becomes Value::as_geometry_handle_ref (23 sites). | PASS | manual |
| α₄ | value-delivered-checks-pinned | trampoline-decode-and-result-build.capability-manifest.yaml pins pub fn registry_free / flat_index_3d / same_grid to crates/reify-ir/src/value.rs; α₄ does the three-write repoint for #8686/#8711/#8717. | PASS | manual |
| α₄ | value-dir-exists | facade module | PASS | path `crates/reify-ir/src/value/mod.rs` present |
| α₅ | overlay-keys-today | MATERIALIZED_ANNOTATIONS_KEY and SOURCE_SPAN_KEY live in StructureInstanceData.fields; user_fields() filters at 9 value.rs sites; no production code outside value.rs names either key (seat G 1.4). | PASS | manual |
| α₅ | registry-free-upstream | producer: task 8686 (S15 δ, pending) adds StructureInstanceData::registry_free; task 7867 (pending) removes type_id from the same literals; α₅ depends on both. | PASS | manual |
| α₅ | overlay-keys-gone | the @@ overlay key literal is deleted | PASS | grep `@@materialized_annotations` absent in `crates/reify-ir/src` |
| α₆ | kernel-failure-variant | gap-fill 2026-10-16 §K4-D6 (fk-4fdd3e5ffbc1): `GeometryError` has no corrupt-handle, contract-violation or kernel-failure variant at main `c7bbfe2519` | PASS | grep `KernelFailure \{` present in `crates/reify-ir/src` |
| α₆ | corrupt-handle-variant | no `CorruptHandle(` in `crates/reify-ir/src` at `c7bbfe2519`; producer is OCCT `ShapeStore::get` (S11 λ2) | PASS | grep `CorruptHandle\(` present in `crates/reify-ir/src` |
| α₆ | error-module-upstream | producer: α₂ #8931 (geometry/error.rs) | PASS | manual |
| β₁ | no-live-consumer | R5 (Leo 2026-10-13): no wasm crate or cfg (git grep wasm32 over crates/ = 0), check-manifold-deps.sh already fails every verify without the deps; no gate compiles any not(has_*) arm. | PASS | manual |
| β₁ | override-taken-on-trust | reify_build_utils::find_dir_with_override returns a non-empty override verbatim and lets an empty one fall through to the candidates, so the stub arm is unreachable on a provisioned host (premise correction 5). | PASS | manual |
| β₁ | require-helper | one build-time decision with a unit-tested message | PASS | grep `pub fn require\(` present in `crates/reify-build-utils/src/lib.rs` |
| β₁ | occt-stub-deleted | the OCCT stub module is gone | PASS | path `crates/reify-kernel-occt/src/stubs.rs` absent |
| β₁ | kappa-subsumed | S14 κ #8632 (pending) adds GeometryError::BackendUnavailable whose only producer is the gmsh stub arm R5 deletes; the decompose session cancels κ naming β₁ and moves fk-c497935d4b8a onto β₁ (L13). | PASS | manual |
| β₁ | every-detector-requires | nine build scripts detect a native dep today (five kernels incl. conformance, plus reify-cli, reify-eval, reify-mesh-morph, gui) via reify_build_utils::find(..).is_some()/match; all switch to require | PASS (corrected 2026-10-20, retro-critic R12: grep → ratchet row) | manual — R12 polarity: a find()-private diff alone (the subsumed S21 β #8646) would turn a `reify_build_utils::find\(` absent grep green without any require(), so it is held as an `adoption-ratchet.manifest` row at ceiling 0 (S21 M1) |
| β₁ | hand-written-cfg-sites-gone | moved from S21 β #8646 (subsumed, R12): the 10 hand-written cargo:rustc-cfg=has_<dep> lines in the 8 build.rs are gone; require() emits the cfg for as long as has_* lives (β₂/β₃ sweep it). Baseline at S21 authoring (main 8e6ffcac9d): 10. | PASS | grep `rustc-cfg=has_` absent in `crates/reify-eval/build.rs`, `crates/reify-cli/build.rs`, `crates/reify-kernel-conformance/build.rs`, `crates/reify-mesh-morph/build.rs`, `crates/reify-kernel-occt/build.rs`, `crates/reify-kernel-gmsh/build.rs`, `crates/reify-kernel-openvdb/build.rs`, `gui/src-tauri/build.rs` |
| β₂ | counts | has_occt 381 lines/124 files, has_gmsh 357/56, has_openvdb 364/45, *_AVAILABLE 652 references workspace-wide (git grep -c at main 274a212e0a (2026-10-13)). | PASS | manual |
| β₂ | kernel-cfgs-gone | no cfg(has_*) gate in the kernel crates | PASS | grep `cfg\(has_(occt\|gmsh\|openvdb)` absent in `crates/reify-kernel-occt/src`, `crates/reify-kernel-gmsh/src`, `crates/reify-kernel-openvdb/src` |
| β₂ | stub-register-gone | the dead stub_register features are deleted | PASS | grep `stub_register` absent in `crates/reify-kernel-gmsh/Cargo.toml`, `crates/reify-kernel-openvdb/Cargo.toml` |
| β₃ | readers-today | OCCT_AVAILABLE readers: 110 files in reify-eval, 15 reify-cli, 4 gui/src-tauri, 1 solver-elastic; GMSH_AVAILABLE: 2 reify-eval, 1 mesh-morph, 6 solver-elastic (fold-check N1, git grep -l at 2c0c57d71c). | PASS | manual |
| β₃ | eval-cfgs-gone | no cfg(has_*) gate in reify-eval sources | PASS | grep `cfg\(has_(occt\|gmsh\|openvdb)` absent in `crates/reify-eval/src` |
| β₃ | occt-available-gone | the constant itself is deleted | PASS | grep `pub const OCCT_AVAILABLE` absent in `crates/reify-kernel-occt/src` |
| γ | coverage-today | assert-crate-dag.sh B1-B6 constrain 4 of 37 packages (cargo metadata --no-deps); not a verify-pipeline file (verify-pipeline-guard.sh requires-full-gate exits 1). | PASS | manual |
| γ | strata-table | the authority table | PASS | path `scripts/crate-strata.json` present |
| δ₁ | arms-today | assert_kernel_contract! has stub; (5 tests) and real; (7 tests); real; needs valid_op executable on a fresh kernel and extract_* success, which Manifold, OpenVDB and Gmsh cannot satisfy (seat F 1.1). | PASS | manual |
| δ₁ | core-module | the neutral core module | PASS | path `crates/reify-test-support/src/kernel_assertions/core.rs` present |
| δ₂ | instantiations-today | occt 2 (real gate-run, stub not), fidget/manifold 0, gmsh/openvdb only assert_stub_kernel_errors! under not(has_*) (git grep at main 274a212e0a (2026-10-13)). | PASS | manual |
| δ₂ | manifold-upstream | producer: task 8529 (in progress) rewrites manifold's catch-all messages in the same kernel.rs; δ₂ depends on it. | PASS | manual |
| δ₂ | fidget-contract | Fidget instantiation file | PASS | path `crates/reify-kernel-fidget/tests/kernel_contract.rs` present |
| δ₂ | manifold-contract | Manifold instantiation file | PASS | path `crates/reify-kernel-manifold/tests/kernel_contract.rs` present |
| δ₃ | divergences-today | Fidget query QueryFailed for any handle; Manifold Distance/Contains/GeoEquiv map InvalidReference to QueryFailed and export wraps TessError into FormatError; OpenVDB execute OperationFailed and tessellate TessellationFailed for a dangling handle (seat F 1.3). | PASS | manual |
| δ₃ | primary-handle-lifted | one exhaustive home for 'which handle does this query name' | PASS | grep `pub fn primary_handle` present in `crates/reify-ir/src` |
| δ₄ | mock-accepts-dangling | MockGeometryKernel execute returns Ok for any op, export writes MOCK_EXPORT_DATA and tessellate returns a mesh for any handle (seat F 1.5); 1270 non-comment lines in 135 files reference it, so strictness is opt-in. | PASS | manual |
| δ₄ | mock-contract | the mock's contract binary | PASS | path `crates/reify-test-support/tests/mock_kernel_contract.rs` present |
| δ₆ | doubles-census | 79 concrete impls: 5 real, 3 stub, 1 holder, 70 test doubles; 52 bespoke outside kernel crates, reify-ir and test-support (census_f.py at main 274a212e0a (2026-10-13)). | PASS | manual |
| δ₆ | doubles-module | the shared building blocks | PASS | path `crates/reify-test-support/src/kernel_doubles.rs` present |
| δ₇ | replaceable-rule | replaceable iff each override is all-error, a Mock configuration, or ObservingKernel with at most four hooks; estimate 35-40 of 52 (seat F 3.5); the ledger is the leaf's first deliverable. | PASS | manual |
| ε₁ | override-pattern | 27 methods (4 required, 23 defaulted), each defaulted method overridden by at least one real kernel; attribute_hook is already an Option accessor (seat A 1.1). | PASS | manual |
| ε₁ | handle-upstream | producer: task 8672 (S11 θ, pending) collapses OcctKernelHandle to call<R>; R1 orders ε₁ after it. | PASS | manual |
| ε₁ | tessellate-with-topology-first | #8672 depends on #8284 (get_task 8672), and #8284 adds a defaulted GeometryKernel::tessellate_with_topology, so ε₁ always sees 28 methods and moves the 28th into TopologyTessellation (premise correction 11). | PASS | manual |
| ε₁ | sub-trait-defined | the first capability sub-trait | PASS | grep `trait TopologyExtraction` present in `crates/reify-ir/src` |
| ε₁ | topology-tessellation-sub-trait | #8284's method in its own sub-trait | PASS | grep `trait TopologyTessellation` present in `crates/reify-ir/src` |
| ε₂ | masking-proxies | rounded_primitives_e2e.rs::RecordingKernel, unified_dag_geometry_executors.rs::OcctOpRecorder and fillet_curated_edges_e2e.rs::RecordingKernel each claim to forward all calls and override 16 of 27 (seat A 1.3). | PASS | manual |
| ε₂ | forward-macro | the one forwarding list beside the trait | PASS | grep `macro_rules! forward_kernel_capabilities` present in `crates/reify-ir/src` |
| ε₂ | delegation-lint | the workspace lint | PASS | path `crates/reify-geometry/tests/kernel_wrappers_forward_every_capability.rs` present |
| ε₃ | call-sites | extract_* production call sites through a GeometryKernel receiver: 60 in reify-eval (seat A 1.2 rule). | PASS | manual |
| ε₃ | facade | the reify-ir call facade | PASS | path `crates/reify-ir/src/kernel_calls.rs` present |
| ε₄ | receivers | realize_mesh_from_voxel/surface_options_content_hash and the gmsh methods are reached through kernels.get (shared borrow); densify/ingest_mesh_at_resolution through get_mut, which is why the voxel group splits (seat A 3.1). | PASS | manual |
| ε₅ | closest-point-duplicate | GeometryQuery::ClosestPointOnShape exists and OCCT's query() answers it via the inherent closest_point_on_shape (seat A 1.1); typed by η₂. | PASS | manual |
| ε₅ | closest-point-trait-method-gone | the duplicate trait method is deleted | PASS | grep `fn closest_point_on_shape` absent in `crates/reify-ir/src` |
| ζ₁ | entry-points-today | validate (clones; 2 production callers discard the witness), into_validated (0), check_mesh_contract (0), check_mesh_contract_welded (1, manifold); check_contract 343 lines (seat E 1.1-1.2). | PASS | manual |
| ζ₁ | validate-gone | the cloning entry point is deleted | PASS | grep `pub fn validate\(&self, tol` absent in `crates/reify-ir/src` |
| ζ₂ | six-raw-mesh-methods | ingest_mesh, ingest_mesh_at_resolution, register_mesh_handle, measure_mesh_deviation, mesh_surface_to_volume, mesh_surface_to_volume_attributed take &Mesh; ValidatedMesh appears in 0 signatures outside geometry.rs (seat E 1.3). | PASS | manual |
| ζ₂ | export-routing-upstream | producer: task 6308 (in progress) fixes the export path that made isosurface(box) export 2 triangles; ζ₂'s MarchingCubes premise check runs after it (premise correction 1). | PASS | manual |
| ζ₂ | manifold-oncelock-gone | the kernel-local posture is deleted | PASS | grep `static MODE` absent in `crates/reify-kernel-manifold/src/kernel.rs` |
| ζ₂ | front-door | the one mint site in reify-eval | PASS | grep `fn admit_mesh` present in `crates/reify-eval/src` |
| ζ₃ | copies-today | ElementOrderTag::P1 => occurs on 7 lines in 7 gmsh files: cache_key.rs (frozen discriminant), three Mesh.ElementOrder option values, three tet strides (seat E 1.6). | PASS | manual |
| ζ₃ | relocation-upstream | producer: task 7636 (pending) moves tet_element_spec into tet_readback.rs. | PASS | manual |
| ζ₃ | real-surface-pin | crates/reify-eval/tests/volume_mesh_realization_e2e.rs exercises real gmsh P1/P2 volume meshes; its node/element counts are the behaviour pin. | PASS | manual |
| ζ₃ | tables-on-tag | the reify-ir home | PASS | grep `fn nodes_per_tet` present in `crates/reify-ir/src` |
| ζ₄ | mc-unmeasured | the only e2e on the MarchingCubes path asserts non-empty; #6308's branch asserts triangle counts and bbox, not closedness; seat E's open-mesh readings came through #6308's misrouted export (premise correction 1). ζ₄ measures check_mesh_contract(0.0) first and escalates on failure. | PASS | manual |
| ζ₄ | exemption-gone | the MarchingCubes exemption at the handoff is deleted | PASS | grep `!from_marching_cubes` absent in `crates/reify-eval/src` |
| η₁ | untyped-today | fn query(&self, q: &GeometryQuery) -> Result<Value, QueryError> over 35 variants; 99 fn query/query_many definitions; 51 reify-eval production call sites (seat B 1.6-1.7). | PASS | manual |
| η₁ | query-rs-upstream | producer: task 8684 (S11 ι) moves OCCT query() into query.rs; task 8751 (S13 ε) completes the geometry_ops partition; η₁ depends on both. | PASS | manual |
| η₁ | reply-type | the typed reply | PASS | grep `pub enum QueryReply` present in `crates/reify-ir/src` |
| η₂ | xyz-parsers-today | parse_xyz_value/parse_xyz_json (15 production call sites), parse_normal_z, OCCT centroid_json + 4 inline format! sites, Manifold json_xyz (seat B 1.4-1.5). | PASS | manual |
| η₂ | xyz-parser-gone | the xyz wrapper is deleted | PASS | grep `fn parse_xyz_value` absent in `crates/reify-eval/src` |
| η₃ | tokenizers-today | parse_flat_number_object, parse_bbox_all_extents, parse_normal_z, parse_bbox_xyz_min in reify-eval plus Aabb::from_kernel_json in reify-test-support (5 tokenizers); Axis::as_byte passes b'x'/b'y'/b'z' (seat B 1.5). | PASS | manual |
| η₃ | oracle-captured | reify eval tests/prd-gate/fixtures/kernel_interface_query_reply_oracle.ri at main 274a212e0a (2026-10-13) exits 0 printing Probe.bb = bbox(point(-0.0040001 m, ...)) and Probe.c / Probe.cp points; the capture is the byte-identity oracle for η₁-η₆. | PASS | manual |
| η₃ | flat-tokenizer-gone | the shared tokenizer is deleted | PASS | grep `fn parse_flat_number_object` absent in `crates/reify-eval/src` |
| η₃ | mock-json-parser-gone | the test-support JSON parser is deleted | PASS | grep `fn from_kernel_json` absent in `crates/reify-test-support/src` |
| η₄ | kind-copies | FaceSurfaceKind/EdgeCurveKind names: occt_wrapper.cpp switch, try_from_str (geometry.rs), geometry_ops expected-one-of literals; OCCT returns Value::String from the FFI (seat B 1.3); #8661 does not cover these enums. | PASS | manual |
| η₄ | args-upstream | producer: task 8748 (S13 β) moves resolve_*_kind_arg into geometry_ops/args.rs. | PASS | manual |
| η₄ | hand-parser-gone | the hand-written name parsers are deleted | PASS | grep `fn try_from_str` absent in `crates/reify-ir/src` |
| η₅ | dual-shape-drift | ShapeLocalTolerance doc says Real, OCCT returns Value::length (Scalar); eval scalar consumers accept both shapes (seat B 1.3). | PASS | manual |
| η₆ | reply-kind-contract | the contract core asserts reply.kind() == query.reply_kind() | PASS | grep `reply_kind\(\)` present in `crates/reify-test-support/src` |
| θ₁ | no-newtype-today | git grep 'pub struct \w+\((pub )?f64\)' over crates = 0; Value::as_f64 accepts any-dimension Scalar (seat C 1.4). | PASS | manual |
| θ₁ | halfspace-probe | reify eval tests/prd-gate/fixtures/kernel_interface_halfspace_nonnumeric_normal.ri at main 274a212e0a (2026-10-13): exit 1, 'error: geometry error: geometry operation failed: expected numeric value' with no op, argument or span. | PASS | manual |
| θ₁ | op-args-upstream | producer: task 8652 (S11 γ1, in progress) owns op_args.rs; task 8669 (S11 ζ) rewrites every execute arm; θ₁ depends on both. | PASS | manual |
| θ₁ | quantity-module | the newtype home | PASS | path `crates/reify-ir/src/quantity.rs` present |
| θ₂ | sweep-classifier-decodes | sweep_classifier.rs decodes Extrude.distance (:350) and profile extents (:628) via as_f64 with no tripwire (seat C 1.3). | PASS | manual |
| θ₂ | as-f64-gone | the untyped decodes are deleted | PASS | grep `as_f64\(\)` absent in `crates/reify-eval/src/sweep_classifier.rs` |
| θ₃ | false-rationale | kernel_validation.rs says arg_acceptance 'lives in reify-eval ... cannot be shared'; arg_acceptance moved into reify-ir under #5791 (done); stale geometry_ops.rs:482/:523 pointers (seat C 2). | PASS | manual |
| θ₃ | macro-gone | the zero-invocation macro and its kin are deleted | PASS | grep `define_kernel_length_extract` absent in `crates/reify-ir/src`, `crates/reify-kernel-occt/src`, `crates/reify-kernel-fidget/src` |
| θ₄ | f64-surface | under the finding's own rule the split is 23 Value-only / 21 f64-family / 4 mixed / 7 none (55 variants); 14 scalar + 14 point-like LENGTH f64 fields, 5 ANGLE (seat C 1.1). | PASS | manual |
| θ₄ | justification-guard | the closed taxonomy for bare numeric fields | PASS | grep `enum Justification` present in `crates/reify-ir/tests` |
| θ₅ | scale-probe | reify eval tests/prd-gate/fixtures/kernel_interface_scale_dimensioned_factor.ri at main 274a212e0a (2026-10-13): exit 0, ScaledBlock.v_bad = 0.000000000008 m^3 (rejection-absent today; G6 branch 4 observed). | PASS | manual |
| θ₅ | count-probe | reify eval tests/prd-gate/fixtures/kernel_interface_pattern_dimensioned_count.ri at main 274a212e0a (2026-10-13): exit 0, Pat.vp = 0.0000029999999999999997 m^3 (3000mm read as 3). | PASS | manual |
| θ₅ | rejection-code-exists | DiagnosticCode::DimensionedArgRejected exists in reify-core diagnostics.rs; dimensionless_spec exists in reify_ir::arg_acceptance (S15-owned, consumed unchanged). | PASS | manual |
| θ₅ | docs-truth-four-surfaces | examples/best_practices/dimensioned_arguments.ri ('the gate runs in ONE direction only'), INDEX.md:61, .claude/skills/reify-design/SKILL.md:105 and chunks/geometry.md:236 ('scale(g, 2mm) is accepted') document today's posture; θ₅ rewrites all four (overlay docs-truth gate). | PASS | manual |
| θ₅ | chunk-claim-rewritten | the chunk's acceptance claim is replaced | PASS | grep `authoring rule you uphold` absent in `crates/reify-mcp/src/tools/chunks/geometry.md`, `crates/reify-mcp/src/tools/chunks/units.md`, `examples/best_practices/dimensioned_arguments.ri` |
| θ₅ | dimensionless-reader | the dimension-checked reader | PASS | grep `fn required_dimensionless_arg` present in `crates/reify-eval/src` |
| ι₁ | pragma-probe | #kernel(gmsh) prints "unknown kernel 'gmsh'; v0.2 supports {fidget, manifold, occt, openvdb}" (seat D 1.5); reify-compiler already depends on reify-core. | PASS | manual |
| ι₁ | pragma-predicate | the one statement of the gmsh exclusion | PASS | grep `fn is_pragma_target` present in `crates/reify-core/src/kernel.rs` |
| ι₂ | undeclared-ops | git grep 'TransformAffineApply\|SurfaceNurbs' crates/reify-kernel-*/src/register.rs = 0 at main 274a212e0a (2026-10-13); affine_tapered_spacer.ri builds only via the sentinel None-arm (premise correction 3). | PASS | manual |
| ι₂ | affine-declared | OCCT's descriptor names AffineApply | PASS | grep `TransformAffineApply` present in `crates/reify-kernel-occt/src/register.rs` |
| ι₂ | completeness-test | the completeness test, beside all five kernel dev-deps | PASS | path `crates/reify-kernel-conformance/tests/descriptor_completeness.rs` present |
| ι₃ | sentinel-mode | every production front end (CLI cmd_check/build/eval, GUI EngineSession) calls with_registered_kernel, giving {__reify_eval_default_kernel: occt} (+ openvdb via ensure_openvdb_kernel) (seat D 1.2). | PASS | manual |
| ι₃ | owner-routing-upstream | producer: task 6308 (in progress) adds kernel_owner.rs and rewrites the export paths; ι₃ depends on it. | PASS | manual |
| ι₃ | kernel-set | the typed map | PASS | path `crates/reify-eval/src/kernel_set.rs` present |
| ι₃ | sentinel-gone | the synthetic key is deleted | PASS | grep `DEFAULT_KERNEL_NAME` absent in `crates/reify-eval/src` |
| ι₄ | boolean-stl-probe | reify build tests/prd-gate/fixtures/kernel_interface_boolean_stl_export.ri -o x.stl at main 274a212e0a (2026-10-13): exit 1, 'internal error: dispatcher named target kernel 'manifold' not present', STL of 12 triangles spanning x in [0, 10] mm (box_b only). | PASS | manual |
| ι₄ | cli-test-ownership | #6104 (pending, dep #6308) prescribes the same harness_cli test file; at decomposition #6104 gets a dependency on ι₄, which creates the file (L15). | PASS | manual |
| ι₄ | select-fn | the one selection function | PASS | grep `pub fn select\(` present in `crates/reify-eval/src/dispatcher.rs` |
| ι₅ | silent-fallback | 7 handle-provenance lookups fall back to the default kernel silently; 9 minted provenance tags are always KernelId::Occt (seat D 1.1). | PASS | manual |
| ι₅ | owner-routing-upstream | producer: implicit-solids γ #8391 (pending, high) owns owner-routed-or-rejected queries with QueryKernelMismatch/QueryKernelUnserved and five delivered checks; ι₅ depends on it and keeps those constructs (L14). #6366 keeps its three sites. | PASS | manual |
| ι₅ | default-query-kernel-gone | the default-kernel query accessor is deleted | PASS | grep `fn default_query_kernel` absent in `crates/reify-eval/src` |
| ι₆ | conversions-already-rows | gmsh (Convert{Mesh}, VolumeMesh) and openvdb (Convert{Mesh}, Voxel) are declared rows; the 7 hard-coded KernelId::Gmsh/OpenVdb lookups bypass them (premise correction 2). | PASS | manual |
| ι₆ | projections-row | the new descriptor field | PASS | grep `pub projections:` present in `crates/reify-ir/src` |
| ι₆ | openvdb-name-helper-gone | the hard-coded OpenVDB selection helper and its 14 call sites go | PASS | grep `fn openvdb_kernel_name` absent in `crates/reify-eval/src` |
| ω | terminal-stamp | overlay decompose-close obligation | PASS | manual |

**Not bound here:** fk-ef5f5da8f729 (filed #8533), fk-10ec3d2a1b76 (filed #8529), fk-65bb1ffb5d60 (filed #8561, referenced by γ's exception row), fk-92d313d2986f (stamped onto meshing-service β #8285, R4).
