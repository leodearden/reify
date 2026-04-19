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
use reify_types::Diagnostic;

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

// ── Severity-robustness regression test ──────────────────────────────────────

/// Robustness test: check (b) must catch a "type mismatch for trait" cascade
/// even when emitted at `Severity::Warning` (not just `Severity::Error`).
///
/// If the helper only filtered `module.diagnostics` by `Severity::Error` before
/// searching for the cascade message, a Warning-severity offender would be
/// invisible and the test would silently pass — the invariant inverted.
#[test]
#[should_panic(expected = "type mismatch for trait")]
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
