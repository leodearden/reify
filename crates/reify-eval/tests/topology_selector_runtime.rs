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
use reify_core::ty::SelectorKind;
use reify_core::{DimensionVector, ModulePath, Severity, ValueCellId};
use reify_eval::Engine;
use reify_ir::value::{LeafQuery, SelectorNode};
use reify_ir::{ExportFormat, GeometryHandleId, Value};
use reify_test_support::MockGeometryKernel;

/// Assert a cell holds a kernel-free `Value::Selector` leaf (task 4118 γ): the
/// 7 predicate/all selector constructors now evaluate to a typed
/// `Value::Selector(kind)` instead of a `Value::List<GeometryHandle>`. The
/// `Selector → List<Geometry>` bridge is the compiler-inserted `ResolveSelector`
/// coercion node, resolved by `topology_selectors::resolve()`.
///
/// Construction is KERNEL-FREE (K2/BT7): these callers build with an UNSTAGED
/// mock kernel (no `with_extracted_*` / predicate-query staging), so the only
/// way the cell can be the correct typed `Value::Selector` with the right leaf
/// target + query is if the post-process packaged the leaf without issuing a
/// single kernel query.
fn assert_selector_leaf(
    cell_value: Option<&Value>,
    label: &str,
    kind: SelectorKind,
    target: GeometryHandleId,
    check_query: impl FnOnce(&LeafQuery),
) {
    let sv = match cell_value {
        Some(Value::Selector(sv)) => sv,
        other => panic!("{label} must be Value::Selector(_), got {other:?}"),
    };
    assert_eq!(sv.kind, kind, "{label}: selector kind");
    match &sv.node {
        SelectorNode::Leaf { target: t, query } => {
            assert_eq!(
                t.kernel_handle, target,
                "{label}: leaf target must be the parent solid handle"
            );
            check_query(query);
        }
        other => panic!("{label} must be a Leaf selector node, got {other:?}"),
    }
}

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
            reify_ir::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
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
            reify_ir::DEFAULT_POINT_ON_SHAPE_TOLERANCE_M,
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
/// must construct a typed `Value::Selector(Edge)` whose leaf is `All` over the
/// parent box handle (task 4118 γ) — NOT an eagerly-resolved `Value::List`.
/// Construction is kernel-FREE: the build uses an UNSTAGED mock kernel, so a
/// correct typed selector proves zero kernel queries were issued (K2/BT7).
#[test]
fn edges_let_constructs_typed_edge_all_selector() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let es = edges(body)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| k);

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "es");
    assert_selector_leaf(
        result.values.get(&cell),
        "Bracket.es",
        SelectorKind::Edge,
        GeometryHandleId(1),
        |q| assert_eq!(*q, LeafQuery::All, "edges(body) → All leaf"),
    );
}

/// `let fs = faces(body)` on a structure containing `let body = box(10mm, 10mm, 10mm)`
/// must construct a typed `Value::Selector(Face)` whose leaf is `All` over the
/// parent box handle (task 4118 γ) — NOT an eagerly-resolved `Value::List`.
/// Construction is kernel-FREE: the build uses an UNSTAGED mock kernel (K2/BT7).
#[test]
fn faces_let_constructs_typed_face_all_selector() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let fs = faces(body)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| k);

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "fs");
    assert_selector_leaf(
        result.values.get(&cell),
        "Bracket.fs",
        SelectorKind::Face,
        GeometryHandleId(1),
        |q| assert_eq!(*q, LeafQuery::All, "faces(body) → All leaf"),
    );
}

