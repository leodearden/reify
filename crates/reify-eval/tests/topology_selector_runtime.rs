//! End-to-end runtime tests for the topology-selector stdlib helpers
//! `closest_point`, `is_on`, and `angle_between_surfaces` (task 2324).
//!
//! These tests exercise the full pipeline: parse → `compile_with_stdlib` →
//! `Engine::build` (with `MockGeometryKernel`) → assert the resulting
//! `BuildResult.values` carry the kernel-resolved value for the topology-
//! selector `let` bindings.
//!
//! Architecture: the kernel-aware dispatch lives in
//! `crates/reify-eval/src/geometry_ops.rs::try_eval_topology_selector` and is
//! invoked as a post-process from `engine_build.rs`'s build / build_snapshot /
//! tessellate paths after `execute_realization_ops` populates `named_steps`.
//! These tests pin that the post-process correctly patches the resulting
//! `Value::Point(_)` / `Value::Bool(_)` / `Value::Scalar { dimension: ANGLE,
//! .. }` into the `ValueMap` (overwriting the `Value::Undef` left by the pure
//! `eval_expr` path).
//!
//! The mock kernel allocates `GeometryHandleId(1)` for the first `execute`
//! call, so each fixture's `box(10mm, 10mm, 10mm)` resolves to handle id 1
//! and the kernel is pre-configured with `with_*_result(GeometryHandleId(1),
//! …)`. The point-arg `let p = point3(...)` realises in the values map as
//! `Value::Point(vec![Value::length(...), …])` and the dispatcher reads it
//! straight out of `values`.
//!
//! Sibling to `crates/reify-eval/tests/conformance_runtime.rs` (task 2320 —
//! `is_watertight` / `is_manifold` / `is_orientable`) and
//! `crates/reify-eval/tests/mechanism_interference_smoke.rs` (task 2531 —
//! `interferes` / `interferes_with` / `min_clearance`). The structural shape
//! is intentionally identical: a per-helper happy-path mock-kernel test, a
//! per-helper literal-arg-falls-through-to-Undef defensive test, a
//! tessellate-path parity test, and an OCCT-gated end-to-end smoke test.

use reify_compiler::compile_with_stdlib;
use reify_eval::Engine;
use reify_test_support::MockGeometryKernel;
use reify_types::{
    DimensionVector, ExportFormat, GeometryHandleId, ModulePath, Severity, Value, ValueCellId,
};

/// Parse and compile a source string with the stdlib prelude.
/// Asserts the parse and compile pipelines produce no errors.
fn compile_no_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("topology_selector_runtime"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    let compiled = compile_with_stdlib(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:#?}", errors);
    compiled
}

/// Build an `Engine` with the constraint checker and a mock kernel
/// configured by `setup` (typically a chain of `with_*_result` builder
/// calls).  The first `execute` call on the mock kernel allocates
/// `GeometryHandleId(1)`, so any `let body = box(...)` in the fixture
/// resolves to handle id 1 — pre-configure the mock kernel accordingly.
fn engine_with_mock_kernel(setup: impl FnOnce(MockGeometryKernel) -> MockGeometryKernel) -> Engine {
    let kernel = setup(MockGeometryKernel::new());
    let checker = reify_constraints::SimpleConstraintChecker;
    Engine::new(Box::new(checker), Some(Box::new(kernel)))
}

// ── Happy-path mock-kernel tests (one per helper) ───────────────────────────

/// `let cp = closest_point(p, body)` on a structure containing
/// `let p = point3(10mm, 0mm, 0mm)` and `let body = box(10mm, 10mm, 10mm)`
/// must resolve to `Value::Point(vec![length(0.005), length(0.0), length(0.0)])`
/// when the kernel reports a JSON-Point3 reply for `ClosestPointOnShape(handle=1,
/// px=0.01, py=0.0, pz=0.0)`. Pins the JSON-decode → `Value::Point` round-trip
/// end-to-end through the post-process.
#[test]
fn closest_point_let_resolves_to_point3_length_via_kernel_reply() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let p = point3(10mm, 0mm, 0mm)\n    \
        let cp = closest_point(p, body)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_closest_point_on_shape_result(
            GeometryHandleId(1),
            [0.01, 0.0, 0.0],
            // Kernel-side JSON-Point3 encoding (matches `OcctKernel::query()`'s
            // ClosestPointOnShape arm). 0.005 m = 5 mm — closest face hit on
            // the +x face of a 10 mm box centered at origin.
            Value::String("{\"x\":0.005,\"y\":0.0,\"z\":0.0}".to_string()),
        )
    });

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "cp");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::Point(vec![
            Value::length(0.005),
            Value::length(0.0),
            Value::length(0.0),
        ])),
        "Bracket.cp must resolve to Value::Point of three Length scalars via \
         kernel ClosestPointOnShape JSON reply, got {:?}",
        result.values.get(&cell),
    );
}

