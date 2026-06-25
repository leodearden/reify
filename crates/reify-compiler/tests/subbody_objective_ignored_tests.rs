//! Compiler tests for W_SUBBODY_OBJECTIVE_IGNORED (task 4823).
//!
//! ## Problem
//!
//! A `minimize` or `maximize` declaration inside a sub specialization body
//! (`sub x : T { minimize <expr> }`) is silently dropped today.  The
//! `MemberDecl::Sub` arm in `entity.rs` reads only `sub.spec_param_overrides`
//! and never inspects `sub.body`'s minimize/maximize entries.
//!
//! Task 4823 replaces the silent drop with a loud `Warning` carrying
//! `DiagnosticCode::SubbodyObjectiveIgnored` and a message that begins with
//! `W_SUBBODY_OBJECTIVE_IGNORED:`, names the sub, and references M-WHOLE (#4785).
//!
//! ## RED → GREEN arc
//!
//! Step 3 (RED): tests compile source with `sub x : T { minimize a }`.
//! Until step 4 wires up the emission in `entity.rs`, the assertion that
//! `warnings_only(&module).any(|d| d.code == Some(SubbodyObjectiveIgnored))`
//! fails — zero warnings are emitted.
//!
//! Step 4 (GREEN): `entity.rs` iterates `sub.body.iter().flatten()`,
//! matches `MemberDecl::Minimize`/`Maximize`, and pushes one
//! `Diagnostic::warning(...)` per occurrence.  All assertions below pass.

use reify_core::DiagnosticCode;
use reify_test_support::{compile_source_with_stdlib, errors_only, warnings_only};

// ── Fixture ────────────────────────────────────────────────────────────────────

/// Minimal structure that supplies a `Length` param for objective expressions.
const T_PREAMBLE: &str = "structure T { param a : Length = 5mm }";

/// Build a full source string with a parent structure `A` whose sub `x`
/// has the given specialization body.
fn source_with_sub_body(body: &str) -> String {
    format!("{T_PREAMBLE}  structure A {{ sub x : T {{ {body} }} }}")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// (a) `sub x : T { minimize a }` → exactly one `SubbodyObjectiveIgnored` warning
///     whose message contains the mnemonic `W_SUBBODY_OBJECTIVE_IGNORED`, names
///     the sub `x`, and references M-WHOLE via `4785`.
///
/// RED until step-4 wires the emission in `entity.rs`.
#[test]
fn subbody_minimize_emits_one_warning() {
    let source = source_with_sub_body("minimize a");
    let module = compile_source_with_stdlib(&source);

    // No errors — compilation proceeds despite the dropped objective.
    assert!(
        errors_only(&module).is_empty(),
        "subbody minimize: unexpected errors: {:?}",
        errors_only(&module)
    );

    let warnings = warnings_only(&module);

    // Exactly one SubbodyObjectiveIgnored warning.
    let matching: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SubbodyObjectiveIgnored))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "subbody minimize: expected exactly 1 SubbodyObjectiveIgnored warning, got {}: {:?}",
        matching.len(),
        warnings.iter().map(|w| &w.message).collect::<Vec<_>>()
    );

    let msg = &matching[0].message;
    assert!(
        msg.contains("W_SUBBODY_OBJECTIVE_IGNORED"),
        "warning message must contain the mnemonic 'W_SUBBODY_OBJECTIVE_IGNORED'; got: {msg:?}"
    );
    assert!(
        msg.contains("x"),
        "warning message must name the sub 'x'; got: {msg:?}"
    );
    assert!(
        msg.contains("4785"),
        "warning message must reference M-WHOLE via '4785'; got: {msg:?}"
    );
}

/// (b) `sub x : T { maximize a }` → same shape: exactly one
///     `SubbodyObjectiveIgnored` warning with identical structural assertions.
///
/// RED until step-4.
#[test]
fn subbody_maximize_emits_one_warning() {
    let source = source_with_sub_body("maximize a");
    let module = compile_source_with_stdlib(&source);

    assert!(
        errors_only(&module).is_empty(),
        "subbody maximize: unexpected errors: {:?}",
        errors_only(&module)
    );

    let warnings = warnings_only(&module);

    let matching: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SubbodyObjectiveIgnored))
        .collect();
    assert_eq!(
        matching.len(),
        1,
        "subbody maximize: expected exactly 1 SubbodyObjectiveIgnored warning, got {}: {:?}",
        matching.len(),
        warnings.iter().map(|w| &w.message).collect::<Vec<_>>()
    );

    let msg = &matching[0].message;
    assert!(
        msg.contains("W_SUBBODY_OBJECTIVE_IGNORED"),
        "warning message must contain the mnemonic; got: {msg:?}"
    );
    assert!(
        msg.contains("x"),
        "warning message must name the sub; got: {msg:?}"
    );
    assert!(
        msg.contains("4785"),
        "warning message must reference M-WHOLE via '4785'; got: {msg:?}"
    );
}

/// (c) PRECISION: a sub body with only a param override (`a = 2mm`) and a
///     bare instantiation form (`sub x = T()`) each produce ZERO
///     `SubbodyObjectiveIgnored` warnings — the diagnostic must not fire on
///     non-objective body members.
///
/// Must stay GREEN both before and after step-4.
#[test]
fn subbody_param_override_no_warning() {
    // Specialization body with a concrete param override — no objective.
    let source = source_with_sub_body("a = 2mm");
    let module = compile_source_with_stdlib(&source);

    let warnings = warnings_only(&module);
    let matching: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SubbodyObjectiveIgnored))
        .collect();
    assert!(
        matching.is_empty(),
        "param override body must NOT emit SubbodyObjectiveIgnored; got: {:?}",
        matching.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

#[test]
fn bare_sub_instantiation_no_warning() {
    // Bare instantiation form (`sub x = T()`) — no body.
    let source = format!("{T_PREAMBLE}  structure A {{ sub x = T() }}");
    let module = compile_source_with_stdlib(&source);

    let warnings = warnings_only(&module);
    let matching: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SubbodyObjectiveIgnored))
        .collect();
    assert!(
        matching.is_empty(),
        "bare instantiation must NOT emit SubbodyObjectiveIgnored; got: {:?}",
        matching.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// (d) PER-OCCURRENCE: a body with both `minimize a` and `maximize a` yields
///     exactly 2 `SubbodyObjectiveIgnored` warnings — one per occurrence.
///
/// RED until step-4.
#[test]
fn subbody_minimize_and_maximize_emit_two_warnings() {
    let source = source_with_sub_body("minimize a  maximize a");
    let module = compile_source_with_stdlib(&source);

    assert!(
        errors_only(&module).is_empty(),
        "minimize+maximize body: unexpected errors: {:?}",
        errors_only(&module)
    );

    let warnings = warnings_only(&module);

    let matching: Vec<_> = warnings
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::SubbodyObjectiveIgnored))
        .collect();
    assert_eq!(
        matching.len(),
        2,
        "minimize+maximize body: expected exactly 2 SubbodyObjectiveIgnored warnings, got {}: {:?}",
        matching.len(),
        warnings.iter().map(|w| &w.message).collect::<Vec<_>>()
    );
}