/// `let com = center_of_mass(body, density)` on a structure containing
/// `let body = box(10mm, 10mm, 10mm)` and `let density = 7850kg/m^3` must
/// resolve to `Value::Point(vec![length(0), length(0), length(0)])` when the
/// mock kernel pre-stages a JSON-Point3 reply for `CenterOfMass(handle=1,
/// density=7850.0)`. Pins the JSON-decode → `Value::Point<Length>` round-trip
/// for the physical-property selector (density routed via `resolve_density_arg`
/// + `accept_arg`, Contract A task 4486 γ).
#[test]
fn center_of_mass_let_resolves_to_point3_length_via_kernel_reply() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let density = 7850kg/m^3\n    \
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
/// `let body = box(50mm, 30mm, 10mm)` and `let density = 7850kg/m^3` must
/// resolve to a rank-2 `Value::Tensor` (3 rows × 3 cols) of MomentOfInertia-
/// dimensioned scalars when the mock kernel pre-stages the OCCT row-of-row
/// `Value::List` reply for `InertiaTensor(handle=1, density=7850.0)`. Pins the
/// raw-Real-rows → nested-Tensor-of-MI-Scalars re-wrap (the eval-side owns the
/// dimension tagging; the kernel reply is dimensionless `Value::Real`).
#[test]
fn moment_of_inertia_let_resolves_to_rank2_tensor_via_kernel_reply() {
    let source = "structure def Bracket {\n    \
        let body = box(50mm, 30mm, 10mm)\n    \
        let density = 7850kg/m^3\n    \
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

/// `let es = edges_by_length(body, r)` with `let r = 0mm..50mm` must construct
/// a typed `Value::Selector(Edge)` whose leaf is `ByLength { 0..0.05 m }` over
/// the parent box handle (task 4118 γ) — NOT an eagerly-filtered `Value::List`.
/// Construction is kernel-FREE: the build uses an UNSTAGED mock kernel, so a
/// correct typed selector proves zero kernel queries were issued (K2/BT7).
#[test]
fn edges_by_length_let_constructs_typed_edge_by_length_selector() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let r = 0mm..50mm\n    \
        let es = edges_by_length(body, r)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| k);

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "es");
    assert_selector_leaf(
        result.values.get(&cell),
        "Bracket.es",
        SelectorKind::Edge,
        GeometryHandleId(1),
        |q| match q {
            LeafQuery::ByLength { min_m, max_m } => {
                assert!((*min_m - 0.0).abs() < 1e-12, "min_m must be 0m, got {min_m}");
                assert!((*max_m - 0.05).abs() < 1e-9, "max_m must be 0.05m, got {max_m}");
            }
            other => panic!("edges_by_length → ByLength leaf, got {other:?}"),
        },
    );
}

/// `let fs = faces_by_area(body, r)` with `let r = 0mm*0mm..1m*1m` must
/// construct a typed `Value::Selector(Face)` whose leaf is `ByArea { 0..1 m² }`
/// over the parent box handle (task 4118 γ) — NOT an eagerly-filtered
/// `Value::List`. Construction is kernel-FREE (UNSTAGED mock kernel, K2/BT7).
#[test]
fn faces_by_area_let_constructs_typed_face_by_area_selector() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let r = 0mm*0mm..1m*1m\n    \
        let fs = faces_by_area(body, r)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| k);

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "fs");
    assert_selector_leaf(
        result.values.get(&cell),
        "Bracket.fs",
        SelectorKind::Face,
        GeometryHandleId(1),
        |q| match q {
            LeafQuery::ByArea { min_m2, max_m2 } => {
                assert!((*min_m2 - 0.0).abs() < 1e-12, "min_m2 must be 0m², got {min_m2}");
                assert!((*max_m2 - 1.0).abs() < 1e-9, "max_m2 must be 1m², got {max_m2}");
            }
            other => panic!("faces_by_area → ByArea leaf, got {other:?}"),
        },
    );
}

/// `let fs = faces_by_normal(body, dir, tol)` with `let dir = vec3(0.0, 0.0, 1.0)`
/// and `let tol = 1deg` must construct a typed `Value::Selector(Face)` whose
/// leaf is `ByNormal { dir: +z, tol: 1° }` over the parent box handle (task
/// 4118 γ) — NOT an eagerly-filtered `Value::List`. Construction is kernel-FREE
/// (UNSTAGED mock kernel, K2/BT7).
#[test]
fn faces_by_normal_let_constructs_typed_face_by_normal_selector() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let dir = vec3(0.0, 0.0, 1.0)\n    \
        let tol = 1deg\n    \
        let fs = faces_by_normal(body, dir, tol)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| k);

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "fs");
    assert_selector_leaf(
        result.values.get(&cell),
        "Bracket.fs",
        SelectorKind::Face,
        GeometryHandleId(1),
        |q| match q {
            LeafQuery::ByNormal { dir, tol_rad } => {
                assert_eq!(*dir, [0.0, 0.0, 1.0], "dir must be +z");
                assert!(
                    (*tol_rad - 1f64.to_radians()).abs() < 1e-9,
                    "tol_rad must be 1°, got {tol_rad}"
                );
            }
            other => panic!("faces_by_normal → ByNormal leaf, got {other:?}"),
        },
    );
}

