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

/// Two Seal-conformant candidates under STRICT `auto:` → ambiguous. The
/// resolver emits a single `AutoTypeParamAmbiguous` error and leaves the
/// substitution empty (strict mode never auto-picks among ≥2 feasible).
#[test]
fn strict_auto_type_arg_ambiguous_emits_error_diagnostic() {
    let source = r#"
        trait Seal {}
        structure def ORingSeal : Seal { param d : Real = 10.0 }
        structure def GasketSeal : Seal { param w : Real = 2.0 }
        structure def Bearing<T: Seal> { param bore : Real = 25.0 }
        structure def Assembly { sub b = Bearing<auto: Seal>() }
    "#;

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
        Some(DiagnosticCode::AutoTypeParamAmbiguous),
        "the lone error must be the ambiguous diagnostic"
    );

    assert!(
        compiled.auto_type_substitution.as_slice().is_empty(),
        "strict ambiguous resolution must leave the substitution empty, got: {:?}",
        compiled.auto_type_substitution.as_slice()
    );
}

/// Two Seal-conformant candidates under FREE `auto(free):` → the resolver picks
/// the lexicographically-first feasible candidate (`GasketSeal` < `ORingSeal`)
/// and emits a single `AutoTypeParamNonUnique` *warning* (not an error), so the
/// substitution is populated with the lex-first pick.
#[test]
fn free_auto_type_arg_ambiguous_selects_lex_first_with_warning() {
    let source = r#"
        trait Seal {}
        structure def ORingSeal : Seal { param d : Real = 10.0 }
        structure def GasketSeal : Seal { param w : Real = 2.0 }
        structure def Bearing<T: Seal> { param bore : Real = 25.0 }
        structure def Assembly { sub b = Bearing<auto(free): Seal>() }
    "#;

    // No Error-severity diagnostics expected (free mode warns, never errors).
    let compiled = parse_and_compile_with_stdlib(source);

    let nonunique: Vec<&Diagnostic> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::AutoTypeParamNonUnique))
        .collect();
    assert_eq!(
        nonunique.len(),
        1,
        "expected exactly one non-unique warning, got: {:?}",
        compiled.diagnostics
    );
    assert_eq!(
        nonunique[0].severity,
        Severity::Warning,
        "auto(free) non-unique resolution must warn, not error"
    );

    assert_eq!(
        compiled.auto_type_substitution.as_slice(),
        &[("T".to_string(), "GasketSeal".to_string())],
        "free mode must pick the lexicographically-first feasible candidate"
    );
}

/// Two `auto:` slots on one sub-component, each bound to a different trait with
/// a single conformant candidate → both resolve, and the substitution preserves
/// the target template's declared type-param order (`T` then `U`).
#[test]
fn multi_param_auto_type_args_resolve_in_declared_order() {
    let source = r#"
        trait Seal {}
        trait Cooled {}
        structure def ORingSeal : Seal { param d : Real = 10.0 }
        structure def AirCooled : Cooled { param f : Real = 5.0 }
        structure def Coupling<T: Seal, U: Cooled> { param x : Real = 1.0 }
        structure def Assembly { sub c = Coupling<auto: Seal, auto: Cooled>() }
    "#;

    let compiled = parse_and_compile_with_stdlib(source);

    assert_eq!(
        compiled.auto_type_substitution.as_slice(),
        &[
            ("T".to_string(), "ORingSeal".to_string()),
            ("U".to_string(), "AirCooled".to_string()),
        ],
        "multi-param substitution must follow the target's declared type-param order"
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
