//! End-to-end behaviour of the `UnresolvedFunction` warning (task #5371).
//!
//! # The hole this closes
//!
//! `expr.rs`'s `NoUserFunctions` ladder ends in a *terminal first-arg
//! fallback*: a callee no ladder arm claims is typed as its first argument's
//! type. Because the fallback is **open-world**, a name that exists nowhere —
//! not in any compiler classification family, not in `eval_builtin`'s dispatch
//! chain, not as a user or stdlib `fn` — compiles with **zero diagnostics** and
//! silently adopts its argument's type. The 5371 observation was
//! `line(point3(1mm,2mm,3mm), point3(4mm,5mm,6mm))`: `line` is not a builtin
//! (verified — it appears in no compiler slice, no stdlib eval arm and no
//! stdlib `.ri` declaration), yet the call type-checked clean.
//!
//! `reify_compiler::is_known_builtin` supplies the missing closed-world
//! membership oracle; this binary pins what the compiler does with it.
//!
//! # Warn-mode-first — typing is deliberately UNCHANGED
//!
//! Every assertion below pairs "a warning is emitted" with "the cell's type is
//! exactly what it was before". That is the ratified fail-closed posture: the
//! diagnostic is the only new observable in #5371, so no corpus can break on
//! it. #5997 flips the severity to Error behind a break-glass knob, and #6014
//! (registry ω) deletes the fallback itself. A test here that asserted a *new*
//! type would be pre-empting both.
//!
//! RED until `DiagnosticCode::UnresolvedFunction` exists in `reify-core` and
//! the fallback in `expr.rs` emits it.

use reify_core::{DimensionVector, Severity, Type};
use reify_test_support::{compile_source_with_stdlib, get_let_expr_in};

/// Every `UnresolvedFunction`-coded diagnostic in `module`, in source order.
///
/// Matches on the CODE, never on message text: the message is explicitly not a
/// stable contract (`diagnostics.rs` says so for every coded diagnostic), and a
/// text match would make this file break on rewording rather than on behaviour.
fn unresolved_function_diags(
    module: &reify_compiler::CompiledModule,
) -> Vec<&reify_core::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(reify_core::DiagnosticCode::UnresolvedFunction))
        .collect()
}

/// Every Error-severity diagnostic message in `module`, in source order.
fn errors(module: &reify_compiler::CompiledModule) -> Vec<&str> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| d.message.as_str())
        .collect()
}

/// `Scalar<LENGTH>` — what `2.5mm` types as, and therefore what the first-arg
/// fallback hands the enclosing call.
fn scalar_length() -> Type {
    Type::Scalar {
        dimension: DimensionVector::LENGTH,
    }
}

// ---------------------------------------------------------------------------
// (a) + (b) the core contract: one coded Warning, no Error, type unchanged
// ---------------------------------------------------------------------------

const UNKNOWN_CALLEE: &str = r#"
    structure UnknownCallee {
        let x = definitely_not_a_builtin(2.5mm)
    }
"#;

/// A callee that exists nowhere produces EXACTLY ONE `UnresolvedFunction`
/// diagnostic, at `Severity::Warning`.
///
/// "Exactly one" is the load-bearing half. The obvious wrong implementation
/// emits from every ladder arm that fails to claim the name, and the second
/// obvious one double-reports alongside the pre-existing zero-arg warning
/// (pinned separately in `zero_arg_unknown_callee_is_not_double_diagnosed`).
#[test]
fn unknown_callee_emits_exactly_one_unresolved_function_warning() {
    let module = compile_source_with_stdlib(UNKNOWN_CALLEE);
    let diags = unresolved_function_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly one UnresolvedFunction diagnostic, got {}: {:?}",
        diags.len(),
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
    assert_eq!(
        diags[0].severity,
        Severity::Warning,
        "#5371 is warn-mode-first; the Error flip is #5997's to make"
    );
}

/// The unknown call must not turn into a compile ERROR. This is what makes the
/// change corpus-safe: `reify check` keeps exiting 0 on today's sources, and
/// only the diagnostic stream changes.
#[test]
fn unknown_callee_produces_no_errors() {
    let module = compile_source_with_stdlib(UNKNOWN_CALLEE);
    assert_eq!(
        errors(&module),
        Vec::<&str>::new(),
        "warn-mode-first: an unresolved callee must not fail the build in #5371"
    );
}

