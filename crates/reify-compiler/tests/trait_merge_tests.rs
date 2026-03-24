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
