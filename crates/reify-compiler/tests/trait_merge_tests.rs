//! Trait member merging tests — task 190.
//!
//! Focused tests for the trait member merging behaviour:
//! two-trait merge, shared-param dedup, diamond dedup,
//! conflict detection, constraint conjunction, and let-binding merge/conflict.

use reify_compiler::*;
use reify_types::*;

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Parse `source` and compile, returning the full CompiledModule.
fn compile_module(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    reify_compiler::compile(&parsed)
}

/// Parse `source`, compile, and return the first template together with all
/// diagnostics emitted during compilation.
fn compile_first_template(source: &str) -> (TopologyTemplate, Vec<Diagnostic>) {
    let module = compile_module(source);
    let template = module.templates.into_iter().next().expect("expected at least 1 template");
    (template, module.diagnostics)
}

// ── step-1 ───────────────────────────────────────────────────────────────────

/// Two traits with distinct params — structure S:A+B must satisfy both.
/// Assert: no errors, template contains value cells for both 'a' and 'b'.
#[test]
fn two_trait_merge_distinct_params() {
    let source = r#"
trait A {
    param a : Length
}

trait B {
    param b : Length
}

structure def S : A + B {
    param a : Length = 1mm
    param b : Length = 2mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let has_a = template.value_cells.iter().any(|vc| vc.id.member == "a");
    let has_b = template.value_cells.iter().any(|vc| vc.id.member == "b");
    assert!(has_a, "expected value cell 'a' from trait A");
    assert!(has_b, "expected value cell 'b' from trait B");
}

// ── step-2 ───────────────────────────────────────────────────────────────────

/// Two traits share the same `param x : Length`.
/// The requirement is deduplicated — structure S provides x once.
/// Assert: no errors, exactly 1 'x' value cell (not 2).
#[test]
fn two_trait_merge_shared_param_deduped() {
    let source = r#"
trait A {
    param x : Length
}

trait B {
    param x : Length
}

structure def S : A + B {
    param x : Length = 5mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let x_cells: Vec<_> = template.value_cells.iter().filter(|vc| vc.id.member == "x").collect();
    assert_eq!(
        x_cells.len(),
        1,
        "expected exactly 1 'x' value cell (deduplicated), got {}: {:?}",
        x_cells.len(),
        x_cells.iter().map(|vc| &vc.id).collect::<Vec<_>>()
    );
}

// ── step-3 ───────────────────────────────────────────────────────────────────

/// Diamond hierarchy: D{param x:Length}, B:D, C:D, A:B+C, structure S:A.
/// The param x from D is reachable via two paths (through B and through C).
/// The visited-set in collect_all_requirements deduplicates it.
/// Assert: no errors, exactly 1 'x' value cell (not 2).
#[test]
fn diamond_hierarchy_params_deduped() {
    let source = r#"
trait D {
    param x : Length
}

trait B : D {
}

trait C : D {
}

trait A : B + C {
}

structure def S : A {
    param x : Length = 5mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let x_cells: Vec<_> = template.value_cells.iter().filter(|vc| vc.id.member == "x").collect();
    assert_eq!(
        x_cells.len(),
        1,
        "expected exactly 1 'x' value cell (diamond dedup), got {}: {:?}",
        x_cells.len(),
        x_cells.iter().map(|vc| &vc.id).collect::<Vec<_>>()
    );
}

// ── step-4 ───────────────────────────────────────────────────────────────────

/// Diamond hierarchy with a default at the root: D{param x:Length=10mm}.
/// Structure S:A does not override x — the default is injected exactly once.
/// Assert: no errors, exactly 1 'x' value cell with default_expr set.
#[test]
fn diamond_hierarchy_default_deduped() {
    let source = r#"
trait D {
    param x : Length = 10mm
}

trait B : D {
}

trait C : D {
}

trait A : B + C {
}

structure def S : A {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let x_cells: Vec<_> = template.value_cells.iter().filter(|vc| vc.id.member == "x").collect();
    assert_eq!(
        x_cells.len(),
        1,
        "expected exactly 1 'x' value cell (diamond default dedup), got {}",
        x_cells.len()
    );
    assert!(
        x_cells[0].default_expr.is_some(),
        "expected default_expr to be set on the injected 'x' cell"
    );
}