/// `let es = edges_parallel_to(body, axis, tol)` with `let axis = vec3(0.0, 0.0, 1.0)`
/// and `let tol = 1deg` must construct a typed `Value::Selector(Edge)` whose
/// leaf is `ByParallel { axis: +z, tol: 1° }` over the parent box handle (task
/// 4118 γ) — NOT an eagerly-filtered `Value::List`. Construction is kernel-FREE
/// (UNSTAGED mock kernel, K2/BT7).
#[test]
fn edges_parallel_to_let_constructs_typed_edge_by_parallel_selector() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let axis = vec3(0.0, 0.0, 1.0)\n    \
        let tol = 1deg\n    \
        let es = edges_parallel_to(body, axis, tol)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| k);

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "es");
    assert_selector_leaf(
        result.values.get(&cell),
        "Bracket.es",
        SelectorKind::Edge,
        GeometryHandleId(1),
        |q| match q {
            LeafQuery::ByParallel { axis, tol_rad } => {
                assert_eq!(*axis, [0.0, 0.0, 1.0], "axis must be +z");
                assert!(
                    (*tol_rad - 1f64.to_radians()).abs() < 1e-9,
                    "tol_rad must be 1°, got {tol_rad}"
                );
            }
            other => panic!("edges_parallel_to → ByParallel leaf, got {other:?}"),
        },
    );
}

/// `let es = edges_at_height(body, z, tol)` with `let z = 0mm` and
/// `let tol = 0.01mm` must construct a typed `Value::Selector(Edge)` whose leaf
/// is `ByHeight { z: 0m, tol: 1e-5 m }` over the parent box handle (task 4118
/// γ) — NOT an eagerly-filtered `Value::List`. Construction is kernel-FREE
/// (UNSTAGED mock kernel, K2/BT7).
#[test]
fn edges_at_height_let_constructs_typed_edge_by_height_selector() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let z = 0mm\n    \
        let tol = 0.01mm\n    \
        let es = edges_at_height(body, z, tol)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| k);

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "es");
    assert_selector_leaf(
        result.values.get(&cell),
        "Bracket.es",
        SelectorKind::Edge,
        GeometryHandleId(1),
        |q| match q {
            LeafQuery::ByHeight { z_m, tol_m } => {
                assert!((*z_m - 0.0).abs() < 1e-12, "z_m must be 0m, got {z_m}");
                assert!((*tol_m - 1e-5).abs() < 1e-12, "tol_m must be 1e-5m, got {tol_m}");
            }
            other => panic!("edges_at_height → ByHeight leaf, got {other:?}"),
        },
    );
}

/// Task ε (evaluate-then-accept): the INLINE tolerance form
/// `edges_at_height(body, 2mm + 3mm, 0.1mm)` — with the z argument written as a
/// non-folding arithmetic expression (a `CompiledExprKind::BinOp`, NOT a
/// `Literal`) instead of a `let`-bound cell — must construct the same
/// `ByHeight { z_m: 0.005, tol_m: 1e-4 }` leaf. Before ε the BinOp z-arg hit
/// `resolve_scalar_bound_expr`'s `_ => None` arm → the whole selector fell
/// through → the cell stayed `Value::Undef` → RED. After ε the resolver
/// evaluates `2mm + 3mm` to a `Scalar{LENGTH, 0.005}` → GREEN. Construction is
/// kernel-FREE (UNSTAGED mock kernel, K2/BT7).
#[test]
fn edges_at_height_inline_scalar_expr_constructs_by_height_selector() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let es = edges_at_height(body, 2mm + 3mm, 0.1mm)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| k);

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "es");
    assert_selector_leaf(
        result.values.get(&cell),
        "Bracket.es",
        SelectorKind::Edge,
        GeometryHandleId(1),
        |q| match q {
            LeafQuery::ByHeight { z_m, tol_m } => {
                assert!((*z_m - 0.005).abs() < 1e-9, "z_m must be 5mm (2mm+3mm), got {z_m}");
                assert!((*tol_m - 1e-4).abs() < 1e-12, "tol_m must be 0.1mm, got {tol_m}");
            }
            other => panic!("edges_at_height → ByHeight leaf, got {other:?}"),
        },
    );
}

