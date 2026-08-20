//! Task 4351 step-3 — RED (reify-eval e2e, cold-population half).
//!
//! `TopologyAttributeTable` is now keyed by the full `KernelHandle {kernel,
//! id}` (step-1/2), but the cold-population WRITE path
//! (`engine_build.rs`'s `seed_primitive_attributes_for_handle` /
//! `populate_attribute_history` call sites) still tags every recorded
//! attribute with an interim `KernelId::Occt`, irrespective of which kernel
//! the dispatcher actually routed the op to (see the "interim ... step-4"
//! comments at those two call sites). This test drives a build through a
//! mock `GeometryKernel` registered under registry-name `"manifold"` (so
//! `kernel_id_for_registry_name("manifold") == KernelId::Manifold`, proven
//! in `reify-core`) and shows the seeded face is recorded under
//! `{Occt, id}` instead of the correct `{Manifold, id}` — the write-side
//! half of the cross-kernel `GeometryHandleId` collision task 4351 fixes
//! (a second, truly cross-kernel build would have this record collide with
//! — and be indistinguishable from — an Occt-kernel record at the same
//! numeric id).
//!
//! GREEN is step-4: `engine_build.rs` binds
//! `op_kernel = kernel_id_for_registry_name(&resolved_kernel_name)` (the
//! same expression already used to tag `step_handles`) and threads it into
//! `seed_primitive_attributes_for_handle` / `populate_attribute_history` in
//! place of the interim `KernelId::Occt`.
//!
//! Patterns reused:
//! - `Engine::with_test_kernels_and_registry` + a caller-supplied
//!   `CapabilityDescriptor` registry — the mock-registry seam from
//!   `crates/reify-eval/tests/cross_kernel_handoff.rs`.
//! - Staged `with_extracted_faces` / `with_extracted_edges` on a bare
//!   `MockGeometryKernel` so the primitive-attribute seeder actually
//!   populates the table — the fixture from
//!   `crates/reify-eval/tests/tolerance_wiring_e2e.rs`'s
//!   `cache_hit_short_circuit_leaves_topology_attribute_table_empty_after_second_build`.

use std::collections::{BTreeMap, HashMap};

use reify_compiler::{CompiledGeometryOp, PrimitiveKind};
use reify_core::{ModulePath, SourceSpan, Type};
use reify_eval::{Engine, try_eval_ad_hoc_selector};
use reify_ir::{
    CapabilityDescriptor, CompiledExpr, ExportFormat, GeometryHandleId, KernelHandle, KernelId,
    Operation, ReprKind, SelectorKind, Value,
};
use reify_test_support::builders::{CompiledModuleBuilder, TopologyTemplateBuilder};
use reify_test_support::mocks::MockGeometryKernel;
use reify_test_support::{MockConstraintChecker, manufacturing_purpose, mm, step_output_template};

