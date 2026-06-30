//! Type-preserving pattern binders over generic enums — task δ #4032.
//!
//! Tests for:
//! (a) Annotation layer (S1/S2): `Result<Length, String>` as a structure param
//!     resolves to `Type::Applied { name: "Result", args: [Length, String] }` with NO
//!     errors (not the current "enum `Result` does not accept type arguments" rejection).
//! (b) Binder substitution (S3/S4): `match r { Ok { value: v } => v + 1mm, ... }`
//!     over `r : Result<Length, String>` types `v` as `Length` so `v + 1mm` is clean
//!     and `v + 1N` produces a dimension-mismatch error.
//! (c) Exhaustiveness (S5/S6): non-exhaustive matches over a `Type::Applied` discriminant
//!     still emit the missing-variant diagnostic (DCE D4 preserved for generic enums).

mod common;

use common::compile_with_stdlib_helper;
use reify_core::{Severity, Type};

// ─── Shared fixtures ─────────────────────────────────────────────────────────

/// Source for `enum Result<T, E>` — the generic two-param enum used across multiple tests.
const RESULT_ENUM_SOURCE: &str = "\
enum Result<T, E> {
    Ok { value: T },
    Err { error: E },
}
";

// ═══════════════════════════════════════════════════════════════════════════════
// S1 — RED: annotation layer (S2 makes these GREEN)
// ═══════════════════════════════════════════════════════════════════════════════

/// (a) δ signal: `param r : Result<Length, String>` must resolve `r` to
/// `Type::Applied { name: "Result", args: [Type::length(), Type::String] }` with
/// NO Error-severity diagnostics.
///
/// RED until S2: today `resolve_enum_type` drops args and emits
/// "enum `Result` does not accept type arguments".
#[test]
fn annotation_generic_enum_applied_type_cell_type() {
    let source = format!(
        "{RESULT_ENUM_SOURCE}\nstructure def D {{ param r : Result<Length, String> }}"
    );
    let module = compile_with_stdlib_helper(&source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for Result<Length, String> param; got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "D")
        .expect("D template must exist");

    let r_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "r")
        .expect("D must have a value cell named 'r'");

    let expected = Type::Applied {
        name: "Result".to_string(),
        args: vec![Type::length(), Type::String],
    };
    assert_eq!(
        r_cell.cell_type, expected,
        "Result<Length, String> must resolve to Applied{{\"Result\", [Length, String]}}, got {:?}",
        r_cell.cell_type
    );
}

/// INV-6: a NON-generic enum `Dir` used as `param d : Dir` still resolves to
/// `Type::Enum("Dir")` with no errors — the plain `resolve_enum_type` path is
/// unchanged by δ.
#[test]
fn annotation_non_generic_enum_cell_type_is_enum() {
    let source = r#"
enum Dir { In, Out }
structure def UseDir {
    param d : Dir
}
"#;
    let module = compile_with_stdlib_helper(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for non-generic Dir param; got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "UseDir")
        .expect("UseDir template must exist");

    let d_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "d")
        .expect("UseDir must have a value cell named 'd'");

    assert_eq!(
        d_cell.cell_type,
        Type::Enum("Dir".to_string()),
        "non-generic Dir must resolve to Type::Enum(\"Dir\"), got {:?}",
        d_cell.cell_type
    );
}

// ─── Arity enforcement ───────────────────────────────────────────────────────

