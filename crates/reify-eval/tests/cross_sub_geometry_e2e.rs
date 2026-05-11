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
