//! Integration tests for the compile-pipeline `auto:` / `auto(free):`
//! type-argument call-site (task 3558, B1).
//!
//! These tests drive real `.ri` source containing `Bearing<auto: Seal>()`
//! through `parse_and_compile_with_stdlib` / `compile_source_with_stdlib` and
//! assert on the resulting `CompiledModule.auto_type_substitution` and
//! diagnostics. Before the call-site wiring lands, `auto:` type-args fall into
//! the "unexpected dimensional expression in type argument" else-arm and the
//! substitution stays empty.

use reify_core::*;
use reify_test_support::{compile_source_with_stdlib, parse_and_compile_with_stdlib};

/// Single Seal-conformant candidate (`ORingSeal`) → the `auto: Seal` type-arg
/// resolves deterministically and populates the module's
/// `auto_type_substitution` with `("T", "ORingSeal")`, with no error
/// diagnostics.
#[test]
fn bearing_auto_seal_single_candidate_populates_substitution() {
    let source = r#"
        trait Seal {}
        structure def ORingSeal : Seal { param d : Real = 10.0 }
        structure def Bearing<T: Seal> { param bore : Real = 25.0 }
        structure def Assembly { sub b = Bearing<auto: Seal>() }
    "#;

    let compiled = parse_and_compile_with_stdlib(source);

    assert_eq!(
        compiled.auto_type_substitution.as_slice(),
        &[("T".to_string(), "ORingSeal".to_string())],
        "expected the auto: Seal slot to resolve to the single candidate ORingSeal"
    );

    let error_count = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count();
    assert_eq!(
        error_count, 0,
        "expected no error diagnostics, got: {:?}",
        compiled.diagnostics
    );
}

/// No Seal-conformant structure exists, so the strict `auto: Seal` slot has an
/// empty candidate pool → the resolver emits a single
/// `AutoTypeParamNoCandidate` error and leaves the substitution empty. Pins the
/// diagnostic-plumbing path: the resolver must be dispatched (and its
/// diagnostics routed into `ctx.diagnostics`) even when `pending_auto_resolutions`
/// resolves to nothing.
#[test]
fn auto_type_arg_no_candidate_emits_diagnostic() {
    let source = r#"
        trait Seal {}
        structure def Bearing<T: Seal> { param x : Real = 1.0 }
        structure def Assembly { sub b = Bearing<auto: Seal>() }
    "#;

    // Error-tolerant helper: we EXPECT an error diagnostic here.
    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<&Diagnostic> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one error diagnostic, got: {:?}",
        compiled.diagnostics
    );
    assert_eq!(
        errors[0].code,
        Some(DiagnosticCode::AutoTypeParamNoCandidate),
        "the lone error must be the no-candidate diagnostic"
    );

    assert!(
        compiled.auto_type_substitution.as_slice().is_empty(),
        "a failed resolution must leave the substitution empty, got: {:?}",
        compiled.auto_type_substitution.as_slice()
    );
}