/// `let is_on_body = is_on(p, body)` on a structure containing
/// `let p = point3(5mm, 0mm, 0mm)` (a point on the +x face of the box)
/// must resolve to `Value::Bool(true)` when the kernel replies `Bool(true)`
/// for `PointOnShape(handle=1, px=0.005, py=0.0, pz=0.0,
/// tolerance=DEFAULT_POINT_ON_SHAPE_TOLERANCE_M)`.
/// Pins the dispatcher's `DEFAULT_POINT_ON_SHAPE_TOLERANCE_M` default — recording
/// the mock under exactly this constant is the contract: if the dispatcher
/// changes the default, the recorded reply would not be served and the
/// cell would stay at `Value::Undef`.
#[test]
fn is_on_let_resolves_to_bool_true_via_kernel_reply_with_default_tolerance() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let p = point3(5mm, 0mm, 0mm)\n    \
        let is_on_body = is_on(p, body)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_point_on_shape_result(
            GeometryHandleId(1),
            [0.005, 0.0, 0.0],
            reify_types::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
            Value::Bool(true),
        )
    });

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "is_on_body");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::Bool(true)),
        "Bracket.is_on_body must resolve to Bool(true) via kernel PointOnShape reply, \
         got {:?}",
        result.values.get(&cell),
    );
}

/// `let ang = angle_between_surfaces(body, body)` on a structure containing
/// `let body = box(10mm, 10mm, 10mm)` must resolve to `Value::angle(PI/2)`
/// when the kernel replies `Real(PI/2)` for `SurfaceAngle(face_a=1, face_b=1)`.
/// v0.1 has no surface-extraction syntax, so passing `body, body` (both
/// resolving to handle id 1) is the only way to exercise the dispatcher
/// through the parsing path — semantic correctness is the OCCT primitive's
/// concern, not the dispatcher's. Pins the dispatcher's `Value::Real(rad)`
/// → `Value::angle(rad)` wrap.
#[test]
fn angle_between_surfaces_let_resolves_to_angle_scalar_via_kernel_reply() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let ang = angle_between_surfaces(body, body)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_surface_angle_result(
            GeometryHandleId(1),
            GeometryHandleId(1),
            Value::Real(std::f64::consts::FRAC_PI_2),
        )
    });

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "ang");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::angle(std::f64::consts::FRAC_PI_2)),
        "Bracket.ang must resolve to Value::angle(PI/2) via kernel SurfaceAngle \
         reply (Real(rad) wrapped to ANGLE-dimensioned Scalar), got {:?}",
        result.values.get(&cell),
    );
}

// ── Defensive literal-arg fall-through tests (one per helper) ───────────────
//
// Mirrors `is_watertight_with_literal_int_arg_falls_through_to_undef` from
// `tests/conformance_runtime.rs:213`. The compile-time result_type wiring
// in `units.rs` / `expr.rs` keys only on the function name so these
// ill-formed call sites compile cleanly; at build time the dispatcher's
// arg-shape guards short-circuit to `None` before any kernel round-trip,
// leaving the cell at the `Value::Undef` left by `eval_expr`.