/// The cell's type is UNCHANGED — still the first argument's `Scalar<LENGTH>`.
///
/// Deliberately asserts the OLD (and, for an unknown name, meaningless) type.
/// The fallback still types the call in #5371; only #6014 may stop it. If this
/// test ever starts failing because the type became `Type::Error`, that is a
/// scope breach into #5997/#6014, not a fix.
#[test]
fn unknown_callee_result_type_is_unchanged_by_the_warning() {
    let module = compile_source_with_stdlib(UNKNOWN_CALLEE);
    assert_eq!(
        get_let_expr_in(&module, "UnknownCallee", "x").result_type,
        scalar_length(),
        "typing must be untouched by #5371 — the fallback still adopts arg0"
    );
}

/// The diagnostic is actionable: it carries a label anchored at a real source
/// span, and it names the offending callee somewhere the user can see it.
///
/// The span check is `!is_empty()` rather than an exact offset because the
/// exact `expr.span` of a call is an `expr.rs` implementation detail; what the
/// user needs is that the label points at source rather than at the synthetic
/// empty span that unanchored diagnostics carry.
#[test]
fn unresolved_function_diagnostic_is_anchored_and_names_the_callee() {
    let module = compile_source_with_stdlib(UNKNOWN_CALLEE);
    let diags = unresolved_function_diags(&module);
    let diag = diags
        .first()
        .expect("no UnresolvedFunction diagnostic to inspect");

    assert!(
        !diag.labels.is_empty(),
        "diagnostic must carry a label so the IDE can underline the call"
    );
    assert!(
        !diag.labels[0].span.is_empty(),
        "label span must be anchored at the call site, not the empty span"
    );

    let named_anywhere = diag.message.contains("definitely_not_a_builtin")
        || diag
            .labels
            .iter()
            .any(|l| l.message.contains("definitely_not_a_builtin"));
    assert!(
        named_anywhere,
        "the offending callee must be named in the message or a label; got \
         message {:?} labels {:?}",
        diag.message,
        diag.labels.iter().map(|l| &l.message).collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// (c) the original #5371 observation
// ---------------------------------------------------------------------------

/// The verbatim 5371 observation. `line` is genuinely nonexistent — it appears
/// in no compiler classification slice, no `reify-stdlib` eval arm, and no
/// stdlib `.ri` declaration — and before this task it compiled with ZERO
/// diagnostics, silently adopting `point3`'s type.
#[test]
fn the_original_line_observation_now_warns() {
    let module = compile_source_with_stdlib(
        r#"
        structure LineObservation {
            let l = line(point3(1mm, 2mm, 3mm), point3(4mm, 5mm, 6mm))
        }
    "#,
    );
    assert_eq!(
        unresolved_function_diags(&module).len(),
        1,
        "the 5371 observation must now surface exactly one warning"
    );
    assert_eq!(
        errors(&module),
        Vec::<&str>::new(),
        "still warn-mode: the observation must not become a build failure"
    );
}

// ---------------------------------------------------------------------------
// (d) the closed world must not cry wolf
// ---------------------------------------------------------------------------

/// No KNOWN name warns — across every route by which a name can be known.
///
/// Without this, the trivially-passing implementation (warn unconditionally at
/// the fallback) would satisfy every test above while making the compiler
/// unusable on real sources. Each cell targets a different membership route:
///
/// * `sqrt` / `point3` / `volume` — ordinary classification family slices, the
///   bulk of the closed world;
/// * `mod` — `FIRST_ARG_TYPED_NAMES`, the type-preserving allowlist. That
///   family is membership-ONLY: it adds no ladder arm and resolves no type, so
///   suppressing this warning is its *entire* observable effect and a
///   compile-level test is the only place it can be exercised;
/// * `floor` — `EVAL_DEFERRED_BUILTIN_NAMES`, the manifest of eval-dispatchable
///   names still deliberately left to the fallback. A false warning on a
///   manifested name is the precise failure the manifest exists to prevent.
#[test]
fn known_builtins_never_emit_an_unresolved_function_warning() {
    let module = compile_source_with_stdlib(
        r#"
        structure KnownCallees {
            let root      = sqrt(4.0)
            let modulo    = mod(7, 3)
            let floored   = floor(2.5)
            let position  = point3(1mm, 2mm, 3mm)
            let solid     = box(1mm, 2mm, 3mm)
            let vol       = volume(solid)
        }
    "#,
    );
    let diags = unresolved_function_diags(&module);
    assert!(
        diags.is_empty(),
        "known builtins must not warn; got {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
