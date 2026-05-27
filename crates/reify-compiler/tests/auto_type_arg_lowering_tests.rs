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
use reify_test_support::parse_and_compile_with_stdlib;

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
