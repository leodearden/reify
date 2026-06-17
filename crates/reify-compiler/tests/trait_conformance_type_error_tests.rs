//! Regression tests pinning the `Type::Error` wildcard × trait-conformance invariant
//! (task-448 anti-cascade × task-1936).
//!
//! ## The invariant
//!
//! The `Type::Error` wildcard in `type_compat.rs` (lines 9–11 for
//! `implicitly_converts_to`, lines 94–96 for `type_compatible`) allows any
//! conformance/field-composition check that involves a `Type::Error` operand to
//! succeed silently — it does not emit a "type mismatch" cascade diagnostic.
//! This is the correct anti-cascade behaviour **only if** the producer site has
//! already emitted a root-cause `Severity::Error` diagnostic.  If that
//! producer-site diagnostic were ever downgraded or suppressed, the wildcard
//! would let a genuinely-broken conformance appear green with no visible error.
//!
//! These tests assert:
//!   (a) at least one `Severity::Error` diagnostic is present (invariant: wildcard
//!       silence is always paired with a producer-site error),
//!   (b) no diagnostic message contains `"type mismatch for trait"` regardless of
//!       severity (anti-cascade: no redundant conformance-layer mismatch on top of
//!       a poisoned operand, even if the cascade were downgraded to Warning),
//!   (c) at least one error message contains `"unknown member"` (root-cause pin:
//!       the specific producer diagnostic from `expr.rs` is still present).
//!
//! ## Scenarios covered
//!
//! **Scenario A** (`poisoned_structure_let_preserves_root_cause_error`):
//! A structure's own annotated `let x : Length = self.unsupported` claims trait
//! `HasX { let x : Length }`.  The entity pass produces `Type::Error` + an
//! "unknown member" error from `expr.rs:~724`; the conformance pass sees
//! `Length` (from the annotation) vs `Length` (requirement) so
//! `implicitly_converts_to(Length, Length)` returns `true` directly — no wildcard
//! needed, no mismatch emitted.  The invariant: ≥1 root-cause error must still be
//! present so the caller isn't misled by the green conformance outcome.
//!
//! **Scenario B** (`poisoned_trait_default_preserves_root_cause_error`):
//! A trait provides a default `let x : Length = self.unsupported`; a structure
//! `def S : T {}` inherits it without override.  During trait-default injection
//! (conformance.rs:~473–535) the compiler evaluates `self.unsupported` in the
//! structure's scope → `Type::Error` + "unknown member" error.  Then
//! `type_compatible(Length, Type::Error)` at line 526 fires the wildcard →
//! no "type mismatch for trait let" cascade.  This scenario directly exercises
//! the wildcard call-site that task-1936 targets.

use reify_test_support::{compile_source, errors_only};
use reify_core::Diagnostic;

// ── Shared assertion helper ───────────────────────────────────────────────────

/// Asserts the three-part poisoned-conformance invariant on a compiled module:
///
/// (a) At least one `Severity::Error` diagnostic is present — wildcard silence
///     is always paired with a root-cause producer error.
/// (b) No diagnostic (at **any** severity) has a message containing
///     `"type mismatch for trait"` — the anti-cascade invariant holds even if
///     the conformance layer were to downgrade the mismatch to `Warning`.
/// (c) At least one `Severity::Error` diagnostic contains `"unknown member"` —
///     the specific root-cause producer diagnostic from `expr.rs` is present.
fn assert_poisoned_conformance_invariant(module: &reify_compiler::CompiledModule) {
    let errors = errors_only(module);

    // (a) At least one Severity::Error must be present.
    assert!(
        !errors.is_empty(),
        "Type::Error wildcard allowed conformance to go green but no Severity::Error \
         was present — the root-cause 'unknown member' diagnostic from the producer \
         site has been downgraded/removed; this breaks the anti-cascade safety promise \
         of type_compat.rs:9–11,94–96",
    );

    // (b) No conformance-layer cascade mismatch on top of the poisoned operand
    //     — checked across ALL severities so a Warning-severity downgrade is
    //     also caught.
    assert!(
        !module
            .diagnostics
            .iter()
            .any(|d| d.message.contains("type mismatch for trait")),
        "unexpected 'type mismatch for trait' cascade diagnostic (regardless of \
         severity) — the Type::Error wildcard should suppress conformance-layer \
         mismatches when a producer-site error is already present; \
         all diagnostics: {:?}",
        module.diagnostics,
    );

    // (c) The specific root-cause "unknown member" diagnostic must be present.
    assert!(
        errors.iter().any(|d| d.message.contains("unknown member")),
        "expected at least one error containing 'unknown member' (the root-cause \
         producer diagnostic from expr.rs); got: {:?}",
        errors,
    );
}

