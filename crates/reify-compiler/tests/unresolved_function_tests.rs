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

// ---------------------------------------------------------------------------
// the zero-arg interaction: two warnings, mutually exclusive
// ---------------------------------------------------------------------------

/// Every Warning-severity diagnostic in `module`, in source order. Used by the
/// zero-arg tests, which assert on the TOTAL warning count rather than on a
/// single code — the whole point there is that no second warning sneaks in.
fn warnings(module: &reify_compiler::CompiledModule) -> Vec<&reify_core::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect()
}

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
        warnings(&module).len(),
        1,
        "exactly one warning total; got {:?}",
        warnings(&module)
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
        warnings(&module).len(),
        1,
        "exactly one warning total; got {:?}",
        warnings(&module)
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
/// order. Matches on the CODE, never on message text.
fn arg_shape_diags(module: &reify_compiler::CompiledModule) -> Vec<&reify_core::Diagnostic> {
    module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(reify_core::DiagnosticCode::BuiltinArgShapeUnrecognized))
        .collect()
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
        warnings(&module).len(),
        1,
        "exactly one warning total; got {:?}",
        warnings(&module)
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
