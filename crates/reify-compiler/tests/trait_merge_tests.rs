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
/// Fixed by c6751bf1c: content_hash comparison for let-binding defaults.
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

/// Step 4: Trait A has `let x : Real = width + 1`, trait B has `let x : Real = width * 2`.
/// Same name, same type, different expressions — expect 'conflicting' error.
/// Fixed by c6751bf1c: content_hash comparison catches expression differences.
#[test]
fn let_defaults_same_name_same_type_different_expr_error() {
    let source = r#"
trait A {
    let x : Real = width + 1.0
}

trait B {
    let x : Real = width * 2.0
}

structure def V : A + B {
    param width : Real = 5.0
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected conflict diagnostic for same-name same-type different-expression let defaults"
    );

    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("conflicting"),
        "error should mention 'conflicting', got: {}",
        error_msg
    );
}

/// Step 6: Trait A requires `param x : Length` (no default),
/// trait B provides `param x : Length = 10mm` (default).
/// Structure implements both with empty body — the default from B satisfies A's requirement.
/// Fixed by d545080b3: available_defaults cross-check in check_trait_conformance.
#[test]
fn requirement_satisfied_by_cross_trait_default() {
    let source = r#"
trait NeedsX {
    param x : Length
}

trait ProvidesX {
    param x : Length = 10mm
}

structure def W : NeedsX + ProvidesX {
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected 0 errors (default from ProvidesX satisfies NeedsX requirement), got: {:?}",
        errors
    );
}

/// Step 8a: Trait A has `let x : Real = a + 1`, trait B has `let x : Real = a * 2`.
/// Structure implements both and provides its own `let x : Real = a + a`.
/// Structure override resolves the conflict — expect 0 errors.
#[test]
fn let_conflict_resolved_by_structure_override() {
    let source = r#"
trait A {
    let x : Real = a + 1.0
}

trait B {
    let x : Real = a * 2.0
}

structure def R : A + B {
    param a : Real = 5.0
    let x : Real = a + a
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected 0 errors (structure override resolves let conflict), got: {:?}",
        errors
    );

    // Exactly 1 'x' value cell (the structure's own, not any trait default).
    let x_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "x")
        .collect();
    assert_eq!(
        x_cells.len(),
        1,
        "expected exactly 1 'x' value cell, got {}",
        x_cells.len()
    );
}

/// Step 8b: Trait A has `constraint x > 0mm`, trait B has `constraint x < 100mm`.
/// Structure provides `param x : Length = 5mm`. Both constraints should be injected.
#[test]
fn constraints_compose_conjunctively_across_traits() {
    let source = r#"
trait HasLowerBound {
    constraint x > 0mm
}

trait HasUpperBound {
    constraint x < 100mm
}

structure def Q : HasLowerBound + HasUpperBound {
    param x : Length = 5mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // At least 2 constraints injected (one from each trait).
    assert!(
        template.constraints.len() >= 2,
        "expected at least 2 constraints (one from each trait), got {}",
        template.constraints.len()
    );
}

/// Step 10: Comprehensive mixed-merging test.
/// Trait A: `param x : Length`, `let area : Real = x * x`, `constraint x > 0mm`.
/// Trait B: `param x : Length`, `let area : Real = x * x`, `constraint x < 1000mm`.
/// Structure implements A + B with `param x : Length = 5mm`.
/// Expect: 0 errors, exactly 1 'x' value cell, exactly 1 'area' value cell,
/// at least 2 constraints (one from each trait).
#[test]
fn mixed_merging_params_lets_constraints() {
    let source = r#"
trait GeomA {
    param x : Length
    let area : Real = x * x
    constraint x > 0mm
}

trait GeomB {
    param x : Length
    let area : Real = x * x
    constraint x < 1000mm
}

structure def M : GeomA + GeomB {
    param x : Length = 5mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // No error-severity diagnostics expected.
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Exactly 1 'x' value cell (the structure's own).
    let x_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "x")
        .collect();
    assert_eq!(
        x_cells.len(),
        1,
        "expected exactly 1 'x' value cell, got {}",
        x_cells.len()
    );

    // Exactly 1 'area' value cell (dedup of identical let defaults).
    let area_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "area")
        .collect();
    assert_eq!(
        area_cells.len(),
        1,
        "expected exactly 1 'area' value cell, got {}",
        area_cells.len()
    );

    // At least 2 constraints injected (one from each trait — both unlabeled).
    assert!(
        template.constraints.len() >= 2,
        "expected at least 2 constraints (one per trait), got {}",
        template.constraints.len()
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