/// `closest_point(42, body)` — literal int as the point arg. The dispatcher's
/// `resolve_point3_length_arg` must reject the non-`ValueRef` arg before any
/// kernel round-trip. Cell stays at `Value::Undef`.
#[test]
fn closest_point_with_literal_int_arg_falls_through_to_undef() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let cp = closest_point(42, body)\n}";
    let compiled = compile_no_errors(source);
    // Kernel is pre-configured with a result that *would* be served if the
    // dispatcher incorrectly resolved the literal arg — so a non-Undef
    // outcome would surface as a regression rather than a coincidental match.
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_closest_point_on_shape_result(
            GeometryHandleId(1),
            [42.0, 0.0, 0.0],
            Value::String("{\"x\":0.0,\"y\":0.0,\"z\":0.0}".to_string()),
        )
    });

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "cp");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::Undef),
        "Bracket.cp with a literal-int point arg must fall through to Undef, \
         got {:?}",
        result.values.get(&cell),
    );
}

/// `is_on(42, body)` — literal int as the point arg. Same defensive fall-through
/// as `closest_point` above.
#[test]
fn is_on_with_literal_int_arg_falls_through_to_undef() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let is_on_body = is_on(42, body)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_point_on_shape_result(
            GeometryHandleId(1),
            [42.0, 0.0, 0.0],
            reify_types::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
            Value::Bool(true),
        )
    });

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "is_on_body");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::Undef),
        "Bracket.is_on_body with a literal-int point arg must fall through to Undef, \
         got {:?}",
        result.values.get(&cell),
    );
}

/// `angle_between_surfaces(42, body)` — literal int as the first face arg.
/// Defensive fall-through: dispatcher's `resolve_geometry_handle_arg` rejects
/// the non-`ValueRef` arg before any kernel round-trip.
#[test]
fn angle_between_surfaces_with_literal_int_arg_falls_through_to_undef() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let ang = angle_between_surfaces(42, body)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_surface_angle_result(
            GeometryHandleId(1),
            GeometryHandleId(1),
            Value::Real(std::f64::consts::FRAC_PI_2),
        )
    });

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "ang");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::Undef),
        "Bracket.ang with a literal-int face arg must fall through to Undef, \
         got {:?}",
        result.values.get(&cell),
    );
}

// ── Topology-selector-vocabulary v1 (task 3560) ─────────────────────────────
//
// Per-selector happy-path mock-kernel tests covering the 11 §3.9 selector
// names that gained eval-time dispatch in task 3560:
//
//   - 1-arg list returns: edges, faces
//   - 2-arg Range-filtered list returns: edges_by_length, faces_by_area
//   - 3-arg predicate-filtered list returns: faces_by_normal, edges_parallel_to,
//     edges_at_height
//   - 2-arg topology-graph queries: adjacent_faces, shared_edges
//   - 2-arg physical-property returns: center_of_mass, moment_of_inertia
//
// Each test mirrors the existing closest_point/is_on/angle_between_surfaces
// pattern at the top of this file: parse + compile with stdlib, pre-stage a
// MockGeometryKernel via the appropriate `with_*_result` builder, run
// `engine.build`, and assert the cell value the post-process patches in.
//
// Test-fixture convention: non-trivial args use let-bound intermediates
// (e.g. `let dir = vec3(0,0,1); let tol = 1deg; let top = faces_by_normal(body, dir, tol)`)
// so the dispatcher resolves them via the ValueRef path — inline FunctionCall
// args (e.g. `vec3(0,0,1)` written inline) fall through to None per the
// literal-args contract in the dispatcher.

/// `let es = edges(body)` on a structure containing `let body = box(10mm, 10mm, 10mm)`
/// must resolve to `Value::List(vec![Int(2), Int(3), Int(4)])` when the mock
/// kernel pre-stages `extract_edges(GeometryHandleId(1)) = [2, 3, 4]`. Pins the
/// 1-arg list-return shape end-to-end through the dispatch.
#[test]
fn edges_let_resolves_to_list_of_int_via_extract_edges() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let es = edges(body)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_extracted_edges(
            GeometryHandleId(1),
            vec![GeometryHandleId(2), GeometryHandleId(3), GeometryHandleId(4)],
        )
    });

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "es");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::List(vec![
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
        ])),
        "Bracket.es must resolve to Value::List of three Value::Int sub-handles \
         via kernel extract_edges, got {:?}",
        result.values.get(&cell),
    );
}