/// `let neighbors = adjacent_faces(body, body)` must resolve to a
/// `Value::List` of one `Value::GeometryHandle` sub-handle (kernel_handle
/// GHId(1)) via `selector_vocabulary_v2::adjacent_to_face` and
/// `dispatch_filtered_subhandles` (PRD §4 KGQ-κ, task 3619).
///
/// NOTE: the natural fixture is
/// `let top = single(faces_by_normal(body, vec3(0,0,1), 1deg)); adjacent_faces(body, top)`
/// but the selector→list-helper→selector eval-chaining is out of scope
/// (engine_build.rs:3942-3949). This test uses the artificial
/// `adjacent_faces(body, body)` form: the mock stages `body` as its own sole
/// face (`extract_faces(1) = [1]`), so `adjacent_to_face` recovers
/// `face_index = 0` and the `AdjacentFaces` reply `[0]` maps back to handle 1.
/// This exercises the full dispatch wiring (handle→index→query→index→sub-handle)
/// even though the topology is synthetic.
#[test]
fn adjacent_faces_let_resolves_via_selector_vocabulary_v2() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let neighbors = adjacent_faces(body, body)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_extracted_faces(GeometryHandleId(1), vec![GeometryHandleId(1)])
            .with_adjacent_faces_result(GeometryHandleId(1), 0, Value::List(vec![Value::Int(0)]))
    });

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "neighbors");
    let list = match result.values.get(&cell) {
        Some(Value::List(elems)) => elems.clone(),
        other => panic!(
            "Bracket.neighbors must be Value::List of GeometryHandle sub-handles \
             (PRD §4 KGQ-κ), got {:?}",
            other
        ),
    };
    assert_eq!(list.len(), 1, "expected 1 adjacent face sub-handle");
    match &list[0] {
        Value::GeometryHandle { kernel_handle, .. } => {
            assert_eq!(
                *kernel_handle,
                GeometryHandleId(1),
                "neighbors[0] kernel_handle must be GHId(1) (AdjacentFaces index 0 → face handle 1)"
            );
        }
        other => panic!(
            "neighbors[0] must be Value::GeometryHandle, got {:?}",
            other
        ),
    }
}

/// `let es = shared_edges(body, body)` must resolve to a `Value::List` of one
/// `Value::GeometryHandle` sub-handle (kernel_handle GHId(2)) via the full
/// OwnerBody→face-index→SharedEdges→edge-index→dispatch_filtered_subhandles
/// pipeline (PRD §4 KGQ-κ, task 3619).
///
/// NOTE: like `adjacent_faces_let_resolves_via_selector_vocabulary_v2`, the
/// natural fixture would let-bind two face handles (e.g. via `single(faces_by_normal(...))`),
/// but the selector→list-helper→selector eval-chaining is out of scope
/// (engine_build.rs:3942-3949). The artificial `shared_edges(body, body)` form
/// stages `body` as its own owner (OwnerBody(1)=1), its own sole face
/// (extract_faces(1)=[1] so face_index=0), and stages a SharedEdges reply
/// `[0]` that maps back via extract_edges(1)=[2] → sub-handle with GHId(2).
#[test]
fn shared_edges_let_resolves_to_list_via_owner_body_derivation() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let es = shared_edges(body, body)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_owner_body_result(GeometryHandleId(1), GeometryHandleId(1))
            .with_extracted_faces(GeometryHandleId(1), vec![GeometryHandleId(1)])
            .with_extracted_edges(GeometryHandleId(1), vec![GeometryHandleId(2)])
            .with_shared_edges_result(GeometryHandleId(1), 0, 0, Value::List(vec![Value::Int(0)]))
    });

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "es");
    let list = match result.values.get(&cell) {
        Some(Value::List(elems)) => elems.clone(),
        other => panic!(
            "Bracket.es must be Value::List of GeometryHandle sub-handles \
             (PRD §4 KGQ-κ), got {:?}",
            other
        ),
    };
    assert_eq!(list.len(), 1, "expected 1 shared edge sub-handle");
    match &list[0] {
        Value::GeometryHandle { kernel_handle, .. } => {
            assert_eq!(
                *kernel_handle,
                GeometryHandleId(2),
                "es[0] kernel_handle must be GHId(2) (SharedEdges index 0 → edge handle 2)"
            );
        }
        other => panic!("es[0] must be Value::GeometryHandle, got {:?}", other),
    }
}