/// RED: a sphere primitive dispatched to a kernel registered under
/// registry-name `"manifold"` must have its seeded face attribute
/// addressable under `KernelHandle{kernel: KernelId::Manifold, ..}` — and
/// NOT under `KernelHandle{kernel: KernelId::Occt, ..}`. Before step-4
/// threads the real op kernel through the write path, the seeder always
/// tags with the interim `KernelId::Occt`, so this fails: the Manifold
/// lookup misses and the Occt lookup (wrongly) hits.
#[test]
fn cold_population_records_seeded_face_under_dispatched_kernel_not_interim_occt() {
    let mm_lit = |v: f64| CompiledExpr::literal(mm(v), Type::length());
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_lit(5.0))],
    };
    let sphere_realization_template = TopologyTemplateBuilder::new("MyDesign")
        .param("MyDesign", "thickness", Type::dimensionless_scalar(), None)
        .realization_named("MyDesign", 0, "body", vec![sphere_op])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_cross_kernel_attribute_collision".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(sphere_realization_template)
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    // The mock is the sole kernel driven for this realization, so its first
    // (and only) handle is `GeometryHandleId(1)` — stage its face/edge
    // extraction so the primitive seeder actually populates
    // `topology_attribute_table` (mirrors tolerance_wiring_e2e.rs).
    let seeded_solid = GeometryHandleId(1);
    let seeded_face = GeometryHandleId(101);
    let seeded_edge = GeometryHandleId(102);
    let kernel = MockGeometryKernel::new()
        .with_extracted_faces(seeded_solid, vec![seeded_face])
        .with_extracted_edges(seeded_solid, vec![seeded_edge]);

    let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
    kernels.insert("manifold".to_string(), Box::new(kernel));

    // Grant the mock the driven primitive op at BRep — mirrors the
    // single-kernel "occt"-only registries used throughout the existing
    // OCCT e2e fixtures, just renamed to "manifold" so
    // `kernel_id_for_registry_name` resolves `KernelId::Manifold`.
    let mut registry: BTreeMap<String, CapabilityDescriptor> = BTreeMap::new();
    registry.insert(
        "manifold".to_string(),
        CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveSphere, ReprKind::BRep)],
        },
    );

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::with_test_kernels_and_registry(
        Box::new(checker),
        kernels,
        registry,
        Some("manifold".to_string()),
    );

    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    let build = engine.build(&module, ExportFormat::Step);
    let errors: Vec<_> = build
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "build against the mock-manifold registry must not emit error diagnostics; got: {errors:?}"
    );

    let recorded_under_manifold = engine
        .topology_attribute_table()
        .lookup(KernelHandle {
            kernel: KernelId::Manifold,
            id: seeded_face,
        })
        .is_some();
    let recorded_under_occt = engine
        .topology_attribute_table()
        .lookup(KernelHandle {
            kernel: KernelId::Occt,
            id: seeded_face,
        })
        .is_some();

    // SANITY: the op-loop actually populated the table for the seeded face
    // under SOME kernel scope (non-vacuousness premise for the two core
    // assertions below — a seeding failure would leave both lookups None
    // and vacuously satisfy the "not under Occt" assertion for the wrong
    // reason).
    assert!(
        recorded_under_manifold || recorded_under_occt,
        "sanity: expected topology_attribute_table to contain an entry for \
         seeded_face {seeded_face:?} under SOME kernel scope after the build \
         — the primitive-attribute seeder in the op-loop must record the \
         sphere's extracted face. If neither scope has an entry, the seeding \
         path itself is broken (a different regression than this test targets)."
    );

    // CORE ASSERTION (task 4351 root cause): the sphere was dispatched to
    // the kernel registered under registry-name "manifold"
    // (`kernel_id_for_registry_name("manifold") == KernelId::Manifold`), so
    // its seeded face attribute must be addressable under
    // `{Manifold, seeded_face}`. RED today: the write path still tags every
    // record with the interim `KernelId::Occt` (engine_build.rs — step-4
    // threads the real op kernel through `seed_primitive_attributes_for_handle`
    // / `populate_attribute_history`), so this lookup misses.
    assert!(
        recorded_under_manifold,
        "expected topology_attribute_table.lookup(KernelHandle{{kernel: \
         KernelId::Manifold, id: {seeded_face:?}}}) to be Some — the sphere \
         was dispatched to the kernel registered under registry-name \
         \"manifold\" (KernelId::Manifold), so its seeded face attribute \
         must be recorded under the Manifold scope. Got None: the write path \
         is still tagging records with the interim KernelId::Occt."
    );
    // CORE ASSERTION (the collision half): no entry must leak into the Occt
    // scope for a kernel that was never Occt. RED today: it does, because
    // the write path hardcodes KernelId::Occt — this is precisely the
    // cross-kernel collision (two kernels' attributes addressable under the
    // same numeric id) that motivates the KernelHandle re-key.
    assert!(
        !recorded_under_occt,
        "expected topology_attribute_table.lookup(KernelHandle{{kernel: \
         KernelId::Occt, id: {seeded_face:?}}}) to be None — the sphere was \
         dispatched to \"manifold\" (KernelId::Manifold), not Occt, so no \
         entry should exist under the Occt scope. Got Some: this is the \
         cross-kernel GeometryHandleId collision task 4351 fixes — the write \
         path is still recording under the interim KernelId::Occt regardless \
         of the dispatched kernel."
    );
}

