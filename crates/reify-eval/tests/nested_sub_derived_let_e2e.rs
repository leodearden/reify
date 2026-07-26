//! Task 5360 — instance-nested sub-component elaboration.
//!
//! A `let` that reads across a sub boundary (`self.<sub>.<member>`) resolves
//! fine at TEMPLATE scope, because the top-level sub-elaboration loop in
//! `engine_eval.rs` elaborates every plain sub of every template. It does NOT
//! resolve at INSTANCE scope: `elaborate_child_instance` elaborates only the
//! child's own params + lets, never recursing into the child template's own
//! `sub_components`. So for `Parent { sub m = Mid() }` where `Mid` itself
//! declares `sub k = Kid()`, the grandchild entity `Parent.m.k` is never
//! materialised and `Parent.m.relay` / `Parent.echo` silently become `Undef`
//! with zero diagnostics.
//!
//! These tests pin the chained cross-sub read at instance scope (t1), the
//! constructor-arg threading through the new nesting recursion (t2), and the
//! never-silent-undef diagnostic for deliberately-unsupported nesting (t3).

#![allow(clippy::mutable_key_type)]

use reify_core::{Diagnostic, Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

/// Tolerance for SI-value comparisons (all fixtures below are exact binary
/// fractions of a metre scaled by small integers, so this is generous).
const EPS: f64 = 1e-12;

/// Assert that `id` is present in `values` and holds a `Scalar` whose
/// `si_value` matches `expected`, with a message naming the cell.
fn assert_scalar_si(values: &reify_ir::ValueMap, entity: &str, member: &str, expected: f64) {
    let id = ValueCellId::new(entity, member);
    let got = values
        .get(&id)
        .unwrap_or_else(|| panic!("cell {entity}.{member} is absent from the values map"));
    match got {
        Value::Scalar { si_value, .. } => assert!(
            (si_value - expected).abs() < EPS,
            "cell {entity}.{member}: expected si_value {expected}, got {si_value}",
        ),
        other => panic!("cell {entity}.{member}: expected Value::Scalar({expected}), got {other:?}"),
    }
}

/// Assert the eval produced no `Severity::Error` diagnostics.
fn assert_no_error_diagnostics(diagnostics: &[Diagnostic], what: &str) {
    let errors: Vec<&Diagnostic> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "{what}: expected no error diagnostics, got: {errors:?}",
    );
}

/// (a) The exact two-level repro from the task description.
///
/// `Parent` reads `self.m.relay`; `Mid` reads `self.k.off`. At template scope
/// `Mid.relay` resolves (20mm) — the achievability basis for this acceptance
/// value. At instance scope the chain must resolve identically: the grandchild
/// instance `Parent.m.k` must exist and carry `off = 20mm`, `Parent.m.relay`
/// must relay it, and `Parent.echo` must echo it — with no diagnostics.
#[test]
fn two_level_chain_resolves() {
    const SOURCE: &str = r#"
structure def Kid {
    param w : Length = 10mm
    let off = w * 2.0
}

structure def Mid {
    sub k = Kid()
    let relay = self.k.off
}

structure def Parent {
    sub m = Mid()
    let echo = self.m.relay
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // Template scope already works today — pinned as the control.
    assert_scalar_si(&result.values, "Mid.k", "off", 0.02);
    assert_scalar_si(&result.values, "Mid", "relay", 0.02);

    // Instance scope: the grandchild entity must be elaborated, and both
    // cross-sub reads above it must resolve to the same 20mm.
    assert_scalar_si(&result.values, "Parent.m.k", "off", 0.02);
    assert_scalar_si(&result.values, "Parent.m", "relay", 0.02);
    assert_scalar_si(&result.values, "Parent", "echo", 0.02);

    assert_no_error_diagnostics(&result.diagnostics, "two_level_chain_resolves");
}

/// (b) The three-level (DriveTendons-shaped) chain: a single source value
/// relayed up through three nested sub boundaries.
///
/// Depth-general behaviour is the point — the fix must recurse, not
/// special-case one extra level. `L3.z.y.x` is the deepest instance entity;
/// `L3.d` is the top of the chain.
#[test]
fn three_level_chain_resolves() {
    const SOURCE: &str = r#"
structure def L0 {
    param w : Length = 10mm
    let a = w * 2.0
}

structure def L1 {
    sub x = L0()
    let b = self.x.a
}

structure def L2 {
    sub y = L1()
    let c = self.y.b
}

structure def L3 {
    sub z = L2()
    let d = self.z.c
}
"#;
    let compiled = parse_and_compile_with_stdlib(SOURCE);
    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    // One level already works today — the control that isolates the defect
    // to depth >= 2.
    assert_scalar_si(&result.values, "L1", "b", 0.02);

    // The deepest instance entity under L3 must exist, and every relay hop
    // above it must carry the same derived 20mm.
    assert_scalar_si(&result.values, "L3.z.y.x", "a", 0.02);
    assert_scalar_si(&result.values, "L3.z.y", "b", 0.02);
    assert_scalar_si(&result.values, "L3.z", "c", 0.02);
    assert_scalar_si(&result.values, "L3", "d", 0.02);

    assert_no_error_diagnostics(&result.diagnostics, "three_level_chain_resolves");
}