/// `let fs = faces(body)` on a structure containing `let body = box(10mm, 10mm, 10mm)`
/// must resolve to `Value::List` of six `Value::Int`s when the mock kernel
/// pre-stages `extract_faces(GeometryHandleId(1)) = [10, 11, 12, 13, 14, 15]`
/// (matching a box's six faces). Pins the 1-arg list-return shape for the
/// face variant.
#[test]
fn faces_let_resolves_to_list_of_int_via_extract_faces() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let fs = faces(body)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_extracted_faces(
            GeometryHandleId(1),
            vec![
                GeometryHandleId(10),
                GeometryHandleId(11),
                GeometryHandleId(12),
                GeometryHandleId(13),
                GeometryHandleId(14),
                GeometryHandleId(15),
            ],
        )
    });

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "fs");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::List(vec![
            Value::Int(10),
            Value::Int(11),
            Value::Int(12),
            Value::Int(13),
            Value::Int(14),
            Value::Int(15),
        ])),
        "Bracket.fs must resolve to Value::List of six Value::Int sub-handles \
         via kernel extract_faces, got {:?}",
        result.values.get(&cell),
    );
}

/// `let com = center_of_mass(body, density)` on a structure containing
/// `let body = box(10mm, 10mm, 10mm)` and `let density = 7850.0` must resolve
/// to `Value::Point(vec![length(0), length(0), length(0)])` when the mock
/// kernel pre-stages a JSON-Point3 reply for `CenterOfMass(handle=1,
/// density=7850.0)`. Pins the JSON-decode → `Value::Point<Length>` round-trip
/// for the physical-property selector (density routed via the new
/// `resolve_real_scalar_arg`).
#[test]
fn center_of_mass_let_resolves_to_point3_length_via_kernel_reply() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let density = 7850.0\n    \
        let com = center_of_mass(body, density)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_center_of_mass_result(
            GeometryHandleId(1),
            7850.0,
            Value::String("{\"x\":0.0,\"y\":0.0,\"z\":0.0}".to_string()),
        )
    });

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "com");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::Point(vec![
            Value::length(0.0),
            Value::length(0.0),
            Value::length(0.0),
        ])),
        "Bracket.com must resolve to Value::Point of three Length scalars via \
         kernel CenterOfMass JSON reply, got {:?}",
        result.values.get(&cell),
    );
}

/// `let i = moment_of_inertia(body, density)` on a structure containing
/// `let body = box(50mm, 30mm, 10mm)` and `let density = 7850.0` must resolve
/// to a rank-2 `Value::Tensor` (3 rows × 3 cols) of MomentOfInertia-dimensioned
/// scalars when the mock kernel pre-stages the OCCT row-of-row `Value::List`
/// reply for `InertiaTensor(handle=1, density=7850.0)`. Pins the
/// raw-Real-rows → nested-Tensor-of-MI-Scalars re-wrap (the eval-side owns the
/// dimension tagging; the kernel reply is dimensionless `Value::Real`).
#[test]
fn moment_of_inertia_let_resolves_to_rank2_tensor_via_kernel_reply() {
    let source = "structure def Bracket {\n    \
        let body = box(50mm, 30mm, 10mm)\n    \
        let density = 7850.0\n    \
        let i = moment_of_inertia(body, density)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_inertia_tensor_result(
            GeometryHandleId(1),
            7850.0,
            Value::List(vec![
                Value::List(vec![Value::Real(1.0), Value::Real(0.0), Value::Real(0.0)]),
                Value::List(vec![Value::Real(0.0), Value::Real(2.0), Value::Real(0.0)]),
                Value::List(vec![Value::Real(0.0), Value::Real(0.0), Value::Real(3.0)]),
            ]),
        )
    });

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "i");
    let mi = |v: f64| Value::Scalar {
        si_value: v,
        dimension: DimensionVector::MOMENT_OF_INERTIA,
    };
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::Tensor(vec![
            Value::Tensor(vec![mi(1.0), mi(0.0), mi(0.0)]),
            Value::Tensor(vec![mi(0.0), mi(2.0), mi(0.0)]),
            Value::Tensor(vec![mi(0.0), mi(0.0), mi(3.0)]),
        ])),
        "Bracket.i must resolve to a rank-2 Value::Tensor (3×3) of \
         MomentOfInertia-dimensioned scalars via kernel InertiaTensor reply, \
         got {:?}",
        result.values.get(&cell),
    );
}

