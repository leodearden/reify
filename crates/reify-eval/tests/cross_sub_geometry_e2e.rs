//! End-to-end tests for cross-sub geometry composition (`self.<sub>.<member>`).
//!
//! Exercises the full source-to-build pipeline (parse → compile → eval → build)
//! with a `MockGeometryKernel`, verifying that the parent template's realisations
//! see the child template's named realisation handles through compound-key
//! `named_steps` entries (`"<sub>.<member>"` → handle).
//!
//! See task 3441 — eval-side `GeomRef::Sub` plumbing for cross-template handles.

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{MockGeometryKernel, compile_source};
use reify_types::{ExportFormat, GeometryOp, Severity};

/// Inner has `body = box(...)`; Outer has `sub inner = Inner()` and
/// `placed = translate(self.inner.body, 10mm, 0mm, 0mm)`.
///
/// Asserts (a) build produces `geometry_output.is_some()`, (b) recorded ops
/// contain a Box (Inner.body) and a Translate whose target == Box's result handle,
/// (c) no Error-severity diagnostics, (d) no "unresolvable GeomRef::Sub" error.
///
/// RED until step-4 (eval-side compound-key named_steps threading) lands.
#[test]
fn cross_sub_translate_resolves_child_body_handle() {
    let source = r#"pub structure Inner {
    let body = box(10mm, 20mm, 30mm)
}
pub structure Outer {
    sub inner = Inner()
    let placed = translate(self.inner.body, 10mm, 0mm, 0mm)
}"#;
    let compiled = compile_source(source);

    // (c) No Error-severity diagnostics at compile time.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics; got: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // Build with MockGeometryKernel to capture recorded ops.
    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // (c) No Error-severity diagnostics from build either.
    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "expected no Error diagnostics from build; got: {:?}",
        build_errors
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );

    // (d) Specifically no "unresolvable GeomRef::Sub" — the parent's
    // named_steps must have been seeded with the compound key `inner.body`.
    let unresolvable: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("unresolvable GeomRef::Sub"))
        .collect();
    assert!(
        unresolvable.is_empty(),
        "expected no 'unresolvable GeomRef::Sub' diagnostic; got: {:?}",
        unresolvable
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // (b) The kernel recorded a Box (Inner.body) and a Translate whose
    // target == the Box's handle.
    let recorded = ops_ref.lock().unwrap().clone();
    assert!(
        recorded.len() >= 2,
        "expected at least 2 recorded kernel ops (Box for Inner.body + Translate \
         for Outer.placed), got {}: {:?}",
        recorded.len(),
        recorded
            .iter()
            .map(|r| format!("{:?}", r.op))
            .collect::<Vec<_>>()
    );

    let box_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Box { .. }))
        .expect("expected a Box op recorded for Inner.body");
    let box_handle = box_rec.result_handle;

    let translate_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Translate { .. }))
        .expect("expected a Translate op recorded for Outer.placed");

    match translate_rec.op {
        GeometryOp::Translate { target, .. } => {
            assert_eq!(
                target, box_handle,
                "Translate target should be Inner.body's Box handle ({:?}); got {:?}",
                box_handle, target
            );
        }
        ref other => panic!("expected Translate op, got {:?}", other),
    }

    // (a) Build produces a geometry output.
    assert!(
        result.geometry_output.is_some(),
        "expected geometry_output to be Some, got None; diagnostics: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );
}