/// Task 4351 step-5 — RED (resolution-scope half, end-to-end signal).
///
/// Drives the same mock-"manifold" build as the write-path test above (so
/// the sphere's seeded face attribute is recorded under
/// `{Manifold, seeded_face}` per step-4), then evaluates a `@face("side")`
/// ad-hoc selector against the manifold body's named step directly through
/// `try_eval_ad_hoc_selector` — its public signature is stable, so no
/// plumbing changes are needed to call it here — and asserts it resolves to
/// `Value::Frame` rather than `Value::Undef`.
///
/// RED today: `resolve_unique_by_attribute`'s internal lookups (the
/// imported-geometry fallback pre-pass and `collect_matches`) are still
/// scoped to the interim `KernelId::Occt` (step-2), so against a table
/// whose only entry is `{Manifold, seeded_face}`, the fallback pre-pass
/// finds no Occt-scoped candidate and short-circuits to
/// `FallbackToComputed` — `try_eval_ad_hoc_selector` then returns
/// `Some(Value::Undef)` without ever reaching the kernel query. GREEN is
/// step-6: threading a `scope_kernel` parameter through the resolver so
/// `try_eval_ad_hoc_selector` (via `named_steps.get(name).kernel`) supplies
/// `KernelId::Manifold` and the lookup hits.
#[test]
fn ad_hoc_face_selector_resolves_against_manifold_scoped_attribute() {
    let mm_lit = |v: f64| CompiledExpr::literal(mm(v), Type::length());
    let sphere_op = CompiledGeometryOp::Primitive {
        kind: PrimitiveKind::Sphere,
        args: vec![("radius".into(), mm_lit(5.0))],
    };
    let sphere_realization_template = TopologyTemplateBuilder::new("MyDesign")
        .param("MyDesign", "thickness", Type::dimensionless_scalar(), None)
        .realization_named("MyDesign", 0, "body", vec![sphere_op])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::new(vec![
        "test_cross_kernel_attribute_collision_selector".to_string(),
    ]))
    .template(step_output_template(1e-6))
    .template(sphere_realization_template)
    .compiled_purpose(manufacturing_purpose("manufacturing", 1e-6))
    .build();

    let seeded_solid = GeometryHandleId(1);
    let seeded_face = GeometryHandleId(101);
    let seeded_edge = GeometryHandleId(102);
    let kernel = MockGeometryKernel::new()
        .with_extracted_faces(seeded_solid, vec![seeded_face])
        .with_extracted_edges(seeded_solid, vec![seeded_edge]);

    let mut kernels: BTreeMap<String, Box<dyn reify_ir::GeometryKernel>> = BTreeMap::new();
    kernels.insert("manifold".to_string(), Box::new(kernel));

    let mut registry: BTreeMap<String, CapabilityDescriptor> = BTreeMap::new();
    registry.insert(
        "manifold".to_string(),
        CapabilityDescriptor {
            supports: vec![(Operation::PrimitiveSphere, ReprKind::BRep)],
        },
    );

    let checker = MockConstraintChecker::new();
    let mut engine = Engine::with_test_kernels_and_registry(
        Box::new(checker),
        kernels,
        registry,
        Some("manifold".to_string()),
    );

    let _eval = engine.eval(&module);
    engine.activate_purpose("manufacturing", "MyDesign");

    let build = engine.build(&module, ExportFormat::Step);
    let errors: Vec<_> = build
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, reify_core::Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "build against the mock-manifold registry must not emit error diagnostics; got: {errors:?}"
    );

    // SANITY: the write path (step-4) recorded the seeded face under the
    // Manifold scope — the non-vacuousness premise for the selector
    // resolution below. If this fails, the write-path regressed; that is a
    // different bug than the resolver-scope one this test targets.
    let table = engine.topology_attribute_table();
    assert!(
        table
            .lookup(KernelHandle {
                kernel: KernelId::Manifold,
                id: seeded_face,
            })
            .is_some(),
        "sanity: expected the seeded face to be recorded under \
         KernelHandle{{Manifold, {seeded_face:?}}} after the build (step-4)."
    );

    // A fresh mock kernel — independent of the engine's internal kernel —
    // configured to answer the `extract_faces` / `Centroid` / `FaceNormal`
    // queries `try_eval_ad_hoc_selector` issues for the manifold body's
    // seeded face. Mirrors `ad_hoc_selector_smoke_tests.rs`'s
    // `configured_kernel()` fixture.
    let centroid_json = Value::String(r#"{"x":0.0,"y":0.0,"z":0.005}"#.to_string());
    let normal_json = Value::String(r#"{"x":0.0,"y":0.0,"z":1.0}"#.to_string());
    let mut selector_kernel = MockGeometryKernel::new()
        .with_extracted_faces(seeded_solid, vec![seeded_face])
        .with_centroid_result(seeded_face, centroid_json)
        .with_face_normal_result(seeded_face, normal_json);

    let mut named_steps = HashMap::new();
    named_steps.insert(
        "body".to_string(),
        KernelHandle {
            kernel: KernelId::Manifold,
            id: seeded_solid,
        },
    );

    // `@face("side")` — "side" is the label `seed_primitive_attributes`
    // implicitly assigns every sphere face (Role::Side, local_index 0 for
    // the lone staged face), and "side" round-trips through
    // `cap_kind_translation` to `(Role::Side, 0)`.
    let expr = CompiledExpr::ad_hoc_selector(
        CompiledExpr::literal(Value::String("body".to_string()), Type::String),
        SelectorKind::Face,
        vec![CompiledExpr::literal(
            Value::String("side".to_string()),
            Type::String,
        )],
    );

    let mut selector_diagnostics = Vec::new();
    let result = try_eval_ad_hoc_selector(
        &expr,
        &named_steps,
        &mut selector_kernel,
        table,
        SourceSpan::empty(0),
        &mut selector_diagnostics,
    );

    assert!(
        matches!(result, Some(Value::Frame { .. })),
        "expected @face(\"side\") against the manifold body to resolve to \
         Some(Value::Frame {{ .. }}) — got {result:?} (diagnostics: \
         {selector_diagnostics:?}). A non-Frame result means \
         resolve_unique_by_attribute's lookups are still scoped to the \
         interim KernelId::Occt (step-2) instead of the manifold body's \
         actual KernelId::Manifold scope (step-6)."
    );
}
