//! Compiler tests for some(expr) and none expressions.
//!
//! Tests verify that the compiler emits CompiledExprKind::OptionSome and
//! CompiledExprKind::OptionNone with correct types instead of falling through
//! to generic function call resolution.

use reify_compiler::{CompiledGuardedGroup, ValueCellDecl};
use reify_test_support::compile_first_template;
use reify_core::{DimensionVector, Severity, Type};
use reify_ir::CompiledExprKind;

/// Helper: compile source and extract the value cell named `cell_name`'s default_expr.
/// Panics if there are errors or the cell is missing.
fn compile_and_get_expr(source: &str, cell_name: &str) -> reify_ir::CompiledExpr {
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_option"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );

    let template = &compiled.templates[0];
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == cell_name)
        .unwrap_or_else(|| panic!("should have '{}' value cell", cell_name));

    cell.default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("'{}' cell should have a default_expr", cell_name))
        .clone()
}

/// Helper: compile source and expect diagnostics (errors allowed). Returns compiled module.
fn compile_expecting_errors(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_option"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

/// Helper: assert that `member` has cell_type == Option<inner_type>, a default_expr
/// of kind OptionNone, and that default_expr.result_type == Option<inner_type>.
/// `label` is incorporated into assertion failure messages for diagnostics.
fn assert_option_none(member: &ValueCellDecl, inner_type: Type, label: &str) {
    let option_type = Type::Option(Box::new(inner_type));
    assert_eq!(
        member.cell_type, option_type,
        "{label}: cell_type should be Option<{option_type:?}>, got {:?}",
        member.cell_type
    );
    let default = member
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("{label}: member should have a default_expr"));
    assert_eq!(
        default.result_type, option_type,
        "{label}: default_expr.result_type should be Option<{option_type:?}>, got {:?}",
        default.result_type
    );
    assert!(
        matches!(&default.kind, CompiledExprKind::OptionNone),
        "{label}: expected OptionNone, got {:?}",
        default.kind
    );
}

// ---------------------------------------------------------------------------
// step-2: some(42) → OptionSome with Int inner and Option<Int> result type
// ---------------------------------------------------------------------------