/// `let es = edges_by_length(body, r)` with `let r = 0mm..50mm` must resolve
/// to the filtered `Value::List` of edge sub-handles whose `EdgeLength` falls
/// in `[0, 0.05] m`. Both staged edges (10 mm and 20 mm) are within range, so
/// both survive. Pins the Range-arg resolution + delegation to
/// `topology_selectors::edges_by_length`.
#[test]
fn edges_by_length_let_resolves_to_filtered_list_via_helper() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let r = 0mm..50mm\n    \
        let es = edges_by_length(body, r)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_extracted_edges(
            GeometryHandleId(1),
            vec![GeometryHandleId(2), GeometryHandleId(3)],
        )
        .with_edge_length_result(GeometryHandleId(2), Value::Real(0.010))
        .with_edge_length_result(GeometryHandleId(3), Value::Real(0.020))
    });

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "es");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::List(vec![Value::Int(2), Value::Int(3)])),
        "Bracket.es must resolve to the length-filtered Value::List (both \
         edges within [0, 50] mm) via topology_selectors::edges_by_length, \
         got {:?}",
        result.values.get(&cell),
    );
}

/// `let fs = faces_by_area(body, r)` with `let r = 0mm*0mm..1m*1m` must
/// resolve to the area-filtered `Value::List`. The single staged face
/// (0.0001 m²) is within `[0, 1] m²`, so it survives. Pins the Area-Range
/// resolution + delegation to `topology_selectors::faces_by_area`.
#[test]
fn faces_by_area_let_resolves_to_filtered_list_via_helper() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let r = 0mm*0mm..1m*1m\n    \
        let fs = faces_by_area(body, r)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_extracted_faces(GeometryHandleId(1), vec![GeometryHandleId(2)])
            .with_surface_area_result(GeometryHandleId(2), Value::Real(0.0001))
    });

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "fs");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::List(vec![Value::Int(2)])),
        "Bracket.fs must resolve to the area-filtered Value::List via \
         topology_selectors::faces_by_area, got {:?}",
        result.values.get(&cell),
    );
}

/// `let fs = faces_by_normal(body, dir, tol)` with `let dir = vec3(0.0, 0.0, 1.0)`
/// and `let tol = 1deg` must resolve to the normal-filtered `Value::List`. The
/// single staged face's normal is exactly `+z` (matching `dir`), so it survives
/// the 1° tolerance. Pins the Vec3-arg + angle-arg resolution + delegation to
/// `topology_selectors::faces_by_normal`.
#[test]
fn faces_by_normal_let_resolves_to_filtered_list_via_helper() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let dir = vec3(0.0, 0.0, 1.0)\n    \
        let tol = 1deg\n    \
        let fs = faces_by_normal(body, dir, tol)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_extracted_faces(GeometryHandleId(1), vec![GeometryHandleId(2)])
            .with_face_normal_result(
                GeometryHandleId(2),
                Value::String("{\"x\":0.0,\"y\":0.0,\"z\":1.0}".to_string()),
            )
    });

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "fs");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::List(vec![Value::Int(2)])),
        "Bracket.fs must resolve to the normal-filtered Value::List via \
         topology_selectors::faces_by_normal, got {:?}",
        result.values.get(&cell),
    );
}

