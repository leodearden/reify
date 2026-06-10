//! Integration gate for `assert_dag_complete` (task 4355 β).
//!
//! Exercises five §6 boundary-test idioms through compile→build to confirm:
//!   (a) the gate does NOT panic on well-formed source (no false positives), AND
//!   (b) the build emits no Error-severity diagnostics.
//!
//! Because the wire-in lives in `Engine::build`, every test here exercises the
//! gate automatically once step-6 activates it. Until step-6, the same tests
//! pass trivially (no gate → no panic) which is the correct "no false-positive"
//! signal.

use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;
use reify_ir::ExportFormat;
use reify_test_support::{MockGeometryKernel, compile_source};

/// Helper: compile source, build with MockGeometryKernel, assert no panic
/// and no Error diagnostics.
fn assert_gate_clean(source: &str) {
    let compiled = compile_source(source);

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

    let kernel = MockGeometryKernel::new();
    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));

    // This call must NOT panic (the gate must not fire on valid source).
    let result = engine.build(&compiled, ExportFormat::Step);

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
}

/// §6 idiom 1 — cross-sub assembly.
///
/// Inner has `body = box(...)`. Outer has `sub inner = Inner()` and
/// `placed = translate(self.inner.body, ...)`.
/// Edge: Outer's realization reads Inner's realization (via GeomRef::Sub).
#[test]
fn gate_cross_sub_assembly_no_false_positive() {
    assert_gate_clean(
        r#"pub structure Inner {
    let body = box(10mm, 20mm, 30mm)
}
pub structure Outer {
    sub inner = Inner()
    let placed = translate(self.inner.body, 10mm, 0mm, 0mm)
}"#,
    );
}

/// §6 idiom 2 — Boolean over two realizations.
///
/// Entity A has `part = box(...)`. Entity B has `part = box(...)`.
/// Entity C has `result = union(self.a.part, self.b.part)`.
/// Both a.part and b.part are realization producers; result is the consumer.
#[test]
fn gate_boolean_over_two_realizations_no_false_positive() {
    assert_gate_clean(
        r#"pub structure A {
    let part = box(10mm, 10mm, 10mm)
}
pub structure B {
    let part = box(5mm, 5mm, 5mm)
}
pub structure C {
    sub a = A()
    sub b = B()
    let result = union(self.a.part, self.b.part)
}"#,
    );
}

/// §6 idiom 3 — selector→op chain: fillet(body, edges(body), r).
///
/// A geometry selector `edges(body)` reads the `body` realization; the
/// `fillet` op then consumes that selection. Both the selector read and
/// the fillet op must have `body` as a producer.
#[test]
fn gate_selector_to_op_chain_no_false_positive() {
    assert_gate_clean(
        r#"structure S {
    param r: Length = 1mm
    let body = box(20mm, 20mm, 20mm)
    let filleted = fillet(body, edges(body), r)
}"#,
    );
}

/// §6 idiom 4 — geometry-reading constraint.
///
/// `bounding_box(part)` reads the `part` realization and returns a box
/// that the constraint checks against a build-volume bound.
/// The constraint's trace must list the `part` realization in
/// `realization_reads`.
#[test]
fn gate_geometry_reading_constraint_no_false_positive() {
    assert_gate_clean(
        r#"structure S {
    param max_size: Length = 200mm
    let part = box(50mm, 50mm, 50mm)
    constraint fits: bounding_box(part).x <= max_size
}"#,
    );
}

/// §6 idiom 5 — Modify/Transform target is a cross-sub realization.
///
/// `placed = translate(self.inner.body, ...)` — the Transform target is
/// a GeomRef::Sub that resolves to Inner's realization. The gate must
/// record that edge and not fire for the (correct) declaration order.
#[test]
fn gate_transform_of_cross_sub_realization_no_false_positive() {
    assert_gate_clean(
        r#"pub structure Part {
    let body = box(10mm, 10mm, 10mm)
}
pub structure Assembly {
    sub p = Part()
    let moved = translate(self.p.body, 5mm, 0mm, 0mm)
}"#,
    );
}