/// A has `body = box(...)`, B has `body = cylinder(...)`, C has
/// `sub a = A()`, `sub b = B()`, `combined = union(self.a.body, self.b.body)`.
///
/// Asserts (a) recorded ops contain a Union whose left == A.body's Box handle
/// and right == B.body's Cylinder handle, (b) build succeeds with
/// `geometry_output.is_some()`, (c) no Error-severity diagnostics.
///
/// RED until step-6 (boolean-op arg-resolution wiring) lands.
#[test]
fn cross_sub_union_two_sub_bodies_composes_in_parent() {
    let source = r#"pub structure A {
    let body = box(10mm, 10mm, 10mm)
}
pub structure B {
    let body = cylinder(5mm, 10mm)
}
pub structure C {
    sub a = A()
    sub b = B()
    let combined = union(self.a.body, self.b.body)
}"#;
    let compiled = compile_source(source);

    // (c) No compile-time Error diagnostics.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics; got: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // (c) No Error diagnostics from build.
    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "expected no build-time Error diagnostics; got: {:?}",
        build_errors
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );

    // (a) Recorded ops contain a Box (A.body), Cylinder (B.body), and a
    // Union whose left == Box's handle and right == Cylinder's handle.
    let recorded = ops_ref.lock().unwrap().clone();
    let box_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Box { .. }))
        .expect("expected a Box op recorded for A.body");
    let cyl_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Cylinder { .. }))
        .expect("expected a Cylinder op recorded for B.body");
    let union_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Union { .. }))
        .expect("expected a Union op recorded for C.combined");

    match union_rec.op {
        GeometryOp::Union { left, right } => {
            assert_eq!(
                left, box_rec.result_handle,
                "Union.left should be A.body's Box handle ({:?}); got {:?}",
                box_rec.result_handle, left
            );
            assert_eq!(
                right, cyl_rec.result_handle,
                "Union.right should be B.body's Cylinder handle ({:?}); got {:?}",
                cyl_rec.result_handle, right
            );
        }
        ref other => panic!("expected Union op, got {:?}", other),
    }

    // (b) Build produces a geometry output.
    assert!(
        result.geometry_output.is_some(),
        "expected geometry_output to be Some, got None; diagnostics: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );
}

/// Stage has `body = box(...)`; Assy has `sub stage_left = Stage()` and
/// `mirrored = mirror(self.stage_left.body, 0, 0, 0, 1, 0, 0)` — origin at
/// the world origin, normal along +X.
///
/// Asserts (a) recorded ops contain a Mirror op whose target == Stage's box
/// handle, (b) build succeeds with `geometry_output.is_some()`, (c) no
/// Error-severity diagnostics.
///
/// Locks down the `mirror(self.<sub>.body, ...)` pattern called out in the
/// task description.  Should pass without code change because `mirror` is in
/// `geometry_arg_indices` returning `[0]` (geometry.rs:163), so the cross-sub
/// pre-check in `compile_geometry_call`'s generic resolution loop already
/// fires for the geometry arg at index 0.
#[test]
fn cross_sub_mirror_uses_child_body_handle() {
    let source = r#"pub structure Stage {
    let body = box(50mm, 30mm, 20mm)
}
pub structure Assy {
    sub stage_left = Stage()
    let mirrored = mirror(self.stage_left.body, 0, 0, 0, 1, 0, 0)
}"#;
    let compiled = compile_source(source);

    // (c) No compile-time Error diagnostics.
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "expected no compile-time Error diagnostics; got: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    let checker = SimpleConstraintChecker;
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // (c) No Error diagnostics from build.
    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        build_errors.is_empty(),
        "expected no build-time Error diagnostics; got: {:?}",
        build_errors
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );

    // (a) Recorded ops contain a Box (Stage.body) and a Mirror whose
    // target == the Box's handle.
    let recorded = ops_ref.lock().unwrap().clone();
    let box_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Box { .. }))
        .expect("expected a Box op recorded for Stage.body");
    let box_handle = box_rec.result_handle;

    let mirror_rec = recorded
        .iter()
        .find(|rec| matches!(rec.op, GeometryOp::Mirror { .. }))
        .expect("expected a Mirror op recorded for Assy.mirrored");

    match mirror_rec.op {
        GeometryOp::Mirror { target, .. } => {
            assert_eq!(
                target, box_handle,
                "Mirror.target should be Stage.body's Box handle ({:?}); got {:?}",
                box_handle, target
            );
        }
        ref other => panic!("expected Mirror op, got {:?}", other),
    }

    // (b) Build produces a geometry output.
    assert!(
        result.geometry_output.is_some(),
        "expected geometry_output to be Some, got None; diagnostics: {:?}",
        result
            .diagnostics
            .iter()
            .map(|d| format!("[{:?}] {}", d.severity, d.message))
            .collect::<Vec<_>>()
    );
}
