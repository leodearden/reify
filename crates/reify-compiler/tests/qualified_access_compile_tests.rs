//! Qualified trait access compilation tests — task 193.
//!
//! Tests for compiling `TypeName::member` (qualified access) and
//! `expr.(TypeName::member)` (instance-level qualified access) expressions.
//! Covers disambiguation, error diagnostics, and constraint compilation.
//!
//! Compiler-side qualified access support restored 2026-04-08 from commit
//! 4e8d65153 (lost in the c88ca9635/3a248e07d regression cluster; see
//! project_regression_c88ca9635.md).

use reify_compiler::*;
use reify_types::*;

// ── Helpers ──────────────────────────────────────────────────────────────────

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

    let errors: Vec<_> =
        diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected compile errors: {:?}", errors);

    let has_x = template.value_cells.iter().any(|vc| vc.id.member == "x");
    assert!(has_x, "expected value cell 'x' in template");

    let has_y = template.value_cells.iter().any(|vc| vc.id.member == "y");
    assert!(
        has_y,
        "expected value cell 'y' in template, got: {:?}",
        template.value_cells.iter().map(|vc| &vc.id.member).collect::<Vec<_>>()
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

    let errors: Vec<_> =
        diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected compile errors: {:?}", errors);

    let cell_names: Vec<_> =
        template.value_cells.iter().map(|vc| vc.id.member.as_str()).collect();

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

    let module = compile_module(source);

    let errors: Vec<_> =
        module.diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(!errors.is_empty(), "expected error diagnostic for unknown trait");

    let error_msg = format!("{:?}", errors);
    let lower = error_msg.to_lowercase();
    assert!(
        lower.contains("trait") && (lower.contains("not found") || lower.contains("unknown")),
        "error should mention 'trait' and 'not found'/'unknown', got: {}",
        error_msg
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

    let module = compile_module(source);

    let errors: Vec<_> =
        module.diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(!errors.is_empty(), "expected error diagnostic for missing trait member");

    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("nonexistent"),
        "error should mention 'nonexistent', got: {}",
        error_msg
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

    let errors: Vec<_> =
        diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
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

    let module = compile_module(source);

    let errors: Vec<_> =
        module.diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(
        !errors.is_empty(),
        "expected error diagnostic: Inner does not implement A"
    );
}

// ── step-13 ───────────────────────────────────────────────────────────────────

/// `let y : Option<Length> = none` should compile with cell_type = Option<Length>,
/// not the placeholder Option<Real> that the `none` keyword produces.
///
/// Source: `structure def S { let y : Option<Length> = none }`
///
/// Assert: no compile errors, value cell 'y' has type Option<Length> (not Option<Real>),
/// and default_expr is OptionNone with matching result_type.
///
/// Currently fails because the let-binding second pass uses compiled_expr.result_type
/// (Option<Real> placeholder) instead of consulting let_decl.type_expr, and lacks the
/// OptionNone fixup present in the param-default path.
#[test]
fn let_none_with_typed_annotation_gets_correct_type() {
    let source = r#"
structure def S {
    let y : Option<Length> = none
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> =
        diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
    assert!(errors.is_empty(), "unexpected compile errors: {:?}", errors);

    let y_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "y")
        .expect("expected value cell 'y'");

    let expected_type = Type::Option(Box::new(Type::Scalar { dimension: DimensionVector::LENGTH }));
    assert_eq!(
        y_cell.cell_type,
        expected_type,
        "cell_type for 'let y : Option<Length> = none' should be Option<Length>, got: {:?}",
        y_cell.cell_type
    );

    // Also verify that the default_expr has been fixed up to have the correct type.
    let default_expr = y_cell.default_expr.as_ref().expect("expected default_expr for let y");
    assert_eq!(
        default_expr.result_type,
        expected_type,
        "default_expr.result_type should be Option<Length> after OptionNone fixup, got: {:?}",
        default_expr.result_type
    );
    assert!(
        matches!(default_expr.kind, CompiledExprKind::OptionNone),
        "default_expr.kind should be OptionNone, got: {:?}",
        default_expr.kind
    );
}