// ── Scenario A: structure's own poisoned annotated let ────────────────────────

/// Pinning test: a structure that claims a trait but whose matching `let`
/// binding uses `self.unsupported` (a canonical `Type::Error` producer) must
/// still carry a root-cause `Severity::Error` diagnostic even though the
/// conformance check succeeds silently (annotation type matches requirement).
///
/// If this test fails it means one of:
///   • the "unknown member" producer-site diagnostic was downgraded/removed,
///   • or the wildcard in `type_compat.rs:9–11/94–96` no longer fires and a
///     spurious "type mismatch for trait" cascade was emitted instead.
#[test]
fn poisoned_structure_let_preserves_root_cause_error() {
    let source = r#"
trait HasX {
    let x : Length
}
structure def S : HasX {
    let x : Length = self.unsupported
}
"#;
    let module = compile_source(source);
    assert_poisoned_conformance_invariant(&module);
}

// ── Scenario B: trait-provided default with poisoned expression ───────────────

/// Pinning test: a trait whose `let` default references `self.unsupported` and
/// a structure that inherits that default without override must carry a
/// root-cause `Severity::Error`.  This scenario directly exercises the
/// `type_compatible(annotation_ty, Type::Error)` wildcard call-site at
/// `conformance.rs:~526` (trait-default injection path).
///
/// If this test fails it means one of:
///   • the "unknown member" producer-site diagnostic was downgraded/removed,
///   • the wildcard at `conformance.rs:526` was tightened (causing a spurious
///     "type mismatch for trait let" cascade to appear instead), or
///   • injection of trait defaults into an inheriting structure no longer
///     evaluates the default expression (so no error is emitted at all).
#[test]
fn poisoned_trait_default_preserves_root_cause_error() {
    let source = r#"
trait HasX {
    let x : Length = self.unsupported
}
structure def S : HasX {}
"#;
    let module = compile_source(source);
    assert_poisoned_conformance_invariant(&module);
}

// ── Shared helper for Scenarios C and D ──────────────────────────────────────

/// Asserts the two-part unresolved-annotation cascade-suppression invariant on
/// a freshly compiled `source` string.
///
/// (a) Root-cause pin: ≥1 `Severity::Error` diagnostic contains both
///     `"unresolved type in conformance check"` and `"UnknownType"` — the
///     specific message from the Named arm of `resolve_member_annotation_type`.
/// (b) Anti-cascade pin: NO diagnostic at **any** severity contains
///     `"type mismatch for trait member"` — so a future Warning-severity
///     downgrade of the cascade is also caught.
fn assert_unresolved_annotation_suppresses_cascade(source: &str) {
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        errors.iter().any(|d| {
            d.message.contains("unresolved type in conformance check")
                && d.message.contains("UnknownType")
        }),
        "expected an error containing 'unresolved type in conformance check' and \
         'UnknownType' (root-cause pin for resolve_member_annotation_type); \
         got: {:?}",
        errors,
    );

    assert!(
        !module
            .diagnostics
            .iter()
            .any(|d| d.message.contains("type mismatch for trait member")),
        "unexpected 'type mismatch for trait member' cascade diagnostic (regardless of \
         severity) — Type::Error returned from resolve_member_annotation_type should \
         suppress this via the producer-side wildcard in type_compat.rs:3–26; \
         all diagnostics: {:?}",
        module.diagnostics,
    );
}

// ── Scenario C: unresolved annotation on param (Named path) ──────────────────

/// Pins the `Type::Error` fallback for the `Named`-path branch of
/// `resolve_member_annotation_type` when the member is a `param`.
///
/// **Before the fix** the closure returned `Type::dimensionless_scalar()`; because the trait
/// requires `Length ≠ Real`, a second misleading
/// `"type mismatch for trait member 'x': expected Length, got Real"` cascade
/// diagnostic appeared on top of the root-cause error.  **After the fix** the
/// producer-side wildcard in `type_compat.rs:3–26` suppresses the cascade.
#[test]
fn param_unresolved_annotation_suppresses_conformance_cascade() {
    assert_unresolved_annotation_suppresses_cascade(
        r#"
trait T {
    param x : Length
}
structure def S : T {
    param x : UnknownType
}
"#,
    );
}

