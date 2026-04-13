//! Tests for imperial unit declarations in stdlib/units.ri (task 335).
//!
//! Covers: yd (Length), lbf (Force), psi/ksi (Pressure), fl_oz/gal (Volume),
//! plus regression guards for the four pre-existing imperial units
//! (ft, thou, lb, oz) and a cross-unit arithmetic check (lbf * mm → Energy).

mod common;

use common::compile_with_stdlib_helper;
use reify_compiler::stdlib_loader;
use reify_types::{DimensionVector, Severity};

// ─── local helper ─────────────────────────────────────────────────────────────

/// Compile a structure with a single default-valued param and return the
/// Scalar's (si_value, dimension) from its default expression.
///
/// Source compiled: `structure def S { param x : <param_type> = <literal> }`
fn stdlib_param_si_value(param_type: &str, literal: &str) -> (f64, DimensionVector) {
    let source = format!(
        "structure def S {{ param x : {} = {} }}",
        param_type, literal
    );
    let module = compile_with_stdlib_helper(&source);
    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "source `{}` produced errors: {:?}",
        source,
        errs
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template not found");
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "x")
        .expect("x cell not found");
    let expr = cell.default_expr.as_ref().expect("x has no default_expr");
    if let reify_types::CompiledExprKind::Literal(reify_types::Value::Scalar {
        si_value,
        dimension,
        ..
    }) = &expr.kind
    {
        (*si_value, *dimension)
    } else {
        panic!("unexpected expression kind: {:?}", expr.kind);
    }
}

// ─── step-1/2: yd — Length ────────────────────────────────────────────────────

#[test]
fn stdlib_yd_resolves_to_length_0p9144() {
    let (v, d) = stdlib_param_si_value("Length", "1yd");
    assert!(
        (v - 0.9144).abs() < 1e-12,
        "1yd should be 0.9144 m, got {}",
        v
    );
    assert_eq!(d, DimensionVector::LENGTH);
}

#[test]
fn stdlib_units_module_contains_yd_with_no_offset() {
    let modules = stdlib_loader::load_stdlib();
    let units_module = modules
        .iter()
        .find(|m| format!("{}", m.path) == "std/units")
        .expect("std/units module not found");

    let yd = units_module
        .units
        .iter()
        .find(|u| u.name == "yd")
        .expect("unit 'yd' not found in std/units");

    assert_eq!(yd.dimension, DimensionVector::LENGTH, "yd dimension wrong");
    assert!(
        (yd.factor - 0.9144).abs() < 1e-12,
        "yd factor should be 0.9144, got {}",
        yd.factor
    );
    assert!(yd.offset.is_none(), "yd should have no offset");
}

// ─── step-3/4: lbf — Force ────────────────────────────────────────────────────

/// lbf = 0.45359237 kg × 9.80665 m/s² = 4.4482216152605 N (exact)
const LBF_SI: f64 = 4.4482216152605;

#[test]
fn stdlib_lbf_resolves_to_force_4p4482216152605() {
    let (v, d) = stdlib_param_si_value("Force", "1lbf");
    assert!(
        (v - LBF_SI).abs() < 1e-9,
        "1lbf should be {} N, got {}",
        LBF_SI,
        v
    );
    assert_eq!(d, DimensionVector::FORCE);
}

#[test]
fn stdlib_units_module_contains_lbf_with_force_dimension() {
    let modules = stdlib_loader::load_stdlib();
    let units_module = modules
        .iter()
        .find(|m| format!("{}", m.path) == "std/units")
        .expect("std/units module not found");

    let lbf = units_module
        .units
        .iter()
        .find(|u| u.name == "lbf")
        .expect("unit 'lbf' not found in std/units");

    assert_eq!(lbf.dimension, DimensionVector::FORCE, "lbf dimension wrong");
    assert!(
        (lbf.factor - LBF_SI).abs() < 1e-9,
        "lbf factor should be {}, got {}",
        LBF_SI,
        lbf.factor
    );
    assert!(lbf.offset.is_none(), "lbf should have no offset");
}