/// A generic enum given too FEW type args (`Result<Length>`) must produce an
/// arity-mismatch diagnostic instead of silently building a wrong-arity Applied.
#[test]
fn annotation_generic_enum_too_few_args_errors() {
    let source = format!(
        "{RESULT_ENUM_SOURCE}\nstructure def D {{ param r : Result<Length> }}"
    );
    let module = compile_with_stdlib_helper(&source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected an arity-mismatch error for Result<Length> (expects 2 args); got none"
    );
    let has_arity_msg = errors
        .iter()
        .any(|e| e.message.contains("expects 2 type arguments") || e.message.contains("found 1"));
    assert!(
        has_arity_msg,
        "error must describe the arity mismatch; got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// A generic enum given too MANY type args (`Result<Length, String, Force>`) must
/// produce an arity-mismatch diagnostic.
#[test]
fn annotation_generic_enum_too_many_args_errors() {
    let source = format!(
        "{RESULT_ENUM_SOURCE}\nstructure def D {{ param r : Result<Length, String, Force> }}"
    );
    let module = compile_with_stdlib_helper(&source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected an arity-mismatch error for Result<Length,String,Force> (expects 2 args); got none"
    );
    let has_arity_msg = errors
        .iter()
        .any(|e| e.message.contains("expects 2 type arguments") || e.message.contains("found 3"));
    assert!(
        has_arity_msg,
        "error must describe the arity mismatch; got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// S3 — RED: binder substitution (S4 makes these GREEN)
// ═══════════════════════════════════════════════════════════════════════════════

/// (clean) `match r { Ok { value: v } => v + 1mm, Err { error: m } => 6mm }`
/// over `r : Result<Length, String>` → ZERO Error diagnostics after S4.
/// (mismatch) `... Ok { value: v } => v + 1N ...` → AT LEAST ONE Error (dimension mismatch).
///
/// RED after S2 / before S4: the binder `v` is typed `Type::TypeParam("T")` or
/// `Type::Error`, so `v + 1mm` may be suppressed (anti-cascade), causing the
/// mismatch assertion to fail.
#[test]
fn binder_substituted_clean_arm_no_errors() {
    let source = format!(
        r#"{RESULT_ENUM_SOURCE}
structure def TestClean {{
    param r : Result<Length, String>
    let bore = match r {{
        Ok {{ value: v }} => v + 1mm,
        Err {{ error: m }} => 6mm,
    }}
}}"#
    );
    let module = compile_with_stdlib_helper(&source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "clean arm (v + 1mm over Length binder) must produce no errors; got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// Dimension mismatch: `v + 1N` where `v : Length` → at least one error.
///
/// RED after S2 / before S4: unsubstituted binder type means `v + 1N` may not
/// produce the expected dimension error (anti-cascade suppresses).
#[test]
fn binder_substituted_mismatch_arm_has_errors() {
    let source = format!(
        r#"{RESULT_ENUM_SOURCE}
structure def TestMismatch {{
    param r : Result<Length, String>
    let bad = match r {{
        Ok {{ value: v }} => v + 1N,
        Err {{ error: m }} => 6mm,
    }}
}}"#
    );
    let module = compile_with_stdlib_helper(&source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "mismatch arm (v + 1N where v : Length) must produce at least one error; got none"
    );
}

/// INV-6: a NON-generic data-carrying enum `Boxed` with `param b : Boxed`
/// correctly types `v` as `Length` in `match b { Cell { value: v } => v + 1mm }`.
///
/// GREEN from the start (DCE ε already handles non-generic binders); regression pin.
#[test]
fn non_generic_enum_binder_clean() {
    let source = r#"
enum Boxed { Cell { value: Length } }
structure def UseBoxed {
    param b : Boxed
    let out = match b {
        Cell { value: v } => v + 1mm,
    }
}
"#;
    let module = compile_with_stdlib_helper(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "non-generic Boxed binder (v + 1mm) must produce no errors; got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// INV-6: `v + 1N` over a non-generic `Boxed { Cell { value: Length } }` binder
/// still produces a dimension mismatch error (DCE ε unchanged).
#[test]
fn non_generic_enum_binder_mismatch() {
    let source = r#"
enum Boxed { Cell { value: Length } }
structure def UseBoxedBad {
    param b : Boxed
    let bad = match b {
        Cell { value: v } => v + 1N,
    }
}
"#;
    let module = compile_with_stdlib_helper(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "mismatch arm (v + 1N where v : Length from non-generic Boxed) must error; got none"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// S5 — RED: exhaustiveness over Applied discriminant (S6 makes these GREEN)
// ═══════════════════════════════════════════════════════════════════════════════

/// (non-exhaustive) `match r { Ok { value: v } => v + 1mm }` over
/// `r : Result<Length, String>` → assert a non-exhaustiveness Error diagnostic.
///
/// RED after S4 / before S6: the exhaustiveness check keys on `Type::Enum` only,
/// so a `Type::Applied` discriminant skips the check silently.
#[test]
fn exhaustiveness_generic_enum_non_exhaustive_errors() {
    let source = format!(
        r#"{RESULT_ENUM_SOURCE}
structure def TestNonExh {{
    param r : Result<Length, String>
    let x = match r {{
        Ok {{ value: v }} => v + 1mm,
    }}
}}"#
    );
    let module = compile_with_stdlib_helper(&source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "non-exhaustive match on generic Result<L,S> must produce an error; got none"
    );
    // Robust token check: the error message must mention exhaustiveness.
    // We rely solely on the "exhaustive" token rather than also checking the
    // raw variant name ("Err"), which would accidentally match the word "Error".
    let has_exhaustive_msg = errors
        .iter()
        .any(|e| e.message.to_lowercase().contains("exhaustive"));
    assert!(
        has_exhaustive_msg,
        "error message must mention 'exhaustive' (e.g. 'non-exhaustive match'); got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// (exhaustive) `match r {{ Ok {{ value: v }} => v + 1mm, Err {{ error: m }} => 6mm }}`
/// → NO non-exhaustiveness error (only the match binder errors that may come from S3).
///
/// We can't easily separate "non-exhaustive" from other errors before S4, so this
/// assertion is that the number of errors does NOT include a missing-`Err` message.
/// After S6 this should produce zero errors total.
#[test]
fn exhaustiveness_generic_enum_exhaustive_no_missing_variant_error() {
    let source = format!(
        r#"{RESULT_ENUM_SOURCE}
structure def TestExh {{
    param r : Result<Length, String>
    let x = match r {{
        Ok {{ value: v }} => v + 1mm,
        Err {{ error: m }} => 6mm,
    }}
}}"#
    );
    let module = compile_with_stdlib_helper(&source);

    // There must be no "non-exhaustive" or "missing variant" errors.
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
        "exhaustive match on generic Result<L,S> must produce no non-exhaustiveness error; got: {:?}",
        exhaustive_errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// INV-6: a non-generic `enum Dir { In, Out }` with only `In =>` arm still emits
/// the non-exhaustive error (DCE ε/D4 unchanged for non-generic enums).
#[test]
fn exhaustiveness_non_generic_enum_still_checked() {
    let source = r#"
enum Dir { In, Out }
structure def UseDir {
    param d : Dir
    let x = match d {
        In => 1mm,
    }
}
"#;
    let module = compile_with_stdlib_helper(source);

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "non-exhaustive match on non-generic Dir must error; got none"
    );
    let has_exhaustive_msg = errors
        .iter()
        .any(|e| e.message.to_lowercase().contains("exhaustive"));
    assert!(
        has_exhaustive_msg,
        "error must mention 'exhaustive' (e.g. 'non-exhaustive match'); got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}