/// `let es = edges_parallel_to(body, axis, tol)` with `let axis = vec3(0.0, 0.0, 1.0)`
/// and `let tol = 1deg` must resolve to the tangent-filtered `Value::List`. The
/// single staged edge's tangent is `+z` (parallel to `axis`), so it survives
/// the 1° tolerance. Pins the Vec3-arg + angle-arg resolution + delegation to
/// `topology_selectors::edges_parallel_to` (sign-tolerant tangent predicate).
#[test]
fn edges_parallel_to_let_resolves_to_filtered_list_via_helper() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let axis = vec3(0.0, 0.0, 1.0)\n    \
        let tol = 1deg\n    \
        let es = edges_parallel_to(body, axis, tol)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_extracted_edges(GeometryHandleId(1), vec![GeometryHandleId(2)])
            .with_edge_tangent_result(
                GeometryHandleId(2),
                Value::String("{\"x\":0.0,\"y\":0.0,\"z\":1.0}".to_string()),
            )
    });

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "es");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::List(vec![Value::Int(2)])),
        "Bracket.es must resolve to the tangent-filtered Value::List via \
         topology_selectors::edges_parallel_to, got {:?}",
        result.values.get(&cell),
    );
}

/// `let es = edges_at_height(body, z, tol)` with `let z = 0mm` and
/// `let tol = 0.01mm` must resolve to the height-filtered `Value::List`. The
/// single staged edge's bbox z-extent is `[0, 0]` (exactly on the `z = 0`
/// plane), within the 0.01 mm tolerance, so it survives. Pins the
/// Length-scalar-arg resolution + delegation to
/// `topology_selectors::edges_at_height`.
#[test]
fn edges_at_height_let_resolves_to_filtered_list_via_helper() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let z = 0mm\n    \
        let tol = 0.01mm\n    \
        let es = edges_at_height(body, z, tol)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_extracted_edges(GeometryHandleId(1), vec![GeometryHandleId(2)])
            .with_bbox_result(
                GeometryHandleId(2),
                Value::String(
                    "{\"xmin\":-0.005,\"ymin\":-0.005,\"zmin\":0.0,\
                      \"xmax\":0.005,\"ymax\":0.005,\"zmax\":0.0}"
                        .to_string(),
                ),
            )
    });

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "es");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::List(vec![Value::Int(2)])),
        "Bracket.es must resolve to the height-filtered Value::List via \
         topology_selectors::edges_at_height, got {:?}",
        result.values.get(&cell),
    );
}

/// `let neighbors = adjacent_faces(body, body)` must resolve to the
/// `Value::List` of face sub-handles adjacent to the given face, via
/// `selector_vocabulary_v2::adjacent_to_face`.
///
/// NOTE: the natural fixture is
/// `let top = single(faces_by_normal(body, vec3(0,0,1), 1deg)); adjacent_faces(body, top)`
/// but `single` is out of scope (task #2698) and `Type::Geometry` face cells
/// are not directly representable, so this test uses the artificial
/// `adjacent_faces(body, body)` form: the mock stages `body` as its own sole
/// face (`extract_faces(1) = [1]`), so `adjacent_to_face` recovers
/// `face_index = 0` and the `AdjacentFaces` reply `[0]` maps back to handle 1.
/// This exercises the full dispatch wiring (handle→index→query→index→handle)
/// even though the topology is synthetic.
#[test]
fn adjacent_faces_let_resolves_via_selector_vocabulary_v2() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let neighbors = adjacent_faces(body, body)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_extracted_faces(GeometryHandleId(1), vec![GeometryHandleId(1)])
            .with_adjacent_faces_result(
                GeometryHandleId(1),
                0,
                Value::List(vec![Value::Int(0)]),
            )
    });

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "neighbors");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::List(vec![Value::Int(1)])),
        "Bracket.neighbors must resolve to the adjacency Value::List via \
         selector_vocabulary_v2::adjacent_to_face (AdjacentFaces index 0 → \
         handle 1), got {:?}",
        result.values.get(&cell),
    );
}