// ── Scenario D: unresolved annotation on let (Named path, Let arm) ────────────

/// Regression pin for the `Let` call-site of `resolve_member_annotation_type`
/// (the `Let` arm of the `structure_members` filter_map).
///
/// The closure is shared between the `Param` arm (Scenario C) and the `Let`
/// arm, so both paths exercise the same `Type::Error` return.  This test
/// guards against a future refactor that splits the two call-sites and
/// accidentally re-introduces `Type::dimensionless_scalar()` on only one of them.
#[test]
fn let_unresolved_annotation_suppresses_conformance_cascade() {
    assert_unresolved_annotation_suppresses_cascade(
        r#"
trait T {
    let x : Length
}
structure def S : T {
    let x : UnknownType = 5mm
}
"#,
    );
}

// ── Scenario E: missing annotation on param (no type_expr) ───────────────────

/// Pins the `Type::Error` poison-sentinel return for the `None` arm of
/// `p.type_expr.as_ref()` inside `check_phase_resolve_structure_members`
/// (conformance/checker.rs, `MemberDecl::Param` branch).
///
/// **Before the fix** the `None` arm returned `Type::dimensionless_scalar()`
/// (= `Real`).  Because the trait requires `Length` (not `Real`), phase 5's
/// member-vs-requirement check saw `implicitly_converts_to(Real, Length)` =
/// `false` and emitted a spurious secondary
/// `"type mismatch for trait member 'x': expected Scalar[m], got Real"` on top
/// of the root-cause `"trait member 'x' has no type annotation; cannot infer
/// type"`.  **After the fix** the `None` arm returns `Type::Error`; the
/// producer-side wildcard in `type_compat.rs` (`implicitly_converts_to(Error,
/// _) => true`) suppresses the cascade.
///
/// `Length` (not `Real`) is deliberate: a `Real` requirement would
/// coincidentally match the `dimensionless_scalar()` fallback and never produce
/// a RED.  The test fixture is the verified two-error baseline from the task
/// analysis.
#[test]
fn missing_annotation_param_suppresses_conformance_cascade() {
    let source = r#"
trait T {
    param x : Length
}
structure def W : T {
    param x
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    // (a) Anti-cascade pin: NO diagnostic at ANY severity contains "type mismatch
    //     for trait member" — catches a future Warning-severity downgrade too.
    assert!(
        !module
            .diagnostics
            .iter()
            .any(|d| d.message.contains("type mismatch for trait member")),
        "unexpected 'type mismatch for trait member' cascade diagnostic (regardless of \
         severity) — Type::Error returned from the missing-annotation arm should suppress \
         this via the producer-side wildcard in type_compat.rs; \
         all diagnostics: {:?}",
        module.diagnostics,
    );

    // (b) Exactly-one-error + root-cause pin: the missing-annotation root cause
    //     must be present and must be the only Severity::Error diagnostic.
    assert_eq!(
        errors.len(),
        1,
        "expected exactly one Severity::Error diagnostic (the missing-annotation root \
         cause); got: {:?}",
        errors,
    );
    assert!(
        errors[0].message.contains("no type annotation")
            || errors[0].message.contains("cannot infer type"),
        "expected the single error to contain 'no type annotation' or 'cannot infer type'; \
         got: {:?}",
        errors[0],
    );
}

// ── Severity-robustness regression test ──────────────────────────────────────

/// Robustness test: check (b) must catch a "type mismatch for trait" cascade
/// even when emitted at `Severity::Warning` (not just `Severity::Error`).
///
/// If the helper only filtered `module.diagnostics` by `Severity::Error` before
/// searching for the cascade message, a Warning-severity offender would be
/// invisible and the test would silently pass — the invariant inverted.
#[test]
#[should_panic(expected = "unexpected 'type mismatch for trait' cascade")]
fn helper_flags_cascade_at_warning_severity() {
    // CompiledModule has many fields; cheapest way to obtain a valid instance
    // is a trivial compile, then overwrite .diagnostics with synthetic entries.
    let mut module = compile_source("structure def Dummy {}");
    module.diagnostics = vec![
        Diagnostic::error("unknown member 'unsupported' in scope S"),
        Diagnostic::warning("type mismatch for trait let x: expected Length, got Error"),
    ];
    assert_poisoned_conformance_invariant(&module);
}
