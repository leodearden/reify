//! Match on the PRELUDE `Result<T, E>` with typed Ok/Err payload binders —
//! task β #4036 (PRD docs/prds/v0_6/result-and-fallback.md).
//!
//! Task 4036 is a CHARACTERIZATION/PIN task: the generic-enum match substrate
//! delivered by δ (#4032, typed pattern binders), ε (#4033, generic eval) and
//! the ζ/DCE payload-binding eval (#3946) already handles `match` over the
//! PRELUDE `Result<T, E>` (`stdlib/result.ri`) identically to a locally
//! declared generic enum — this file locks that surface with NO inline
//! `enum Result` declaration anywhere (mirrors `result_prelude_enum_tests.rs`
//! #4035's construction-inference pins, but for `match`).
//!
//! Mirrors `generic_enum_pattern_binder_tests.rs` (#4032) minus its inline
//! `RESULT_ENUM_SOURCE` fixture: every source below relies solely on the
//! PRELUDE Result via `reify_test_support::compile_source_with_stdlib`.
//!
//! Tests in this section:
//!   (T-axis binder) `Ok { value: v } => v + 1mm, Err { error: m } => 6mm`
//!     over `r : Result<Length, String>` types `v` as `Length` (δ subst
//!     through the prelude) → zero errors; `v + 1N` → dimension mismatch.
//!   (exhaustiveness) a non-exhaustive match (`Ok` arm only) over the
//!     `Type::Applied` discriminant still fires the missing-variant
//!     diagnostic; an exhaustive match (via `Err` arm or `_`) does not.
//!
//! The E-axis (`Err { error: msg }` binder typed `String`) is pinned in a
//! later section of this same file.

use reify_core::Severity;
use reify_test_support::compile_source_with_stdlib;

// ═══════════════════════════════════════════════════════════════════════════
// T-axis binder: `Ok { value: v }` types `v` as the substituted `T` (Length)
// ═══════════════════════════════════════════════════════════════════════════

/// [CORE SIGNAL] `Ok { value: v } => v + 1mm, Err { error: m } => 6mm` over
/// `r : Result<Length, String>` (the PRELUDE Result, no inline `enum Result`)
/// must produce ZERO Error diagnostics — δ (#4032) substitutes the payload
/// binder `v` at `T = Length` through the `Type::Applied` discriminant, so
/// `v + 1mm` is a clean Length+Length add.
#[test]
fn ok_binder_typed_length_clean_arm_no_errors() {
    let source = r#"
structure def Widget {
    param r : Result<Length, String>
    let bore = match r {
        Ok { value: v } => v + 1mm,
        Err { error: m } => 6mm,
    }
}
"#;
    let module = compile_source_with_stdlib(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "Ok {{ value: v }} => v + 1mm over the PRELUDE Result<Length, String> must \
         produce no errors (v typed Length via δ subst); got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// Dimension mismatch: `v + 1N` where `v : Length` (substituted through the
/// PRELUDE Result) → at least one Error diagnostic. Regression guard
/// alongside the clean-arm test above so a fix that merely suppresses all
/// binder errors (rather than substituting the real type) would be caught.
#[test]
fn ok_binder_typed_length_mismatch_arm_has_errors() {
    let source = r#"
structure def Widget {
    param r : Result<Length, String>
    let bad = match r {
        Ok { value: v } => v + 1N,
        Err { error: m } => 6mm,
    }
}
"#;
    let module = compile_source_with_stdlib(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "Ok {{ value: v }} => v + 1N where v : Length (PRELUDE Result) must \
         produce at least one dimension-mismatch error; got none"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// Exhaustiveness over the `Type::Applied` discriminant (PRELUDE Result)
// ═══════════════════════════════════════════════════════════════════════════

/// (non-exhaustive) `match r { Ok { value: v } => v + 1mm }` — no `Err` arm,
/// no `_` — over the PRELUDE `Result<Length, String>` must produce a
/// non-exhaustiveness Error. Empirically: "non-exhaustive match on 'Result':
/// missing variant(s) Err". The diagnostic carries NO `DiagnosticCode`, so
/// the assertion keys on the message substring "exhaustive" (lower-cased),
/// mirroring `generic_enum_pattern_binder_tests.rs`'s exhaustiveness pins.
#[test]
fn non_exhaustive_match_missing_err_arm_errors() {
    let source = r#"
structure def Widget {
    param r : Result<Length, String>
    let x = match r {
        Ok { value: v } => v + 1mm,
    }
}
"#;
    let module = compile_source_with_stdlib(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "non-exhaustive match (Ok arm only) over the PRELUDE Result<Length, String> \
         must produce an error; got none"
    );
    let has_exhaustive_msg = errors
        .iter()
        .any(|e| e.message.to_lowercase().contains("exhaustive"));
    assert!(
        has_exhaustive_msg,
        "error message must mention 'exhaustive' (e.g. 'non-exhaustive match'); got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// (exhaustive via `Err` arm) adding the `Err { error: m } => 6mm` arm makes
/// the match exhaustive — no "exhaustive"/"missing variant" error.
#[test]
fn exhaustive_match_with_err_arm_no_missing_variant_error() {
    let source = r#"
structure def Widget {
    param r : Result<Length, String>
    let x = match r {
        Ok { value: v } => v + 1mm,
        Err { error: m } => 6mm,
    }
}
"#;
    let module = compile_source_with_stdlib(source);
    let exhaustive_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .filter(|d| {
            d.message.to_lowercase().contains("exhaustive")
                || d.message.to_lowercase().contains("missing variant")
        })
        .collect();
    assert!(
        exhaustive_errors.is_empty(),
        "exhaustive match (Ok + Err arms) over the PRELUDE Result<Length, String> \
         must produce no non-exhaustiveness error; got: {:?}",
        exhaustive_errors
            .iter()
            .map(|e| &e.message)
            .collect::<Vec<_>>()
    );
}

/// (exhaustive via wildcard) `_ => 6mm` in place of the `Err` arm also makes
/// the match exhaustive — no "exhaustive"/"missing variant" error.
#[test]
fn exhaustive_match_with_wildcard_arm_no_missing_variant_error() {
    let source = r#"
structure def Widget {
    param r : Result<Length, String>
    let x = match r {
        Ok { value: v } => v + 1mm,
        _ => 6mm,
    }
}
"#;
    let module = compile_source_with_stdlib(source);
    let exhaustive_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .filter(|d| {
            d.message.to_lowercase().contains("exhaustive")
                || d.message.to_lowercase().contains("missing variant")
        })
        .collect();
    assert!(
        exhaustive_errors.is_empty(),
        "exhaustive match (Ok + wildcard) over the PRELUDE Result<Length, String> \
         must produce no non-exhaustiveness error; got: {:?}",
        exhaustive_errors
            .iter()
            .map(|e| &e.message)
            .collect::<Vec<_>>()
    );
}
