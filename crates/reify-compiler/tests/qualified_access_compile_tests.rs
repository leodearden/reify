//! Qualified trait access compilation tests — task 193.
//!
//! Tests for compiling `TypeName::member` (qualified access) and
//! `expr.(TypeName::member)` (instance-level qualified access) expressions.
//! Covers disambiguation, error diagnostics, and constraint compilation.
//!
//! Compiler-side qualified access support restored 2026-04-08 from commit
//! 4e8d65153 (lost in the c88ca9635/3a248e07d regression cluster; see
//! project_regression_c88ca9635.md).

use reify_test_support::{compile_first_template, compile_source};
use reify_core::*;

// ── step-1 ────────────────────────────────────────────────────────────────────

/// Basic qualified access: `let y : Length = A::x` in a structure that conforms to A.
///
/// trait A { param x : Length }
/// structure S : A { param x : Length = 5mm, let y : Length = A::x }
///
/// Assert: no compile errors, template has value cells for both 'x' and 'y'.
#[test]
fn basic_qualified_access_compiles() {
    let source = r#"
trait A {
    param x : Length
}

structure def S : A {
    param x : Length = 5mm
    let y : Length = A::x
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected compile errors: {:?}", errors);

    let has_x = template.value_cells.iter().any(|vc| vc.id.member == "x");
    assert!(has_x, "expected value cell 'x' in template");

    let has_y = template.value_cells.iter().any(|vc| vc.id.member == "y");
    assert!(
        has_y,
        "expected value cell 'y' in template, got: {:?}",
        template
            .value_cells
            .iter()
            .map(|vc| &vc.id.member)
            .collect::<Vec<_>>()
    );
}

// ── step-2 ────────────────────────────────────────────────────────────────────

/// Disambiguation when two traits define the same member name.
///
/// trait A { param size : Length }
/// trait B { param size : Length }
/// structure S : A + B {
///     param size : Length = 5mm
///     let a_size : Length = A::size
///     let b_size : Length = B::size
/// }
///
/// Assert: no compile errors, template has value cells for 'size', 'a_size', 'b_size'.
/// Both qualified accesses resolve without ambiguity errors.
#[test]
fn disambiguation_two_traits_same_member() {
    let source = r#"
trait A {
    param size : Length
}

trait B {
    param size : Length
}

structure def S : A + B {
    param size : Length = 5mm
    let a_size : Length = A::size
    let b_size : Length = B::size
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected compile errors: {:?}", errors);

    let cell_names: Vec<_> = template
        .value_cells
        .iter()
        .map(|vc| vc.id.member.as_str())
        .collect();

    assert!(
        cell_names.contains(&"size"),
        "expected 'size' value cell, got: {:?}",
        cell_names
    );
    assert!(
        cell_names.contains(&"a_size"),
        "expected 'a_size' value cell, got: {:?}",
        cell_names
    );
    assert!(
        cell_names.contains(&"b_size"),
        "expected 'b_size' value cell, got: {:?}",
        cell_names
    );
}

// ── step-3 ────────────────────────────────────────────────────────────────────

/// Error when qualified access references a trait that does not exist.
///
/// structure S { param x : Length = 5mm, let y : Length = UnknownTrait::x }
///
/// Assert: error diagnostic containing 'trait' and 'not found' (or 'unknown').
#[test]
fn error_trait_not_found() {
    let source = r#"
structure def S {
    param x : Length = 5mm
    let y : Length = UnknownTrait::x
}
"#;

    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected error diagnostic for unknown trait"
    );

    let mentions_expected_keywords = errors.iter().any(|d| {
        let lower = d.message.to_lowercase();
        lower.contains("trait") && (lower.contains("not found") || lower.contains("unknown"))
    });
    assert!(
        mentions_expected_keywords,
        "error should mention 'trait' and 'not found'/'unknown', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ── step-4 ────────────────────────────────────────────────────────────────────

/// Error when qualified access references a member not defined in the trait.
///
/// trait A { param x : Length }
/// structure S : A { param x : Length = 5mm, let y : Length = A::nonexistent }
///
/// Assert: error diagnostic mentioning 'nonexistent' and indicating it is not in the trait.
#[test]
fn error_member_not_in_trait() {
    let source = r#"
trait A {
    param x : Length
}

structure def S : A {
    param x : Length = 5mm
    let y : Length = A::nonexistent
}
"#;

    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected error diagnostic for missing trait member"
    );

    let mentions_nonexistent = errors
        .iter()
        .any(|d| d.message.to_lowercase().contains("nonexistent"));
    assert!(
        mentions_nonexistent,
        "error should mention 'nonexistent', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ── step-5 ────────────────────────────────────────────────────────────────────

/// Qualified access in a constraint expression compiles cleanly.
///
/// trait A { param x : Length }
/// structure S : A { param x : Length = 5mm, constraint A::x > 0mm }
///
/// Assert: no compile errors, template has at least 1 constraint.
#[test]
fn qualified_access_in_constraint_expr() {
    let source = r#"
trait A {
    param x : Length
}

structure def S : A {
    param x : Length = 5mm
    constraint A::x > 0mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected compile errors: {:?}", errors);

    assert!(
        !template.constraints.is_empty(),
        "expected at least 1 constraint from qualified access expr"
    );
}

// ── step-6 ────────────────────────────────────────────────────────────────────

/// Error when instance qualified access is used on a sub-component that does
/// not implement the referenced trait.
///
/// trait A { param x : Length }
/// trait B { param y : Length }
/// structure Outer { sub inner = Inner, let z : Length = inner.(A::x) }
/// structure Inner : B { param y : Length = 3mm }
///
/// Inner only conforms to B, not A → error about A not being implemented by Inner.
#[test]
fn error_instance_does_not_implement_trait() {
    let source = r#"
trait A {
    param x : Length
}

trait B {
    param y : Length
}

structure def Inner : B {
    param y : Length = 3mm
}

structure def Outer {
    sub inner = Inner()
    let z : Length = inner.(A::x)
}
"#;

    let module = compile_source(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected error diagnostic: Inner does not implement A"
    );

    // Strengthen: at least one error must carry the typed `TraitNotImplemented`
    // diagnostic code produced by compile_instance_qualified_access_expr (expr.rs).
    // Anchoring to the typed code (introduced in task 2205) decouples the test from
    // human-readable wording while still distinguishing a genuine conformance
    // failure from an unrelated parse or type error.
    let mentions_trait_conformance = errors
        .iter()
        .any(|d| d.code == Some(DiagnosticCode::TraitNotImplemented));
    assert!(
        mentions_trait_conformance,
        "expected at least one error with code DiagnosticCode::TraitNotImplemented, got: {:?}",
        errors
            .iter()
            .map(|d| (d.code, &d.message))
            .collect::<Vec<_>>()
    );
}