// ── Tessellate-path parity test ─────────────────────────────────────────────

/// The post-process must run on the `tessellate_realizations` path too, so
/// `TessellateResult.values` exposes the same kernel-resolved value as
/// `BuildResult.values` for topology-selector cells. Without this wiring, a
/// GUI overlay that reads `TessellateResult.values` to display selector
/// results next to a mesh would see `Value::Undef` while a parallel build
/// path's overlay would see `Value::Point(_)`. Sibling to
/// `tessellate_realizations_post_processes_conformance_queries` from
/// `conformance_runtime.rs:247`.
#[test]
fn tessellate_realizations_post_processes_topology_selectors() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let p = point3(10mm, 0mm, 0mm)\n    \
        let cp = closest_point(p, body)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_closest_point_on_shape_result(
            GeometryHandleId(1),
            [0.01, 0.0, 0.0],
            Value::String("{\"x\":0.005,\"y\":0.0,\"z\":0.0}".to_string()),
        )
    });

    let result = engine.tessellate_realizations(&compiled);

    let cell = ValueCellId::new("Bracket", "cp");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::Point(vec![
            Value::length(0.005),
            Value::length(0.0),
            Value::length(0.0),
        ])),
        "TessellateResult.values must expose the kernel-resolved Point3-Length \
         for closest_point cells (parity with BuildResult.values), got {:?}",
        result.values.get(&cell),
    );
}

// ── OCCT-gated end-to-end smoke test ────────────────────────────────────────

/// OCCT-backed end-to-end smoke test for the topology-selector dispatch
/// surface. Gated by `reify_kernel_occt::OCCT_AVAILABLE` so the file always
/// compiles; the test is a runtime no-op when the OCCT shared lib is absent.
///
/// `box(10mm, 10mm, 10mm)` produces a 10 mm box centered at origin
/// (`make_box` constructs at corner `(-5 mm, -5 mm, -5 mm)` per the OCCT
/// wrapper). The closest point on this box's surface to `(10 mm, 0, 0)` is
/// `(5 mm, 0, 0)` — on the +x face. We assert the resulting `Value::Point`
/// matches within 1 µm tolerance to absorb floating-point noise.
///
/// Confirms `try_eval_topology_selector` composes correctly with the real
/// OCCT kernel — the dispatch resolves the geometry-arg ValueRef against the
/// realisation's named-step handle map, round-trips
/// `GeometryQuery::ClosestPointOnShape` through OCCT, parses the JSON-Point3
/// reply, and patches the resulting `Value::Point` into the cell.
#[test]
fn closest_point_on_box_via_occt_returns_plus_x_face_hit() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping closest_point_on_box_via_occt_returns_plus_x_face_hit: OCCT not available"
        );
        return;
    }
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let p = point3(10mm, 0mm, 0mm)\n    \
        let cp = closest_point(p, body)\n}";
    let compiled = compile_no_errors(source);

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(planner)));

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "cp");
    let value = result
        .values
        .get(&cell)
        .unwrap_or_else(|| panic!("Bracket.cp must be present in BuildResult.values"));
    let components = match value {
        Value::Point(items) => items,
        other => panic!(
            "Bracket.cp must be Value::Point(...) via OCCT closest_point, got {:?}",
            other
        ),
    };
    assert_eq!(
        components.len(),
        3,
        "Bracket.cp must be a Point3 (three components), got {} components",
        components.len()
    );

    let expected = [0.005_f64, 0.0, 0.0];
    let names = ["x", "y", "z"];
    for (i, component) in components.iter().enumerate() {
        let si = match component {
            Value::Scalar { si_value, .. } => *si_value,
            other => panic!(
                "Bracket.cp.{} must be Length-dimensioned Scalar, got {:?}",
                names[i], other
            ),
        };
        let diff = (si - expected[i]).abs();
        assert!(
            diff < 1e-6,
            "Bracket.cp.{} must be within 1 µm of {} m via OCCT, got {} m \
             (diff = {})",
            names[i],
            expected[i],
            si,
            diff
        );
    }
}
