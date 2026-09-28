//! Determinacy predicate compilation tests.

use reify_core::{ContentHash, Severity};
use reify_ir::{CompiledExprKind, DeterminacyPredicateKind, TAG_DETERMINACY_PREDICATE};

/// step-27: Compile `constraint determined(x)` and verify its content_hash
/// matches the canonical stable-byte formula:
///   ContentHash::of(&[17, kind_byte]).combine(ContentHash::of_str(&cell_id))
/// where kind_byte = 0 for Determined (matching the quantifier pattern).
///
/// Written TDD-style (step-27 = red, step-28 = green) to pin the canonical hash
/// formula: discriminator tag 17 + kind_byte (0=Determined), matching the
/// quantifier factory method pattern in expr.rs.
#[test]
fn test_determinacy_hash_matches_canonical() {
    let source = r#"
structure S {
    param x : Length
    constraint determined(x)
}
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_det_hash"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no error diagnostics, got: {:?}",
        errors
    );

    let template = &compiled.templates[0];
    assert!(
        !template.constraints.is_empty(),
        "should have at least one constraint"
    );

    // Find the DeterminacyPredicate constraint
    let constraint_expr = &template.constraints[0].expr;
    match &constraint_expr.kind {
        CompiledExprKind::DeterminacyPredicate { kind, cell } => {
            assert_eq!(*kind, DeterminacyPredicateKind::Determined);

            // Compute the expected canonical hash using stable byte discriminators
            // (matching the quantifier pattern: [tag, kind_byte] + stringified cell_id)
            let kind_byte: u8 = 0; // Determined = 0
            let expected_hash = ContentHash::of(&[TAG_DETERMINACY_PREDICATE, kind_byte])
                .combine(ContentHash::of_str(&format!("{}", cell)));

            assert_eq!(
                constraint_expr.content_hash, expected_hash,
                "DeterminacyPredicate hash should use stable byte encoding [17, kind_byte], \
                 not Debug-string encoding. Got {:?}, expected {:?}",
                constraint_expr.content_hash, expected_hash
            );
        }
        other => panic!("expected DeterminacyPredicate, got {:?}", other),
    }
}

// --- Error-path regression tests (step-29) ---
// These guard the existing error handling in the compiler's determinacy predicate
// compilation. All three should pass immediately since the error paths already work.

/// step-29: determined() with zero arguments emits an error diagnostic.
#[test]
fn test_determined_wrong_arg_count_zero() {
    let source = r#"
structure S {
    constraint determined()
}
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_det_err0"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one error diagnostic for zero-arg determined()"
    );
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("expects 1 argument")),
        "expected 'expects 1 argument' error, got: {:?}",
        errors
    );
}

/// step-29: determined() with two arguments emits an error diagnostic.
#[test]
fn test_determined_wrong_arg_count_two() {
    let source = r#"
structure S {
    param a : Length
    param b : Length
    constraint determined(a, b)
}
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_det_err2"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one error diagnostic for two-arg determined()"
    );
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("expects 1 argument, got 2")),
        "expected 'expects 1 argument, got 2' error, got: {:?}",
        errors
    );
}

/// step-29: determined() with a computed expression (not a cell reference) emits an error.
#[test]
fn test_determined_non_cell_ref() {
    let source = r#"
structure S {
    param x : Length
    constraint determined(x + 1.0)
}
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test_det_err_ref"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least one error diagnostic for non-cell-ref determined()"
    );
    assert!(
        errors
            .iter()
            .any(|d| d.message.contains("must be a direct cell reference")),
        "expected 'must be a direct cell reference' error, got: {:?}",
        errors
    );
}
