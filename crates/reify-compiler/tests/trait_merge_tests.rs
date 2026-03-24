//! Trait member merging compilation tests.
//!
//! Tests for merging members when a structure implements multiple traits:
//! let default deduplication, expression conflict detection, and
//! cross-trait requirement satisfaction by defaults.

use reify_compiler::*;
use reify_types::*;

/// Helper: parse source and compile, returning the CompiledModule.
fn compile_module(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

/// Helper: parse source and compile, returning first template + diagnostics.
fn compile_first_template(source: &str) -> (TopologyTemplate, Vec<Diagnostic>) {
    let module = compile_module(source);
    let template = module.templates.into_iter().next().expect("expected 1 template");
    (template, module.diagnostics)
}

/// Step 1a: Two traits each providing `let area : Real = width * height`.
/// Structure implements both — identical let defaults should be merged (dedup).
/// Expect 0 errors and exactly 1 'area' value cell.
#[test]
fn let_defaults_same_name_same_expr_merge() {
    let source = r#"
trait HasArea {
    let area : Real = width * height
}

trait AlsoHasArea {
    let area : Real = width * height
}

structure def S : HasArea + AlsoHasArea {
    param width : Real = 5.0
    param height : Real = 3.0
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // No error-severity diagnostics expected.
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Exactly 1 'area' value cell should exist (dedup, not 2).
    let area_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "area")
        .collect();
    assert_eq!(
        area_cells.len(),
        1,
        "expected exactly 1 'area' value cell after merge, got {}",
        area_cells.len()
    );
}

/// Step 2: Trait A has `let x : Length = 5mm`, trait B has `let x : Mass = 1kg`.
/// Structure implements both — different types → 'conflicting' error.
/// NOTE: This test is EXPECTED TO FAIL until step-3 fixes the Type::Real sentinel.
#[test]
fn let_defaults_same_name_different_type_error() {
    let source = r#"
trait A {
    let x : Length = 5mm
}

trait B {
    let x : Mass = 1kg
}

structure def U : A + B {
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected conflict diagnostic for same-name different-type let defaults");

    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("conflicting"),
        "error should mention 'conflicting', got: {}",
        error_msg
    );
}

/// Step 1b: Two traits each requiring `param x : Length`.
/// Structure provides `param x : Length = 5mm` — requirement dedup baseline.
/// Expect 0 errors (existing behavior).
#[test]
fn param_requirements_same_name_same_type_merge() {
    let source = r#"
trait NeedsX {
    param x : Length
}

trait AlsoNeedsX {
    param x : Length
}

structure def T : NeedsX + AlsoNeedsX {
    param x : Length = 5mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    // No error-severity diagnostics expected — same-type requirement dedup.
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}