/// `shared_edges(face_a, face_b)` where the two faces' OwnerBody replies
/// indicate DIFFERENT parent solids must silently degrade to an empty
/// `Value::List` AND emit a warning diagnostic mentioning "different parent
/// solids". This pins the design-doc §4.3 cross-solid guard rail — an
/// unhelpful but well-defined contract that prevents the dispatch from
/// constructing a malformed SharedEdges query against a single shape when the
/// faces span two different shapes.
///
/// Fixture: two distinct boxes (`body_a` = handle 1, `body_b` = handle 2),
/// each declared as its own OwnerBody. `shared_edges(body_a, body_b)` resolves
/// args[0]→1, args[1]→2; OwnerBody(1)=1, OwnerBody(2)=2 → parent_a != parent_b
/// → empty list + warning.
#[test]
fn shared_edges_cross_solid_returns_empty_list_with_warning() {
    let source = "structure def Bracket {\n    \
        let body_a = box(10mm, 10mm, 10mm)\n    \
        let body_b = box(5mm, 5mm, 5mm)\n    \
        let es = shared_edges(body_a, body_b)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| {
        k.with_owner_body_result(GeometryHandleId(1), GeometryHandleId(1))
            .with_owner_body_result(GeometryHandleId(2), GeometryHandleId(2))
    });

    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("Bracket", "es");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::List(vec![])),
        "Bracket.es must silently degrade to an empty Value::List when the two \
         faces have different parent solids, got {:?}",
        result.values.get(&cell),
    );
    assert!(
        result.diagnostics.iter().any(|d| {
            d.severity == Severity::Warning
                && d.message.to_lowercase().contains("different parent solids")
        }),
        "expected a warning diagnostic mentioning 'different parent solids', got {:?}",
        result.diagnostics,
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

/// Tessellate-path parity for the selector cluster under the task-4118 (γ)
/// re-type. Exercises one representative selector — `edges(body)` — via
/// `engine.tessellate_realizations(&compiled)` and asserts the same
/// `Bracket.es == Value::Selector(Edge)` (All leaf) outcome as the build-path
/// test `edges_let_constructs_typed_edge_all_selector`. Pins that all three
/// call sites in `engine_build.rs` (build / build_snapshot / tessellate)
/// consistently package the typed selector through the post-process; without
/// this, a GUI overlay reading `TessellateResult.values` would diverge from a
/// parallel build's overlay. Construction is kernel-FREE (UNSTAGED mock kernel,
/// K2/BT7).
#[test]
fn tessellate_realizations_post_processes_new_topology_selectors() {
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let es = edges(body)\n}";
    let compiled = compile_no_errors(source);
    let mut engine = engine_with_mock_kernel(|k| k);

    let result = engine.tessellate_realizations(&compiled);

    let cell = ValueCellId::new("Bracket", "es");
    assert_selector_leaf(
        result.values.get(&cell),
        "Bracket.es (tessellate path)",
        SelectorKind::Edge,
        GeometryHandleId(1),
        |q| assert_eq!(*q, LeafQuery::All, "edges(body) → All leaf on tessellate path"),
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

/// OCCT-backed end-to-end smoke test for the cluster-A 1-arg list-return
/// selectors `edges` and `faces` (task 3560). Gated by
/// `reify_kernel_occt::OCCT_AVAILABLE` so the file always compiles; the test
/// is a runtime no-op when the OCCT shared lib is absent.
///
/// `box(10mm, 10mm, 10mm)` is the canonical reference solid: 12 edges
/// (4 along each of the three axes) and 6 faces (one per axis-aligned
/// half-space). The OCCT kernel's `extract_edges` / `extract_faces`
/// canonicalises via `TopoDS_Shape::IsSame` so the counts match the
/// well-known topology invariants for a cuboid.
///
/// Confirms the cluster-A dispatch arms (`Edges`, `Faces` in
/// `try_eval_topology_selector`) compose correctly with the real OCCT
/// kernel — the dispatch resolves the geometry-arg ValueRef against the
/// values map (hydrated by `post_process_geometry_handle_cells`), round-trips
/// through `kernel.extract_edges` / `kernel.extract_faces`, and wraps the
/// resulting `Vec<GeometryHandleId>` as `Value::List(Vec<Value::GeometryHandle>)`
/// sub-handles (task 3616). Sibling to
/// `closest_point_on_box_via_occt_returns_plus_x_face_hit` above.
///
/// NOTE on kernel wrapping: the sibling closest_point test wraps the
/// `OcctKernelHandle` in a `SingleKernelHolder` because `closest_point`
/// flows through `GeometryKernel::query` (which SingleKernelHolder
/// forwards). The cluster-A selectors instead call
/// `GeometryKernel::extract_edges` / `extract_faces` directly — and
/// `SingleKernelHolder` does NOT override the trait default for those
/// methods (the default returns
/// `Err(QueryError::QueryFailed("topology extraction not supported by this
/// kernel"))`), which would downgrade the test to `Value::Undef`. So this
/// test passes the boxed `OcctKernelHandle` directly to `Engine::new` —
/// matching how `Engine::with_registered_kernel` boxes the factory output
/// in production. Forwarding extract_edges/faces/vertices through
/// SingleKernelHolder is out-of-scope for task 3560 (would touch
/// reify-geometry/src/lib.rs).
#[test]
fn edges_and_faces_of_box_via_occt_return_canonical_counts() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping edges_and_faces_of_box_via_occt_return_canonical_counts: OCCT not available"
        );
        return;
    }
    let source = "structure def Bracket {\n    \
        let body = box(10mm, 10mm, 10mm)\n    \
        let es = edges(body)\n    \
        let fs = faces(body)\n}";
    let compiled = compile_no_errors(source);

    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel: Box<dyn reify_ir::GeometryKernel> =
        Box::new(reify_kernel_occt::OcctKernelHandle::spawn());
    let mut engine = Engine::new(Box::new(checker), Some(kernel));

    let result = engine.build(&compiled, ExportFormat::Step);

    // Task 4118 (γ): es/fs cells now hold a typed `Value::Selector` (All leaf)
    // built kernel-FREE, not an eagerly-extracted `Value::List`. The end-to-end
    // Selector → List<Geometry> → resolve() path over real OCCT geometry (the
    // canonical 12-edge / 6-face counts) is covered by the step-15 golden
    // (`selector_coercion_golden.rs`) and the kernel_queries_* re-assertions,
    // which exercise the compiler-inserted ResolveSelector coercion node. Here
    // we pin that the OCCT build path packages the typed selector leaf over the
    // realized box handle.
    let es_cell = ValueCellId::new("Bracket", "es");
    assert_selector_leaf(
        result.values.get(&es_cell),
        "Bracket.es (OCCT path)",
        SelectorKind::Edge,
        GeometryHandleId(1),
        |q| assert_eq!(*q, LeafQuery::All, "edges(body) → All leaf (OCCT path)"),
    );

    let fs_cell = ValueCellId::new("Bracket", "fs");
    assert_selector_leaf(
        result.values.get(&fs_cell),
        "Bracket.fs (OCCT path)",
        SelectorKind::Face,
        GeometryHandleId(1),
        |q| assert_eq!(*q, LeafQuery::All, "faces(body) → All leaf (OCCT path)"),
    );
}
