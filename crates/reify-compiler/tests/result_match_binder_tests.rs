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

use reify_compiler::CompiledModule;
use reify_core::{Severity, Type};
use reify_ir::{CompiledExpr, CompiledExprKind, CompiledPattern};
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
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "exhaustive match (Ok + Err arms) over the PRELUDE Result<Length, String> \
         must produce no errors at all; got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    let exhaustive_errors: Vec<_> = errors
        .iter()
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
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "exhaustive match (Ok + wildcard) over the PRELUDE Result<Length, String> \
         must produce no errors at all; got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
    let exhaustive_errors: Vec<_> = errors
        .iter()
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

// ═══════════════════════════════════════════════════════════════════════════
// E-axis binder: `Err { error: msg }` types `msg` as the substituted `E`
// (String) — task 4036's distinct, previously-uncovered signal (δ #4032 only
// exercised the T-axis binder).
//
// Reify has no arithmetic operand-kind guard for `+` (infer_binop_type
// returns left.clone() for Add/Sub), so `String + Length` silently types as
// `String` with no diagnostic of its own; nor is there a match-arm-type-
// consistency check (the match RESULT type is taken from the FIRST arm
// only). So the `msg : String` binder is only OBSERVABLE where the String
// match-result reaches a checked position. Ordering the `Err` arm FIRST
// makes the match result type String, which a `let bore : Length =`
// annotation-vs-initializer check rejects — the faithful realization of
// "msg + 1mm is a type error" for this binder. See helpers below for a
// direct, annotation-independent pin of the compiled arm body's type.
// ═══════════════════════════════════════════════════════════════════════════

/// (a, negative) `Err { error: msg } => msg + 1mm` ordered FIRST so the match
/// RESULT type is `String`, under a `let bore : Length =` annotation — the
/// declared-vs-initializer check must reject the `String` initializer.
/// Empirically: "let binding 'bore' declared `Scalar[m]` but its initializer
/// evaluates to `String`; declared type and initializer type must agree".
#[test]
fn err_binder_typed_string_first_arm_arith_under_length_annotation_errors() {
    let source = r#"
structure def Widget {
    param r : Result<Length, String>
    let bore : Length = match r {
        Err { error: msg } => msg + 1mm,
        Ok { value: v } => v,
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
        "Err {{ error: msg }} => msg + 1mm ordered first (match result type String) \
         under `let bore : Length` must produce at least one error; got none"
    );
    let has_string_msg = errors.iter().any(|e| e.message.contains("String"));
    assert!(
        has_string_msg,
        "error message must mention 'String' (the msg binder's substituted E type); got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// Same shape as above but the Err arm body is the BARE binder (`=> msg`,
/// no `+ 1mm`) — proves it is the binder itself that is typed `String`, not
/// an artifact of the `+ 1mm` arithmetic.
#[test]
fn err_binder_typed_string_first_arm_bare_under_length_annotation_errors() {
    let source = r#"
structure def Widget {
    param r : Result<Length, String>
    let bore : Length = match r {
        Err { error: msg } => msg,
        Ok { value: v } => v,
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
        "Err {{ error: msg }} => msg (bare binder) ordered first under \
         `let bore : Length` must produce at least one error; got none"
    );
    let has_string_msg = errors.iter().any(|e| e.message.contains("String"));
    assert!(
        has_string_msg,
        "error message must mention 'String' (the bare msg binder's substituted E type); got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );
}

/// (b, positive) regression guard alongside (a): a body that genuinely
/// evaluates to `String` (`Ok` arm literal `"ok"`, `Err` arm binder `msg`)
/// under a matching `let label : String =` annotation must check clean.
#[test]
fn err_binder_typed_string_under_matching_string_annotation_checks_clean() {
    let source = r#"
structure def Widget {
    param r : Result<Length, String>
    let label : String = match r {
        Ok { value: v } => "ok",
        Err { error: msg } => msg,
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
        "Ok {{ value: v }} => \"ok\", Err {{ error: msg }} => msg under `let label : String` \
         must produce no errors; got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );

    let expr = widget_value_cell(&module, "label");
    assert_eq!(
        expr.result_type,
        Type::String,
        "'label' should be typed Type::String, got {:?}",
        expr.result_type
    );
}

/// (c, robust direct pin) Independent of any downstream annotation check:
/// compile `let probe = match r { Ok { value: v } => v, Err { error: msg } => msg }`
/// and inspect the compiled `Match` arms directly. The `Err` arm's body
/// (`msg`) must be typed `Type::String`; the `Ok` arm's body (`v`) must be
/// typed `Type::length()` (T-axis regression pin, same match expression).
#[test]
fn direct_inspection_err_arm_body_is_string_and_ok_arm_body_is_length() {
    let source = r#"
structure def Widget {
    param r : Result<Length, String>
    let probe = match r {
        Ok { value: v } => v,
        Err { error: msg } => msg,
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
        "probe match (Ok => v, Err => msg) must produce no errors; got: {:?}",
        errors.iter().map(|e| &e.message).collect::<Vec<_>>()
    );

    let expr = widget_value_cell(&module, "probe");
    let err_arm = find_arm_by_tag(expr, "Err");
    assert_eq!(
        err_arm.body.result_type,
        Type::String,
        "the Err {{ error: msg }} arm body ('msg') should be typed Type::String \
         (E substituted through the PRELUDE Result), got {:?}",
        err_arm.body.result_type
    );

    let ok_arm = find_arm_by_tag(expr, "Ok");
    assert_eq!(
        ok_arm.body.result_type,
        Type::length(),
        "the Ok {{ value: v }} arm body ('v') should be typed Type::length() \
         (T substituted through the PRELUDE Result), got {:?}",
        ok_arm.body.result_type
    );
}

// ─── helpers (mirror result_prelude_enum_tests.rs's widget_value_cell) ─────

/// Locate the compiled default/init expr of the named value cell (`param` or
/// `let`) on the `Widget` structure's `TopologyTemplate`.
fn widget_value_cell<'a>(module: &'a CompiledModule, member: &str) -> &'a CompiledExpr {
    let widget = module
        .templates
        .iter()
        .find(|t| t.name == "Widget")
        .expect("Widget template should be present in module.templates");
    let cell = widget
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("Widget should declare a '{member}' value cell"));
    cell.default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("'{member}' value cell should carry a compiled default_expr"))
}

/// Find the arm of a compiled `Match` expression whose pattern tag name is
/// `tag` (e.g. `"Ok"` / `"Err"`) and return it. Panics if `expr` is not a
/// `CompiledExprKind::Match` or no arm carries that tag.
fn find_arm_by_tag<'a>(
    expr: &'a CompiledExpr,
    tag: &str,
) -> &'a reify_ir::CompiledMatchArm {
    match &expr.kind {
        CompiledExprKind::Match { arms, .. } => arms
            .iter()
            .find(|arm| {
                arm.patterns
                    .iter()
                    .any(|p| p.tag_name() == Some(tag) && matches!(p, CompiledPattern::VariantBind { .. }))
            })
            .unwrap_or_else(|| {
                panic!(
                    "no VariantBind arm tagged '{tag}' found; arms: {:?}",
                    arms.iter().map(|a| &a.patterns).collect::<Vec<_>>()
                )
            }),
        other => panic!("expected CompiledExprKind::Match {{ .. }}, got {:?}", other),
    }
}
