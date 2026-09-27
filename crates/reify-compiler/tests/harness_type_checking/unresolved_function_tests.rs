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
use reify_test_support::{compile_source_with_stdlib, errors_only, get_let_expr_in, warnings_only};

/// Every diagnostic in `module` carrying `code`, in source order.
///
/// Matches on the CODE, never on message text: the message is explicitly not a
/// stable contract (`diagnostics.rs` says so for every coded diagnostic), and a
/// text match would make this file break on rewording rather than on behaviour.
fn diags_with_code(
    module: &reify_compiler::CompiledModule,
    code: reify_core::DiagnosticCode,
) -> Vec<&reify_core::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(code))
        .collect()
}

/// Every `UnresolvedFunction`-coded diagnostic in `module`, in source order.
fn unresolved_function_diags(
    module: &reify_compiler::CompiledModule,
) -> Vec<&reify_core::Diagnostic> {
    diags_with_code(module, reify_core::DiagnosticCode::UnresolvedFunction)
}

/// Every Error-severity diagnostic MESSAGE in `module`, in source order.
///
/// A message-level view over `reify_test_support::errors_only`, kept local only
/// because the assertions below compare against `Vec::<&str>::new()`, whose
/// failure dump reads far better than a `Vec<&Diagnostic>`.
fn errors(module: &reify_compiler::CompiledModule) -> Vec<&str> {
    errors_only(module)
        .into_iter()
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

// ---------------------------------------------------------------------------
// the zero-arg interaction: two warnings, mutually exclusive
// ---------------------------------------------------------------------------

/// Does `module` carry the legacy pre-#5371 zero-arg warning?
///
/// Matched on TEXT, unavoidably: that warning predates the coded-diagnostic
/// convention and carries `code: None` (verified — it is emitted bare in
/// `expr.rs`'s fallback), so there is no code to match on. #6014 deletes it
/// outright, at which point this helper goes with it.
fn has_legacy_zero_arg_warning(module: &reify_compiler::CompiledModule) -> bool {
    module.diagnostics.iter().any(|d| {
        d.message
            .contains("cannot infer return type of zero-arg function")
    })
}

/// An UNKNOWN zero-arg callee is diagnosed ONCE, as `UnresolvedFunction` — the
/// legacy zero-arg warning must not also fire.
///
/// Both warnings live in the same fallback and, before this step, both fired:
/// the user saw "unresolved function: f" immediately followed by "cannot infer
/// return type of zero-arg function 'f', defaulting to Real" — two lines for
/// one defect, the second of which is noise once the first has said the name
/// does not exist. Measured on the pre-step tree: exactly 2 diagnostics.
///
/// The two are complements, not a hierarchy: "I do not know this name" and "I
/// know this name but cannot infer its return type without arguments" cannot
/// both be true of the same call.
#[test]
fn zero_arg_unknown_callee_is_not_double_diagnosed() {
    let module = compile_source_with_stdlib(
        r#"
        structure ZeroArgUnknown {
            let x = definitely_not_a_builtin()
        }
    "#,
    );

    assert_eq!(
        unresolved_function_diags(&module).len(),
        1,
        "an unknown zero-arg callee is still an unresolved function"
    );
    assert!(
        !has_legacy_zero_arg_warning(&module),
        "the legacy zero-arg warning must NOT also fire — the name is unknown, \
         so 'cannot infer its return type' is noise on top of 'no such \
         function'; got {:?}",
        module
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        warnings_only(&module).len(),
        1,
        "exactly one warning total; got {:?}",
        warnings_only(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// …and the typing of that zero-arg call is UNCHANGED: still
/// `Type::dimensionless_scalar()` (the "defaulting to Real" behaviour), even
/// though the warning that used to announce it is now suppressed.
///
/// Suppressing a warning must not quietly change what the compiler infers.
#[test]
fn zero_arg_unknown_callee_result_type_is_unchanged() {
    let module = compile_source_with_stdlib(
        r#"
        structure ZeroArgUnknownType {
            let x = definitely_not_a_builtin()
        }
    "#,
    );
    assert_eq!(
        get_let_expr_in(&module, "ZeroArgUnknownType", "x").result_type,
        Type::dimensionless_scalar(),
        "the fallback still defaults a zero-arg call to Real in #5371"
    );
}

/// The converse: a KNOWN zero-arg name that is merely unregistered keeps the
/// legacy warning and gains NO `UnresolvedFunction`.
///
/// `world()` (`reify-stdlib/src/mechanism.rs:42`) is the case that exists: it
/// is a genuine zero-arg builtin — its eval arm rejects any argument — and it
/// sits in `EVAL_DEFERRED_BUILTIN_NAMES` awaiting #6007 (registry τ5). So the
/// compiler really cannot infer its return type, and really does know the
/// name. Exactly one warning, and it is the legacy one.
///
/// Without this direction, "emit UnresolvedFunction instead of the zero-arg
/// warning" could be implemented by deleting the zero-arg warning outright,
/// silently dropping a true diagnostic for every registered zero-arg builtin.
#[test]
fn zero_arg_known_but_unregistered_callee_keeps_only_the_legacy_warning() {
    let module = compile_source_with_stdlib(
        r#"
        structure ZeroArgKnown {
            let w = world()
        }
    "#,
    );

    assert!(
        unresolved_function_diags(&module).is_empty(),
        "`world` is in EVAL_DEFERRED_BUILTIN_NAMES, so it is inside the closed \
         world and must not be reported unresolved"
    );
    assert!(
        has_legacy_zero_arg_warning(&module),
        "the legacy zero-arg warning is still TRUE for a known-but-unregistered \
         zero-arg builtin and must be preserved; got {:?}",
        module
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        warnings_only(&module).len(),
        1,
        "exactly one warning total; got {:?}",
        warnings_only(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// task item 4: the arg-aware arms that fall through on SHAPE, not on NAME
// ---------------------------------------------------------------------------
//
// Three ladder arms are *arg-aware*: they read the compiled argument list and
// return `None` — deliberately, for anti-cascade — when the name is theirs but
// the arg SHAPE is not what the family expects. `None` from an arg-aware arm is
// indistinguishable, at the call site, from `None` meaning "not my name", so
// the call slides all the way to the terminal first-arg fallback and is typed
// from arg0. Measured on the pre-step tree: `single(42)`, `sample(42, 7)` and
// friends compile with ZERO diagnostics and adopt `Int`.
//
// That is silent graceful degradation of a call the compiler *did* recognise —
// a strictly worse failure than the unknown-name case above, because the user
// wrote a real builtin and got no hint that its contract was missed.
//
// As with `UnresolvedFunction`, typing stays UNCHANGED: every test below pairs
// the new warning with the old inferred type.

/// Every `BuiltinArgShapeUnrecognized`-coded diagnostic in `module`, in source
/// order.
fn arg_shape_diags(module: &reify_compiler::CompiledModule) -> Vec<&reify_core::Diagnostic> {
    diags_with_code(
        module,
        reify_core::DiagnosticCode::BuiltinArgShapeUnrecognized,
    )
}

/// Is `text` present in the diagnostic's message or in any of its labels?
fn mentions(diag: &reify_core::Diagnostic, text: &str) -> bool {
    diag.message.contains(text) || diag.labels.iter().any(|l| l.message.contains(text))
}

/// (a) list-helper: `single(42)` — `single` IS a builtin, but `single` wants a
/// `List<T>` and got an `Int`, so `infer_list_helper_return_type`
/// (`list_helpers.rs`) returns `None` and the call rides the fallback.
///
/// Exactly one coded Warning, zero Errors, and the cell keeps the type the
/// fallback gave it (`Int` — arg0's type, measured pre-step).
#[test]
fn mis_shaped_list_helper_call_warns_once_without_changing_typing() {
    let module = compile_source_with_stdlib(
        r#"
        structure MisShapedListHelper {
            let x = single(42)
        }
    "#,
    );

    let diags = arg_shape_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly one BuiltinArgShapeUnrecognized diagnostic, got {:?}",
        module
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        diags[0].severity,
        Severity::Warning,
        "#5371 is warn-mode-first; #6002's sibling E_BuiltinArgShape owns the poison"
    );
    assert!(
        mentions(diags[0], "single"),
        "the diagnostic must name the builtin; got {:?}",
        diags[0].message
    );
    assert!(
        mentions(diags[0], "List"),
        "the diagnostic must name the EXPECTED arg shape — that is the whole \
         actionable content, since the user already knows what they wrote; got \
         message {:?} labels {:?}",
        diags[0].message,
        diags[0].labels.iter().map(|l| &l.message).collect::<Vec<_>>()
    );
    assert!(
        !diags[0].labels.is_empty() && !diags[0].labels[0].span.is_empty(),
        "the diagnostic must be anchored at the call site"
    );

    assert_eq!(
        errors(&module),
        Vec::<&str>::new(),
        "warn-mode-first: a mis-shaped call must not fail the build in #5371"
    );
    assert_eq!(
        get_let_expr_in(&module, "MisShapedListHelper", "x").result_type,
        Type::Int,
        "typing is UNCHANGED — the fallback still adopts arg0's Int"
    );
}

/// (c) field op: `sample(42, 7)` — `sample` IS a builtin, but arg0 is not a
/// `Field`, so `field_op_result_type` (`units.rs`) returns `None`.
#[test]
fn mis_shaped_field_op_call_warns_once_without_changing_typing() {
    let module = compile_source_with_stdlib(
        r#"
        structure MisShapedFieldOp {
            let x = sample(42, 7)
        }
    "#,
    );

    let diags = arg_shape_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly one BuiltinArgShapeUnrecognized diagnostic, got {:?}",
        module
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert_eq!(diags[0].severity, Severity::Warning);
    assert!(
        mentions(diags[0], "sample"),
        "the diagnostic must name the builtin; got {:?}",
        diags[0].message
    );
    assert!(
        mentions(diags[0], "Field"),
        "the diagnostic must name the expected arg shape; got message {:?} \
         labels {:?}",
        diags[0].message,
        diags[0].labels.iter().map(|l| &l.message).collect::<Vec<_>>()
    );

    assert_eq!(errors(&module), Vec::<&str>::new());
    assert_eq!(
        get_let_expr_in(&module, "MisShapedFieldOp", "x").result_type,
        Type::Int,
        "typing is UNCHANGED — the fallback still adopts arg0's Int"
    );
}

/// A mis-shaped call to a KNOWN builtin must NOT also be reported unresolved.
///
/// The two codes answer different questions — "I have never heard of this name"
/// versus "I know this name and you called it wrong" — and only the second is
/// true here. All three families are inside `is_known_builtin`'s closed world
/// (they contribute production slices to the union), so a `UnresolvedFunction`
/// on any of them would be a membership bug in the union itself.
#[test]
fn mis_shaped_known_builtins_are_never_reported_unresolved() {
    let module = compile_source_with_stdlib(
        r#"
        structure MisShapedNotUnresolved {
            let a = single(42)
            let b = flat_map(42, 7)
            let c = generate(3, 7)
            let d = sample(42, 7)
            let e = gradient(42)
        }
    "#,
    );
    assert!(
        unresolved_function_diags(&module).is_empty(),
        "these are all real builtin names; got {:?}",
        unresolved_function_diags(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// The NEGATIVE direction: a WELL-shaped call to each arg-aware family is
/// claimed by its own ladder arm and never reaches the fallback, so it must
/// emit no shape warning at all.
///
/// This is the test that stops the trivially-passing implementation ("warn
/// whenever the name is in one of the three families"), which would fire on
/// every correct `sample`/`gradient`/`single` in the corpus.
#[test]
fn well_shaped_arg_aware_calls_emit_no_shape_warning() {
    let module = compile_source_with_stdlib(
        r#"
        structure WellShaped {
            let f      = fn_field(|p| 2.0 * p)
            let g      = gradient(f)
            let s      = sample(f, 1.0)
            let r      = restrict(f, 1.0)
            let fs     = from_samples([1.0], [2.0], 0)
            let one    = single([1.0, 2.0])
            let flat   = flat_map([1.0, 2.0], |v| [v])
            let gen    = generate(3, |i| i)
        }
    "#,
    );
    assert!(
        arg_shape_diags(&module).is_empty(),
        "well-shaped calls must not warn; got {:?}",
        arg_shape_diags(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert_eq!(errors(&module), Vec::<&str>::new());
}

/// (b) affine algebra — a PREMISE GUARD, not a positive case.
///
/// `affine_map_algebra_result_type` (`units.rs`) is arg-aware like the other
/// two, but NO member of `AFFINE_ALGEBRA_NAMES` can actually reach the terminal
/// fallback, so the family has no observable mis-shape behaviour to pin.
/// Measured, name by name, on this tree:
///
/// * `affine_compose` / `affine_inverse` — the resolver answers `Some` from the
///   NAME alone (`AffineMap(3)` / `Option<AffineMap(3)>`); there is no arg gate
///   to miss.
/// * `determinant` — the resolver returns `None` for a non-`AffineMap` arg0 *on
///   purpose*, and the LATER `is_math_typed_fn` arm then claims it. That
///   hand-off IS the matrix-determinant behaviour, documented on
///   `AFFINE_ALGEBRA_NAMES`.
/// * `affine_apply` — `is_geometry_function` sits EARLIER in the ladder and
///   claims it before the affine resolver is consulted at all.
///
/// The fallback still keys on `is_affine_map_algebra_name` alongside the other
/// two families: the arm is defense-in-depth against a ladder reorder or a
/// slice edit that removes one of the two shadows above. This test is what
/// makes that reorder visible — if it ever starts failing, the shadowing
/// documented on `AFFINE_ALGEBRA_NAMES` has been broken and the note there is
/// now false.
#[test]
fn affine_algebra_names_never_reach_the_terminal_fallback() {
    let module = compile_source_with_stdlib(
        r#"
        structure AffineShadowed {
            let a = affine_compose(42, 7)
            let b = affine_inverse(42)
            let c = determinant(42)
            let d = affine_apply(42, 7)
        }
    "#,
    );
    assert!(
        arg_shape_diags(&module).is_empty(),
        "every AFFINE_ALGEBRA_NAMES member is shadowed by another ladder arm, \
         so none can reach the fallback; got {:?}",
        arg_shape_diags(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        unresolved_function_diags(&module).is_empty(),
        "…and they are all known names"
    );
}

/// A zero-arg call to a mis-shapeable KNOWN builtin is diagnosed ONCE.
///
/// `single()` reaches the fallback with an empty argument list, so BOTH the new
/// shape warning and the legacy zero-arg warning have a claim on it. Measured
/// pre-step: exactly one diagnostic, the legacy `"cannot infer return type of
/// zero-arg function 'single'"`.
///
/// The shape warning wins, on the same complements-not-a-hierarchy reasoning
/// that `zero_arg_unknown_callee_is_not_double_diagnosed` applies to
/// `UnresolvedFunction`: "you called `single` with the wrong argument shape" is
/// the actionable statement, and "I could not infer the return type" is the
/// mechanical consequence of it. One defect, one line.
#[test]
fn zero_arg_mis_shaped_known_builtin_is_not_double_diagnosed() {
    let module = compile_source_with_stdlib(
        r#"
        structure ZeroArgMisShaped {
            let x = single()
        }
    "#,
    );

    assert_eq!(
        arg_shape_diags(&module).len(),
        1,
        "a zero-arg `single()` is a mis-shaped call to a known builtin"
    );
    assert!(
        !has_legacy_zero_arg_warning(&module),
        "the legacy zero-arg warning must NOT also fire; got {:?}",
        module
            .diagnostics
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        warnings_only(&module).len(),
        1,
        "exactly one warning total; got {:?}",
        warnings_only(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        get_let_expr_in(&module, "ZeroArgMisShaped", "x").result_type,
        Type::dimensionless_scalar(),
        "typing is UNCHANGED — a zero-arg fallback call still defaults to Real"
    );
}

// ---------------------------------------------------------------------------
// (f) FALSE POSITIVES: the warning must not fire for a name this module DECLARES
// ---------------------------------------------------------------------------
//
// `is_known_builtin` answers "is this a BUILTIN?", and it answers correctly:
// a user `fn` is not a builtin. The fallback's defect is that it read that
// `false` as "exists nowhere", which only holds when the caller's function
// table was COMPLETE. It is not complete inside a `fn` body:
// `compile_builder/functions_phase.rs` compiles each body against
// `&ctx.functions`, the user-only table that GROWS as the loop walks source
// order, and only builds the merged `ctx.resolution_functions` afterwards.
// So a call to a later-declared sibling yields `OverloadResolution::
// NoUserFunctions` and lands on the terminal fallback with a name that is
// perfectly real. Entity bodies, compiled after that merge, never see this.
//
// Each route below was reproduced first-hand before being written down. The
// CONTROLS in section (g) bound the fix: they are the routes that are already
// correct, and a fix that silences them too has become a blanket disable.
//
// Typing is NOT in question here — these tests remove a diagnostic and assert
// nothing about types. `forward_reference_typing_is_byte_identical` in section
// (h) is where the type answer is pinned.

/// Route (1): a `fn` body calling a LATER-declared sibling `fn`.
///
/// Reversing the two declarations compiles clean (control (a)), which is the
/// proof that the trigger is SOURCE ORDER and not a real lookup miss.
#[test]
fn fn_body_calling_a_later_declared_sibling_is_not_unresolved() {
    let module = compile_source_with_stdlib(
        r#"
        pub fn a(x: Real) -> Real { b(x) }
        pub fn b(x: Real) -> Real { x }
    "#,
    );

    assert_eq!(
        unresolved_function_diags(&module).len(),
        0,
        "'b' is declared in this very module; got {:?}",
        unresolved_function_diags(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// Route (2): a mutually-referential `fn` pair.
///
/// The load-bearing route. Unlike (1) there is NO declaration order that makes
/// this quiet, so the warning is unfixable by the user — it tells them to
/// remove a call to a function they can see two lines below.
#[test]
fn mutually_referential_fn_pair_is_not_unresolved() {
    let module = compile_source_with_stdlib(
        r#"
        pub fn even(n: Int) -> Int { odd(n) }
        pub fn odd(n: Int) -> Int { even(n) }
    "#,
    );

    assert_eq!(
        unresolved_function_diags(&module).len(),
        0,
        "neither half of a mutually-referential pair can be reordered into \
         scope; got {:?}",
        unresolved_function_diags(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// Route (3): a structure CONSTRUCTOR called from a TRAIT STATIC fn body.
///
/// Distinct cause from (1)/(2): `compile_builder/traits_phase.rs` passes `None`
/// for `compile_function`'s `prelude_template_registry` ("v1: no prelude
/// template registry for static fn bodies"), so the constructor is never
/// claimed and rides the fallback. Control (c) pins that the SAME call from a
/// regular fn body is already clean, which is what confines this route to the
/// trait-static site.
#[test]
fn struct_constructor_in_a_trait_static_fn_body_is_not_unresolved() {
    let module = compile_source_with_stdlib(
        r#"
        structure Widget { let w: Length = 1mm }
        trait Maker { fn build() -> Real { let x = Widget(w: 2mm); 1.0 } }
    "#,
    );

    assert_eq!(
        unresolved_function_diags(&module).len(),
        0,
        "'Widget' is a declared structure in this module; got {:?}",
        unresolved_function_diags(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// (g) CONTROLS: the routes that bound the fix
// ---------------------------------------------------------------------------

/// Control (a): reversing route (1)'s declaration order is — and stays — clean.
///
/// Pinned as its own test rather than as a comment on route (1), because it is
/// the assertion that makes "the trigger is source order" falsifiable.
#[test]
fn fn_body_calling_an_earlier_declared_sibling_stays_clean() {
    let module = compile_source_with_stdlib(
        r#"
        pub fn b(x: Real) -> Real { x }
        pub fn a(x: Real) -> Real { b(x) }
    "#,
    );

    assert_eq!(
        unresolved_function_diags(&module).len(),
        0,
        "an earlier sibling resolves normally; got {:?}",
        unresolved_function_diags(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// Control (b): a genuinely-undeclared callee inside a `fn` body STILL warns.
///
/// The single most important control. The cheap wrong fix — suppress
/// `UnresolvedFunction` inside fn bodies — passes every test in section (f)
/// and fails this one, which is exactly why it is here.
#[test]
fn genuinely_undeclared_callee_in_a_fn_body_still_warns() {
    let module = compile_source_with_stdlib(
        r#"
        pub fn a(x: Real) -> Real { definitely_not_a_builtin(x) }
    "#,
    );

    assert_eq!(
        unresolved_function_diags(&module).len(),
        1,
        "the closed world must still be closed inside fn bodies; got {:?}",
        unresolved_function_diags(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// Control (c): a struct constructor from a REGULAR fn body is clean in BOTH
/// declaration orders — `functions_phase` passes `Some(&merged_registry)`.
///
/// This is what confines route (3) to `traits_phase`'s `None`. A fix that
/// needs to widen past that site has misdiagnosed the cause.
#[test]
fn struct_constructor_in_a_regular_fn_body_stays_clean_in_both_orders() {
    for (order, source) in [
        (
            "structure first",
            r#"
            structure Widget { let w: Length = 1mm }
            pub fn make() -> Real { let x = Widget(w: 2mm); 1.0 }
        "#,
        ),
        (
            "fn first",
            r#"
            pub fn make() -> Real { let x = Widget(w: 2mm); 1.0 }
            structure Widget { let w: Length = 1mm }
        "#,
        ),
    ] {
        let module = compile_source_with_stdlib(source);
        assert_eq!(
            unresolved_function_diags(&module).len(),
            0,
            "{order}: constructor calls from a regular fn body resolve via the \
             merged template registry; got {:?}",
            unresolved_function_diags(&module)
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );

        // Silence is not enough: prove the constructor is RESOLVED here, not
        // merely un-warned. A genuinely wrong field name must still be caught,
        // which only a resolved `StructureInstanceCtor` can do.
        let wrong_field = compile_source_with_stdlib(&source.replace("w: 2mm", "nope: 2mm"));
        assert!(
            wrong_field
                .diagnostics
                .iter()
                .any(|d| d.code == Some(reify_core::DiagnosticCode::CtorUnknownField)),
            "{order}: a bad field name must still raise E_CTOR_UNKNOWN_FIELD; got {:?}",
            wrong_field
                .diagnostics
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    }
}

/// Control (d): a trait static fn calling a FORWARD-declared top-level `fn` is
/// ALREADY clean — `traits_phase` runs after `functions_phase`, so by then
/// `ctx.functions` is complete.
///
/// Written down because the obvious generalisation from route (3) is that this
/// warns too. Measured: it does not. Pinning a bug that never existed would
/// mislead the next reader into "fixing" a route that was always correct.
#[test]
fn trait_static_fn_calling_a_forward_declared_fn_stays_clean() {
    let module = compile_source_with_stdlib(
        r#"
        trait Maker { fn build() -> Real { helper(1.0) } }
        pub fn helper(x: Real) -> Real { x }
    "#,
    );

    assert_eq!(
        unresolved_function_diags(&module).len(),
        0,
        "`ctx.functions` is complete by the time traits_phase runs; got {:?}",
        unresolved_function_diags(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// Control (e): a builtin called from a `fn` body stays clean.
///
/// `is_known_builtin` is a name fact and does not care which body asks; this
/// pins that the fn-body routes above did not disturb the builtin answer.
#[test]
fn builtin_called_from_a_fn_body_stays_clean() {
    let module = compile_source_with_stdlib(
        r#"
        pub fn a(x: Real) -> Real { sqrt(x) }
    "#,
    );

    assert_eq!(
        unresolved_function_diags(&module).len(),
        0,
        "`sqrt` is a builtin; got {:?}",
        unresolved_function_diags(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// (h) the THIRD state: declared here, not resolvable from here, zero warnings
// ---------------------------------------------------------------------------
//
// Steps 11-12 pinned a TWO-state invariant — the fallback's two warnings are
// "complements, not a hierarchy; exactly one fires, never both". Sections (f)
// and (g) added a state those tests do not describe: the callee IS declared in
// this module but is not resolvable from this body's table, and NEITHER warning
// fires.
//
// Pinned explicitly so the next reader cannot mistake that silence for a
// regression, nor for a return of the pre-#5371 open-world silence. The two are
// not the same: open-world silence covered every unknown name, whereas this
// silence is granted only to names the module demonstrably declares — which is
// what control (b) in section (g) keeps honest.
//
// Both tests below were confirmed DISCRIMINATING by measurement, not by
// inspection: with the declared-name gate in `expr.rs` disabled, each source
// here emits `unresolved function: b`, one-arg and zero-arg alike.

/// The result type of `fn_name`'s body expression.
///
/// Reads the same public IR surface `boundary2_producer` reads for value cells;
/// the assertions below are about the compiler's OUTPUT, not its internals.
fn fn_body_result_type(module: &reify_compiler::CompiledModule, fn_name: &str) -> Type {
    module
        .functions
        .iter()
        .find(|f| f.name == fn_name)
        .unwrap_or_else(|| panic!("no compiled fn named '{fn_name}'"))
        .body
        .result_expr
        .result_type
        .clone()
}

/// A forward-referenced sibling emits NEITHER warning — including zero-arg.
///
/// The zero-arg half is the one with a back door. `expr.rs`'s legacy guard is
/// `if known && arg_shape_expected.is_none()`, so an implementation that made
/// `known` true for a declared fn would silence `UnresolvedFunction` and then
/// leak the legacy "cannot infer return type of zero-arg function" warning in
/// its place — trading one false positive for another. `known` must stay a
/// BUILTIN fact; declaredness is asked separately.
#[test]
fn forward_referenced_sibling_emits_neither_warning() {
    for (shape, source) in [
        (
            "one-arg",
            r#"
            pub fn a(x: Real) -> Real { b(x) }
            pub fn b(x: Real) -> Real { x }
        "#,
        ),
        (
            "zero-arg",
            r#"
            pub fn a() -> Real { b() }
            pub fn b() -> Real { 1.0 }
        "#,
        ),
    ] {
        let module = compile_source_with_stdlib(source);
        assert!(
            unresolved_function_diags(&module).is_empty(),
            "{shape}: no UnresolvedFunction; got {:?}",
            unresolved_function_diags(&module)
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
        assert!(
            !has_legacy_zero_arg_warning(&module),
            "{shape}: the legacy zero-arg warning must not leak in through the \
             `known` guard; got {:?}",
            warnings_only(&module)
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
        assert!(
            warnings_only(&module).is_empty(),
            "{shape}: this state emits ZERO warnings; got {:?}",
            warnings_only(&module)
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    }
}

/// The RESULT TYPE at a forward-referenced call is the pre-fix answer, exactly.
///
/// Asserted, never assumed: #5371 removes a diagnostic and moves no type, and
/// #5997 flips this Warning to an Error against whatever baseline it finds.
///
/// Both cases are built so the two candidate answers DIFFER, which makes each
/// assertion positive evidence that the forward reference still does NOT
/// resolve — silencing the warning did not quietly complete the lookup, which
/// remains `functions_phase`'s contract and #6014's business.
///   * one-arg: `b` is declared `-> Real` and called with a `Length`.
///     Resolution would say `Real`; the first-arg fallback says `Scalar<LENGTH>`.
///   * zero-arg: `b` is declared `-> Length`. Resolution would say
///     `Scalar<LENGTH>`; the zero-arg fallback defaults to `Real`.
#[test]
fn forward_reference_typing_is_byte_identical() {
    let one_arg = compile_source_with_stdlib(
        r#"
        pub fn a(x: Length) -> Length { b(x) }
        pub fn b(x: Length) -> Real { 1.0 }
    "#,
    );
    assert_eq!(
        fn_body_result_type(&one_arg, "a"),
        scalar_length(),
        "still typed from arg0, NOT from b's declared `-> Real` return type"
    );

    let zero_arg = compile_source_with_stdlib(
        r#"
        pub fn a() -> Real { b() }
        pub fn b() -> Length { 1mm }
    "#,
    );
    assert_eq!(
        fn_body_result_type(&zero_arg, "a"),
        Type::dimensionless_scalar(),
        "a zero-arg fallback call still defaults to Real, NOT to b's `-> Length`"
    );
}

/// The two-state invariant survives wherever it still applies: a genuinely
/// undeclared zero-arg callee yields EXACTLY ONE warning.
///
/// Sections (f)/(g) added a third state; they did not merge the first two.
/// `UnresolvedFunction` and the legacy zero-arg warning are still complements
/// for a name that really does exist nowhere.
#[test]
fn genuinely_undeclared_zero_arg_callee_still_yields_exactly_one_warning() {
    let module = compile_source_with_stdlib(
        r#"
        pub fn a() -> Real { definitely_not_a_builtin() }
    "#,
    );

    assert_eq!(
        unresolved_function_diags(&module).len(),
        1,
        "the stronger claim still fires; got {:?}",
        warnings_only(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        !has_legacy_zero_arg_warning(&module),
        "…and the legacy warning still does not double-report it; got {:?}",
        warnings_only(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        warnings_only(&module).len(),
        1,
        "exactly one warning total; got {:?}",
        warnings_only(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// Route (4): a structure CONSTRUCTOR called from a trait-INSTANCE assoc fn
/// body — a trait-default body, or a structure's override of one.
///
/// Same cause as route (3), different compiler: `conformance/checker.rs`
/// resolves the assoc-fn table through `functions::compile_assoc_function`,
/// the sibling of `compile_function`, which likewise sets no template
/// registry. So `Widget(w: 2mm)` is never claimed as a
/// `StructureInstanceCtor` and rides the terminal fallback with a name the
/// module plainly declares. Route (3)'s fix touched only `compile_function`
/// and left this path warning on ordinary, idiomatic code (esc-5371-12).
///
/// Both halves are exercised because they reach `compile_assoc_function` from
/// two different call sites in `check_phase_resolve_assoc_fns` — the
/// default-body loop and the bodyless-requirement override loop — and a fix
/// applied inside the callee must cover both.
#[test]
fn struct_constructor_in_a_trait_instance_assoc_fn_body_is_not_unresolved() {
    for (shape, source) in [
        (
            "trait-default body",
            r#"
            structure Widget { let w: Length = 1mm }
            trait Maker { fn make_it(self) -> Widget = Widget(w: 2mm) }
            structure Host : Maker { let n: Real = 1.0 }
        "#,
        ),
        (
            "structure override of a trait default",
            r#"
            structure Widget { let w: Length = 1mm }
            trait Maker { fn make_it(self) -> Widget = Widget(w: 1mm) }
            structure Host : Maker {
                let n: Real = 1.0
                fn make_it(self) -> Widget = Widget(w: 2mm)
            }
        "#,
        ),
        (
            "structure override of a bodyless requirement",
            r#"
            structure Widget { let w: Length = 1mm }
            trait Maker { fn make_it(self) -> Widget }
            structure Host : Maker {
                let n: Real = 1.0
                fn make_it(self) -> Widget = Widget(w: 2mm)
            }
        "#,
        ),
    ] {
        let module = compile_source_with_stdlib(source);
        assert_eq!(
            unresolved_function_diags(&module).len(),
            0,
            "{shape}: 'Widget' is a declared structure in this module; got {:?}",
            unresolved_function_diags(&module)
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    }
}

/// Control (i): route (4)'s fix must NOT silence a genuinely-undeclared callee
/// in the same bodies.
///
/// The fix widens `declared_callable_names` at the assoc-fn sites; this is the
/// assertion that the widening is bounded by what the module actually
/// declares, not a blanket suppression for assoc-fn bodies.
#[test]
fn genuinely_undeclared_callee_in_an_assoc_fn_body_still_warns() {
    let module = compile_source_with_stdlib(
        r#"
        trait Maker { fn make_it(self) -> Real = nope_not_a_thing(1.0) }
        structure Host : Maker { let n: Real = 1.0 }
    "#,
    );

    let diags = unresolved_function_diags(&module);
    assert_eq!(
        diags.len(),
        1,
        "'nope_not_a_thing' is declared nowhere and must still warn EXACTLY once \
         — matching the sibling control `genuinely_undeclared_callee_in_a_fn_body\
         _still_warns`, since one-line-per-defect is the load-bearing property of \
         the #5371 split; got {:?}",
        warnings_only(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        diags.iter().all(|d| d.severity == Severity::Warning),
        "warn-mode only",
    );
}

/// Control (j): an assoc-fn body calling a declared top-level `fn` is ALREADY
/// clean — conformance runs after `phase_functions`, so `resolution_functions`
/// is complete and the call RESOLVES rather than reaching the fallback.
///
/// This is the falsifiable half of route (4)'s fix: it populates only the
/// STRUCTURE half of `declared_callable_names` at the assoc-fn sites, and this
/// test is what makes "the fn half would be dead weight there" a measurement
/// rather than an assumption. If this ever reds, the fn half must be threaded
/// in too. Sibling of control (d), which pins the same fact for trait STATIC
/// fn bodies.
#[test]
fn assoc_fn_calling_a_declared_fn_stays_clean() {
    for (order, source) in [
        (
            "fn first",
            r#"
            pub fn helper(x: Real) -> Real { x }
            trait Maker { fn make_it(self) -> Real = helper(1.0) }
            structure Host : Maker { let n: Real = 1.0 }
        "#,
        ),
        (
            "trait first",
            r#"
            trait Maker { fn make_it(self) -> Real = helper(1.0) }
            structure Host : Maker { let n: Real = 1.0 }
            pub fn helper(x: Real) -> Real { x }
        "#,
        ),
    ] {
        let module = compile_source_with_stdlib(source);
        assert_eq!(
            unresolved_function_diags(&module).len(),
            0,
            "{order}: `resolution_functions` is complete by the time conformance \
             runs; got {:?}",
            unresolved_function_diags(&module)
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    }
}

// ---------------------------------------------------------------------------
// (l) A function-TYPED value applied by bare name is a real callee.
// ---------------------------------------------------------------------------

/// `f(x)` where `f` is a `(Length) -> Real` PARAMETER must not warn.
///
/// `f` is an ordinary value binding, so it lives in the scope's `names` map and
/// NOT in the module's declared callable vocabulary, and no ladder arm above
/// the fallback claims a call whose callee is a local of `Type::Function`.
///
/// MEASURED on the branch tip before the fix: `warning: unresolved function: f`.
/// Today's corpus cannot catch it — the only function-typed params in the
/// stdlib (`map_or` in `option_recovery.ri`, `map_err` in `result.ri`) have stub
/// bodies that never apply their parameter — so the sweep stays green either
/// way. #5997 flips this Warning to an Error, at which point the first real
/// higher-order body becomes a hard compile failure.
#[test]
fn function_typed_parameter_applied_by_bare_name_is_not_unresolved() {
    let module = compile_source_with_stdlib(
        r#"
        pub fn apply_once(f: (Length) -> Real, x: Length) -> Length { f(x) }
    "#,
    );

    assert!(
        unresolved_function_diags(&module).is_empty(),
        "'f' is a function-typed parameter in scope, not an unknown name; got {:?}",
        unresolved_function_diags(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert_eq!(errors(&module), Vec::<&str>::new());

    // Typing is UNCHANGED: withholding the warning did not quietly teach the
    // fallback to read `f`'s declared `-> Real` return type. The two candidate
    // answers differ, so this is positive evidence.
    assert_eq!(
        fn_body_result_type(&module, "apply_once"),
        scalar_length(),
        "still typed from arg0, NOT from the function type's `-> Real` codomain"
    );
}

/// Control: the guard is bounded by what the scope actually binds.
///
/// A bare name that is neither a builtin, nor declared by the module, nor bound
/// as a function-typed value must STILL warn — otherwise the fix above is a
/// blanket disable for fn bodies that happen to take a function parameter.
#[test]
fn undeclared_callee_beside_a_function_typed_parameter_still_warns() {
    let module = compile_source_with_stdlib(
        r#"
        pub fn apply_once(f: (Length) -> Real, x: Length) -> Length { not_a_thing(x) }
    "#,
    );

    assert_eq!(
        unresolved_function_diags(&module).len(),
        1,
        "'not_a_thing' is declared nowhere; got {:?}",
        warnings_only(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ---------------------------------------------------------------------------
// (m) End-to-end coverage for every REACHABLE `arg_shape_expectation` arm.
// ---------------------------------------------------------------------------

/// Each reachable arm of `arg_shape_expectation` produces exactly one
/// `BuiltinArgShapeUnrecognized` naming the builtin AND its declared parameter
/// list.
///
/// The unit test `arg_shape_expectation_covers_every_arg_aware_family_member`
/// only checks that each slice entry lands on a match arm; it never exercises
/// the fallback. So a ladder reorder that shadowed one of these names out of
/// the terminal arm would leave every other test in this file green. This is
/// the test that notices.
///
/// Two groups are deliberately absent, each with its own premise guard: the
/// AFFINE family (section (j)) and `compose` (below).
#[test]
fn every_reachable_arg_shape_arm_warns_once_with_its_parameter_list() {
    // (callee, mis-shaped call, a substring of the family's declared params)
    let cases: &[(&str, &str, &str)] = &[
        // list-helper — list_helpers.rs
        ("single", "single(42)", "List<T>"),
        ("flat_map", "flat_map(42, 7)", "(A) -> List<B>"),
        ("generate", "generate(3, 7)", "(Int, (Int) -> B)"),
        // field-op — units.rs, PRD §5.1 signature table
        ("fn_field", "fn_field(42)", "(D) -> C"),
        ("from_samples", "from_samples(42, 7, 0)", "List<D>"),
        ("restrict", "restrict(42, 7)", "Geometry"),
        ("sample", "sample(42, 7)", "Field<D, C>"),
        ("gradient", "gradient(42)", "(Field<D, C>)"),
        ("divergence", "divergence(42)", "(Field<D, C>)"),
        ("curl", "curl(42)", "(Field<D, C>)"),
        ("laplacian", "laplacian(42)", "(Field<D, C>)"),
    ];

    for (callee, call, expected_params) in cases {
        let module = compile_source_with_stdlib(&format!(
            r#"
            structure ArgShapeCase {{
                let x = {call}
            }}
        "#
        ));

        let diags = arg_shape_diags(&module);
        assert_eq!(
            diags.len(),
            1,
            "{callee}: expected exactly one BuiltinArgShapeUnrecognized for \
             `{call}`; got {:?}",
            module
                .diagnostics
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            diags[0].severity,
            Severity::Warning,
            "{callee}: #5371 is warn-mode-first"
        );
        assert!(
            mentions(diags[0], callee),
            "{callee}: the diagnostic must name the builtin; got {:?}",
            diags[0].message
        );
        assert!(
            mentions(diags[0], expected_params),
            "{callee}: the diagnostic must carry the family's declared parameter \
             list — that is the whole actionable content; wanted {expected_params:?}, \
             got message {:?} labels {:?}",
            diags[0].message,
            diags[0].labels.iter().map(|l| &l.message).collect::<Vec<_>>()
        );

        // A mis-shaped call to a name the compiler DOES know is never also
        // reported unresolved, and never changes typing in warn mode.
        assert!(
            unresolved_function_diags(&module).is_empty(),
            "{callee}: a known builtin must not also be reported unresolved"
        );
        assert_eq!(
            errors(&module),
            Vec::<&str>::new(),
            "{callee}: warn-mode-first — a mis-shaped call must not fail the build"
        );
    }
}

/// PREMISE GUARD: `compose` is the one `arg_shape_expectation` arm that cannot
/// be reached from a call site, so it is excluded from the table above.
///
/// It is the only one of the thirteen names that is ALSO a real stdlib
/// declaration — `pub fn compose<A, B, C>(f: Field<B, C>, g: Field<A, B>)` at
/// `stdlib/fields.ri:122` — so user-function overload resolution claims the
/// name long before the `NoUserFunctions` ladder, let alone its terminal
/// fallback. Its arm stays in `arg_shape_expectation` because `compose` is a
/// genuine member of `FIELD_OP_NAMES` and
/// `arg_shape_expectation_covers_every_arg_aware_family_member` requires every
/// slice entry to land on one; it is simply dead at this site.
///
/// This assertion is the tripwire in both directions: if the stdlib `fn` is
/// ever removed, `compose` starts reaching the fallback and belongs back in the
/// positive table above.
#[test]
fn compose_is_claimed_by_its_stdlib_fn_and_never_reaches_the_fallback() {
    let module = compile_source_with_stdlib(
        r#"
        structure ComposePremise {
            let x = compose(42, 7)
        }
    "#,
    );

    assert!(
        arg_shape_diags(&module).is_empty(),
        "`compose` resolves as a stdlib `fn`, so the terminal fallback never \
         sees it and cannot diagnose its arg shape; got {:?}",
        arg_shape_diags(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
    assert!(
        unresolved_function_diags(&module).is_empty(),
        "`compose` is declared in the stdlib prelude; got {:?}",
        unresolved_function_diags(&module)
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}
