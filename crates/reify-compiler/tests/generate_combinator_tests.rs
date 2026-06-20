//! Compiler typing tests for the free-function `generate(n, |i| expr)` combinator
//! (task 3994 / structural-query ζ, PRD §5.9 / §2.3).
//!
//! `generate(n, |i| expr)` applies the lambda to indices `0..n-1` and collects the
//! results into a `List` whose element type is the lambda body type.
//!
//! Observable signals exercised here:
//!   - `generate(4, |i| i * 1mm)` types its cell to `List<Length>` (result typing, step-2).
//!   - `generate(3, |i| i)` types its cell to `List<Int>`         (index-param Int seeding, step-4).
//!   - non-Int count `generate(3mm, …)` / `generate(2.5, …)` emits ArgTypeMismatch (step-6).

use reify_core::{DiagnosticCode, Severity, Type};
use reify_test_support::compile_source;

/// Helper: fetch a structure template's let-cell `default_expr.result_type`.
fn cell_result_type(compiled: &reify_compiler::CompiledModule, structure: &str, cell: &str) -> Type {
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == structure)
        .unwrap_or_else(|| panic!("{} template not found", structure));
    let vc = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == cell)
        .unwrap_or_else(|| panic!("value cell '{}' not found in {}", cell, structure));
    vc.default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("cell '{}' has no default_expr", cell))
        .result_type
        .clone()
}

/// Helper: collect Error-severity diagnostic messages.
fn error_messages(compiled: &reify_compiler::CompiledModule) -> Vec<String> {
    compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.clone())
        .collect()
}

// ─── step-1: result typing ───

/// `generate(4, |i| i * 1mm)` types cell `xs` to `List<Length>` with zero Error
/// diagnostics. The body `i * 1mm` is `Length` whether `i` is Int or Real, so this
/// exercises the result-typing arm ALONE (independent of index-param Int seeding).
///
/// RED today: `generate` is unrecognized by `infer_list_helper_return_type`, so the
/// first-arg fallback types `xs` as `Int` (the count's type), not `List<Length>`.
#[test]
fn generate_result_types_to_list_of_body_type() {
    let source = r#"
        structure S {
            let xs = generate(4, |i| i * 1mm)
        }
    "#;
    let compiled = compile_source(source);

    let errors = error_messages(&compiled);
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics, got: {:?}",
        errors
    );

    assert_eq!(
        cell_result_type(&compiled, "S", "xs"),
        Type::List(Box::new(Type::length())),
        "expected xs : List<Length>",
    );
}

// ─── step-3: index param typed Int ───

/// `generate(3, |i| i)` types cell `xs` to `List<Int>`. The body `i` returns the
/// sole index param verbatim, so the cell element type is exactly the param type —
/// `List<Int>` ONLY IF the unannotated lambda param `i` is seeded to `Int`, not the
/// default Real.
///
/// RED today: unannotated lambda params default to `Type::dimensionless_scalar()`
/// (Real), so the cell types as `List<Real>` (= `List<Scalar{DIMENSIONLESS}>`), not
/// `List<Int>`. The index-param Int seeding (step-4) makes this GREEN.
#[test]
fn generate_seeds_index_param_to_int() {
    let source = r#"
        structure S {
            let xs = generate(3, |i| i)
        }
    "#;
    let compiled = compile_source(source);

    let errors = error_messages(&compiled);
    assert!(
        errors.is_empty(),
        "expected zero Error diagnostics, got: {:?}",
        errors
    );

    assert_eq!(
        cell_result_type(&compiled, "S", "xs"),
        Type::List(Box::new(Type::Int)),
        "expected xs : List<Int> (the sole unannotated index param `i` seeded to Int)",
    );
}

// ─── step-5: non-integer count compile diagnostic ───

/// Helper: collect the messages of diagnostics carrying `ArgTypeMismatch`.
/// (ArgTypeMismatch is always emitted at Error severity via `Diagnostic::error`.)
fn arg_type_mismatch_messages(compiled: &reify_compiler::CompiledModule) -> Vec<String> {
    compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::ArgTypeMismatch))
        .map(|d| d.message.clone())
        .collect()
}

/// A dimensioned (Length) count `generate(3mm, …)` emits an `ArgTypeMismatch`
/// referencing generate's count argument — the count must be a non-negative `Int`.
///
/// RED today: `builtin_arg_slots("generate")` returns `vec![]`, so
/// `check_builtin_arg_types` emits nothing for the count arg. The new
/// `ExpectedArg::Int` slot (step-6) makes this GREEN.
#[test]
fn generate_dimensioned_count_emits_arg_type_mismatch() {
    let source = r#"
        structure S {
            let xs = generate(3mm, |i| i)
        }
    "#;
    let compiled = compile_source(source);
    let mismatches = arg_type_mismatch_messages(&compiled);
    assert!(
        !mismatches.is_empty(),
        "expected an ArgTypeMismatch for the dimensioned count `3mm`, got none",
    );
    assert!(
        mismatches.iter().any(|m| m.contains("generate")),
        "ArgTypeMismatch message should reference `generate`: {:?}",
        mismatches,
    );
}

/// A dimensionless Real count `generate(2.5, …)` likewise emits `ArgTypeMismatch`
/// (the count must be `Int`, not `Real`).
#[test]
fn generate_real_count_emits_arg_type_mismatch() {
    let source = r#"
        structure S {
            let xs = generate(2.5, |i| i)
        }
    "#;
    let compiled = compile_source(source);
    assert!(
        !arg_type_mismatch_messages(&compiled).is_empty(),
        "expected an ArgTypeMismatch for the Real count `2.5`, got none",
    );
}

/// A well-typed `Int` count `generate(3, …)` emits NO `ArgTypeMismatch` — guards
/// against a false positive on the valid call.
#[test]
fn generate_int_count_emits_no_arg_type_mismatch() {
    let source = r#"
        structure S {
            let xs = generate(3, |i| i)
        }
    "#;
    let compiled = compile_source(source);
    let mismatches = arg_type_mismatch_messages(&compiled);
    assert!(
        mismatches.is_empty(),
        "a well-typed Int count must not emit ArgTypeMismatch, got: {:?}",
        mismatches,
    );
}