/// step-2: compile `let x = some(42)` → OptionSome wrapping Literal(Int(42)).
/// Currently FAILS because some(42) compiles as stdlib FunctionCall.
#[test]
fn compile_some_integer_literal() {
    let source = r#"
structure S {
    let x = some(42)
}
"#;
    let expr = compile_and_get_expr(source, "x");

    assert_eq!(
        expr.result_type,
        Type::Option(Box::new(Type::Int)),
        "some(42) should have type Option<Int>, got {:?}",
        expr.result_type
    );

    match &expr.kind {
        CompiledExprKind::OptionSome(inner) => {
            assert!(
                matches!(&inner.kind, CompiledExprKind::Literal(v) if matches!(v, reify_ir::Value::Int(42))),
                "expected Literal(Int(42)), got {:?}",
                inner.kind
            );
            assert_eq!(inner.result_type, Type::Int);
        }
        other => panic!("expected OptionSome, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// step-4: none → OptionNone with Option(_) result type
// ---------------------------------------------------------------------------

/// step-4: compile `let x = none` → OptionNone.
/// Currently FAILS because none produces 'unresolved name' error.
#[test]
fn compile_none_as_let_value() {
    let source = r#"
structure S {
    let x = none
}
"#;
    let expr = compile_and_get_expr(source, "x");

    assert!(
        matches!(expr.result_type, Type::Option(_)),
        "none should have type Option<_>, got {:?}",
        expr.result_type
    );

    assert!(
        matches!(&expr.kind, CompiledExprKind::OptionNone),
        "expected OptionNone, got {:?}",
        expr.kind
    );
}

// ---------------------------------------------------------------------------
// step-6: param with Option<Int> annotation and none default → typed OptionNone
// ---------------------------------------------------------------------------

/// step-6: compile `param x: Option<Int> = none` → OptionNone with type Option<Int>.
/// Currently FAILS because: (1) resolve_type doesn't handle Option<T>,
/// (2) none doesn't get type context from param annotation.
#[test]
fn compile_param_option_int_default_none() {
    let source = r#"
structure S {
    param x: Option<Int> = none
}
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_option"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );

    let template = &compiled.templates[0];
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("should have 'x' value cell");

    // The cell type should be Option<Int>
    assert_eq!(
        cell.cell_type,
        Type::Option(Box::new(Type::Int)),
        "cell_type should be Option<Int>, got {:?}",
        cell.cell_type
    );

    // The default_expr should be OptionNone with type Option<Int>
    let default = cell.default_expr.as_ref().expect("should have default");
    assert_eq!(
        default.result_type,
        Type::Option(Box::new(Type::Int)),
        "default_expr should have type Option<Int>, got {:?}",
        default.result_type
    );
    assert!(
        matches!(&default.kind, CompiledExprKind::OptionNone),
        "expected OptionNone, got {:?}",
        default.kind
    );
}

// ---------------------------------------------------------------------------
// step-8: edge cases
// ---------------------------------------------------------------------------

/// step-8a: some() with 0 args → diagnostic error emitted.
#[test]
fn compile_some_zero_args_emits_error() {
    let source = r#"
structure S {
    let x = some()
}
"#;
    let compiled = compile_expecting_errors(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected an error for some() with 0 args"
    );
    let msg = &errors[0].message;
    assert!(
        msg.contains("some") && (msg.contains("1") || msg.contains("argument")),
        "error message should mention 'some' and argument count, got: {:?}",
        msg
    );
}

/// step-8b: some(1, 2) with 2 args → diagnostic error emitted.
#[test]
fn compile_some_two_args_emits_error() {
    let source = r#"
structure S {
    let x = some(1, 2)
}
"#;
    let compiled = compile_expecting_errors(source);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected an error for some(1, 2) with 2 args"
    );
}

/// step-8c: nested some(some(42)) → OptionSome(OptionSome(Literal(42))) with type Option<Option<Int>>.
#[test]
fn compile_some_nested() {
    let source = r#"
structure S {
    let x = some(some(42))
}
"#;
    let expr = compile_and_get_expr(source, "x");

    assert_eq!(
        expr.result_type,
        Type::Option(Box::new(Type::Option(Box::new(Type::Int)))),
        "some(some(42)) should have type Option<Option<Int>>, got {:?}",
        expr.result_type
    );

    match &expr.kind {
        CompiledExprKind::OptionSome(outer_inner) => {
            assert_eq!(
                outer_inner.result_type,
                Type::Option(Box::new(Type::Int)),
                "inner should have type Option<Int>, got {:?}",
                outer_inner.result_type
            );
            match &outer_inner.kind {
                CompiledExprKind::OptionSome(innermost) => {
                    assert!(
                        matches!(&innermost.kind, CompiledExprKind::Literal(v) if matches!(v, reify_ir::Value::Int(42))),
                        "expected Literal(Int(42)), got {:?}",
                        innermost.kind
                    );
                }
                other => panic!("expected inner OptionSome, got {:?}", other),
            }
        }
        other => panic!("expected outer OptionSome, got {:?}", other),
    }
}

/// step-8d: some(x) where x is a param → OptionSome(ValueRef) with Option<param_type>.
#[test]
fn compile_some_param_ref() {
    let source = r#"
structure S {
    param x: Int
    let y = some(x)
}
"#;
    let expr = compile_and_get_expr(source, "y");

    assert_eq!(
        expr.result_type,
        Type::Option(Box::new(Type::Int)),
        "some(x) where x:Int should have type Option<Int>, got {:?}",
        expr.result_type
    );

    match &expr.kind {
        CompiledExprKind::OptionSome(inner) => {
            assert!(
                matches!(&inner.kind, CompiledExprKind::ValueRef(_)),
                "expected ValueRef for 'x', got {:?}",
                inner.kind
            );
            assert_eq!(inner.result_type, Type::Int);
        }
        other => panic!("expected OptionSome, got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// step-10: no-context none → OptionNone with Option<Real> default, no error
// ---------------------------------------------------------------------------

/// step-10: `let x = none` (no type annotation) → OptionNone with Type::Option(Type::dimensionless_scalar()),
/// no error diagnostics. Verifies graceful fallback when type cannot be inferred.
#[test]
fn compile_none_no_context_defaults_to_option_real() {
    let source = r#"
structure S {
    let x = none
}
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_option"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for `let x = none`, got: {:?}",
        errors
    );

    let template = &compiled.templates[0];
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("should have 'x' value cell");

    let default = cell.default_expr.as_ref().expect("should have default");
    assert_eq!(
        default.result_type,
        Type::Option(Box::new(Type::dimensionless_scalar())),
        "none without context should default to Option<Real>, got {:?}",
        default.result_type
    );
    assert!(
        matches!(&default.kind, CompiledExprKind::OptionNone),
        "expected OptionNone, got {:?}",
        default.kind
    );
}

// ---------------------------------------------------------------------------
// task 1098: port param Option<Length> = none → typed OptionNone (step-1)
// ---------------------------------------------------------------------------

/// Port param with Option<Length> annotation and none default should produce
/// a ValueCellDecl with cell_type == Option<Length> and default_expr that is
/// OptionNone with result_type == Option<Length> (not the fallback Option<Real>).
#[test]
fn port_param_none_with_typed_annotation_gets_correct_type() {
    let source = r#"
trait MyPort {
    param x : Length
}

structure def S {
    port p : MyPort {
        param x : Option<Length> = none
    }
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(template.ports.len(), 1, "expected 1 port");
    let port = &template.ports[0];
    assert_eq!(port.name, "p");

    let member = port
        .members
        .iter()
        .find(|m| m.id.member == "p.x")
        .expect("should have port member 'p.x'");

    assert_option_none(
        member,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "port param",
    );
}

// ---------------------------------------------------------------------------
// task 1098: port let Option<Length> = none → typed OptionNone (step-3)
// ---------------------------------------------------------------------------

/// Port let with Option<Length> annotation and none value should produce
/// a ValueCellDecl with cell_type == Option<Length> and default_expr that is
/// OptionNone with result_type == Option<Length> (not the fallback Option<Real>).
#[test]
fn port_let_none_with_typed_annotation_gets_correct_type() {
    let source = r#"
trait MyPort {
    param x : Length
}

structure def S {
    port p : MyPort {
        param x : Length = 5mm
        let y : Option<Length> = none
    }
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(template.ports.len(), 1, "expected 1 port");
    let port = &template.ports[0];

    let member = port
        .members
        .iter()
        .find(|m| m.id.member == "p.y")
        .expect("should have port member 'p.y'");

    assert_option_none(
        member,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "port let",
    );
}

// ---------------------------------------------------------------------------
// task 1098: guarded param Option<Length> = none → typed OptionNone (step-5)
// ---------------------------------------------------------------------------

/// Guarded param with Option<Length> annotation and none default should produce
/// a ValueCellDecl in guarded_groups[0].members with cell_type == Option<Length>
/// and default_expr that is OptionNone with result_type == Option<Length>
/// (not the fallback Option<Real>).
#[test]
fn guarded_param_none_with_typed_annotation_gets_correct_type() {
    let source = r#"
structure S {
    param active : Bool = true
    where active {
        param x : Option<Length> = none
    }
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(template.guarded_groups.len(), 1, "expected 1 guarded group");
    let group: &CompiledGuardedGroup = &template.guarded_groups[0];

    let member = group
        .members
        .iter()
        .find(|m| m.id.member == "x")
        .expect("should have guarded member 'x'");

    assert_option_none(
        member,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "guarded param",
    );
}

// ---------------------------------------------------------------------------
// task 1098: guarded let Option<Length> = none → typed OptionNone (step-7)
// ---------------------------------------------------------------------------

/// Guarded let with Option<Length> annotation and none value should produce
/// a ValueCellDecl in guarded_groups[0].members with cell_type == Option<Length>
/// and default_expr that is OptionNone with result_type == Option<Length>
/// (not the fallback Option<Real>).
#[test]
fn guarded_let_none_with_typed_annotation_gets_correct_type() {
    let source = r#"
structure S {
    param active : Bool = true
    where active {
        let y : Option<Length> = none
    }
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(template.guarded_groups.len(), 1, "expected 1 guarded group");
    let group: &CompiledGuardedGroup = &template.guarded_groups[0];

    let member = group
        .members
        .iter()
        .find(|m| m.id.member == "y")
        .expect("should have guarded member 'y'");

    assert_option_none(
        member,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "guarded let",
    );
}

// ---------------------------------------------------------------------------
// task 1098 amendment: nested guarded param (suggestion 3)
// ---------------------------------------------------------------------------

/// Nested guarded param with Option<Length> annotation and none default should
/// produce a ValueCellDecl in the inner guarded group with cell_type ==
/// Option<Length> and default_expr OptionNone with result_type == Option<Length>.
///
/// This exercises the recursive `register_guarded_names` and
/// `compile_guarded_members` paths that were updated to thread
/// `type_param_names` / `alias_registry`.
#[test]
fn nested_guarded_param_none_with_typed_annotation_gets_correct_type() {
    let source = r#"
structure S {
    param active : Bool = true
    where active {
        param inner_flag : Bool = true
        where inner_flag {
            param x : Option<Length> = none
        }
    }
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    // The inner guarded group contains 'x'; find it by searching all groups.
    let group = template
        .guarded_groups
        .iter()
        .find(|g| g.members.iter().any(|m| m.id.member == "x"))
        .expect("should have a guarded group with member 'x'");

    let member = group
        .members
        .iter()
        .find(|m| m.id.member == "x")
        .expect("should have nested guarded member 'x'");

    assert_option_none(
        member,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "nested guarded param",
    );
}

// ---------------------------------------------------------------------------
// task 1098 amendment: type alias inside guarded block (suggestion 4)
// ---------------------------------------------------------------------------

/// Guarded param using a type alias `Option<MyLen> = none` should resolve the
/// alias and produce cell_type == Option<Length> with OptionNone result_type ==
/// Option<Length>.
///
/// This verifies that the upgrade from `resolve_type_name` to
/// `resolve_type_expr_with_aliases` in `register_guarded_names` correctly
/// handles type aliases, not just built-in type names.
#[test]
fn guarded_param_option_type_alias_none_gets_correct_type() {
    let source = r#"
type MyLen = Length

structure S {
    param active : Bool = true
    where active {
        param x : Option<MyLen> = none
    }
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(template.guarded_groups.len(), 1, "expected 1 guarded group");
    let group: &CompiledGuardedGroup = &template.guarded_groups[0];

    let member = group
        .members
        .iter()
        .find(|m| m.id.member == "x")
        .expect("should have guarded member 'x'");

    // MyLen resolves to Length, so Option<MyLen> resolves to Option<Length>.
    assert_option_none(
        member,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "guarded param (alias)",
    );
}

// ---------------------------------------------------------------------------
// task 1726: port param Option<MyLen> = none with type alias → typed OptionNone
// (analogous to guarded_param_option_type_alias_none_gets_correct_type)
// ---------------------------------------------------------------------------

/// Port param using a type alias `Option<MyLen> = none` should resolve the alias
/// and produce cell_type == Option<Length> with OptionNone result_type ==
/// Option<Length>.
///
/// This exercises the resolve_type_expr_with_aliases call at entity.rs:368 (pass-1
/// registration) and fixup_option_none_for_param (pass-2) for port params with a
/// type alias.  The existing port test uses Option<Length> directly; this test
/// specifically exercises the alias-resolution code path.
#[test]
fn port_param_option_type_alias_none_gets_correct_type() {
    let source = r#"
type MyLen = Length

trait MyPort {
    param x : Length
}

structure def S {
    port p : MyPort {
        param x : Option<MyLen> = none
    }
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(template.ports.len(), 1, "expected 1 port");
    let port = &template.ports[0];
    assert_eq!(port.name, "p");

    let member = port
        .members
        .iter()
        .find(|m| m.id.member == "p.x")
        .expect("should have port member 'p.x'");

    // MyLen resolves to Length, so Option<MyLen> resolves to Option<Length>.
    assert_option_none(
        member,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "port param (alias)",
    );
}

// ---------------------------------------------------------------------------
// task 1726: port let Option<MyLen> = none with type alias → typed OptionNone
// (analogous to guarded_param_option_type_alias_none_gets_correct_type)
// ---------------------------------------------------------------------------

/// Port let using a type alias `Option<MyLen> = none` should resolve the alias
/// and produce cell_type == Option<Length> with OptionNone result_type ==
/// Option<Length>.
///
/// This exercises the fixup_option_none_for_let path, which independently calls
/// resolve_type_expr_with_aliases for port let members.  The existing port let
/// test uses Option<Length> directly; this test specifically exercises the
/// alias-resolution code path.
#[test]
fn port_let_option_type_alias_none_gets_correct_type() {
    let source = r#"
type MyLen = Length

trait MyPort {
    param x : Length
}

structure def S {
    port p : MyPort {
        param x : Length = 5mm
        let y : Option<MyLen> = none
    }
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(template.ports.len(), 1, "expected 1 port");
    let port = &template.ports[0];

    let member = port
        .members
        .iter()
        .find(|m| m.id.member == "p.y")
        .expect("should have port member 'p.y'");

    // MyLen resolves to Length, so Option<MyLen> resolves to Option<Length>.
    assert_option_none(
        member,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "port let (alias)",
    );
}

// ---------------------------------------------------------------------------
// task 1726 amend: guarded let Option<MyLen> = none with type alias → typed OptionNone
// (the missing counterpart to guarded_param_option_type_alias_none_gets_correct_type)
// ---------------------------------------------------------------------------

/// Guarded let using a type alias `Option<MyLen> = none` should resolve the
/// alias and produce cell_type == Option<Length> with OptionNone result_type ==
/// Option<Length>.
///
/// This exercises the `fixup_option_none_for_let` path inside guarded blocks
/// when the annotation references a user-defined type alias rather than a
/// built-in type name directly.  The non-alias variant is covered by
/// `guarded_let_none_with_typed_annotation_gets_correct_type`; this test
/// specifically exercises the alias-resolution code path.
#[test]
fn guarded_let_option_type_alias_none_gets_correct_type() {
    let source = r#"
type MyLen = Length

structure S {
    param active : Bool = true
    where active {
        let y : Option<MyLen> = none
    }
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(template.guarded_groups.len(), 1, "expected 1 guarded group");
    let group: &CompiledGuardedGroup = &template.guarded_groups[0];

    let member = group
        .members
        .iter()
        .find(|m| m.id.member == "y")
        .expect("should have guarded member 'y'");

    // MyLen resolves to Length, so Option<MyLen> resolves to Option<Length>.
    assert_option_none(
        member,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "guarded let (alias)",
    );
}

// ---------------------------------------------------------------------------
// task 1725: guarded else-branch param Option<Length> = none → typed OptionNone
// ---------------------------------------------------------------------------

/// Guarded else-branch param with Option<Length> annotation and none default should
/// produce a ValueCellDecl in guarded_groups[0].else_members with
/// cell_type == Option<Length> and default_expr that is OptionNone with
/// result_type == Option<Length> (not the fallback Option<Real>).
#[test]
fn guarded_else_param_none_with_typed_annotation_gets_correct_type() {
    let source = r#"
structure S {
    param active : Bool = true
    where active {
    } else {
        param x : Option<Length> = none
    }
}
"#;
    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);

    assert_eq!(template.guarded_groups.len(), 1, "expected 1 guarded group");
    let group: &CompiledGuardedGroup = &template.guarded_groups[0];

    let member = group
        .else_members
        .iter()
        .find(|m| m.id.member == "x")
        .expect("should have else-branch guarded member 'x'");

    assert_option_none(
        member,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "else-branch guarded param",
    );
}

// ---------------------------------------------------------------------------
// task 193 / suggestion #19: non-port let Option<Length> = none → typed OptionNone
// (relocated from qualified_access_compile_tests.rs)
// ---------------------------------------------------------------------------

/// `let y : Option<Length> = none` should compile with cell_type = Option<Length>,
/// not the placeholder Option<Real> that the `none` keyword produces.
///
/// Source: `structure def S { let y : Option<Length> = none }`
///
/// Assert: no compile errors, value cell 'y' has type Option<Length> (not Option<Real>),
/// and default_expr is OptionNone with matching result_type.
#[test]
fn let_none_with_typed_annotation_gets_correct_type() {
    let source = r#"
structure def S {
    let y : Option<Length> = none
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected compile errors: {:?}", errors);

    let y_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "y")
        .expect("expected value cell 'y'");

    let expected_type = Type::Option(Box::new(Type::Scalar {
        dimension: DimensionVector::LENGTH,
    }));
    assert_eq!(
        y_cell.cell_type, expected_type,
        "cell_type for 'let y : Option<Length> = none' should be Option<Length>, got: {:?}",
        y_cell.cell_type
    );

    // Also verify that the default_expr has been fixed up to have the correct type.
    let default_expr = y_cell
        .default_expr
        .as_ref()
        .expect("expected default_expr for let y");
    assert_eq!(
        default_expr.result_type, expected_type,
        "default_expr.result_type should be Option<Length> after OptionNone fixup, got: {:?}",
        default_expr.result_type
    );
    assert!(
        matches!(default_expr.kind, CompiledExprKind::OptionNone),
        "default_expr.kind should be OptionNone, got: {:?}",
        default_expr.kind
    );
}
