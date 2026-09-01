//! Shared `auto:`-type-parameter test fixtures and their anti-vacuity guards
//! (task #6798, PRD `docs/prds/v0_6/driver-contract-implementation.md` leaf
//! pi).
//!
//! Lives in its own `#[cfg(test)]` module rather than at the top level of
//! `diagnostics.rs` so ~250 lines of test scaffolding stay out of the
//! production module's item list. Both `crate::diagnostics::tests` and
//! `crate::analysis::tests` import from here, so the fixtures and their
//! guards cannot drift between the entry points that share them.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, ModulePath};

/// BT8 fixture (PRD `docs/prds/v0_6/driver-contract-implementation.md`, leaf
/// pi / task #6798).
///
/// A CONSTANT constraint (`constraint 0 > 1`) on a two-candidate `auto:`
/// trait bound. This is the fixture the whole leaf hinges on: a constant
/// constraint has no `ValueRef` leaves, so `SimpleConstraintChecker`
/// evaluates it to `Value::Bool(false)` → `Satisfaction::Violated` for
/// EVERY candidate, regardless of the compile-time `ValueMap` — this is the
/// ONLY compile-time shape on which the real checker diverges from the
/// `CompileTimeIndeterminateChecker` stub (PRD §12 premise correction 3).
/// Under the stub both candidates are `Indeterminate` → both feasible →
/// strict mode → `E_AUTO_TYPE_PARAM_AMBIGUOUS`; under the real checker both
/// are `Violated` → zero feasible → `E_AUTO_TYPE_PARAM_NO_CANDIDATE`. That
/// AMBIGUOUS-vs-NO_CANDIDATE divergence is exactly the task's named
/// consumer signal: editor users seeing an ambiguity error that
/// `reify check` does not report.
///
/// The type parameter `T` is deliberately UNUSED in `Bearing`'s body
/// (`param bore : Real = 1.0`, not `param seal : T`). Measured: with `T`
/// used, a failed `auto:` resolution leaves a `TypeParam("T")`-typed value
/// cell, and `reify-eval`'s `#[cfg(debug_assertions)]`
/// `assert_value_cell_types_representable`
/// (`crates/reify-eval/src/engine_eval.rs:206`) PANICS during the LSP's
/// eval/check pass — today, on the stub path too (all tests run in debug).
/// With `T` unused there is no such value cell, so both LSP entry points
/// return cleanly and the AMBIGUOUS/NO_CANDIDATE divergence is preserved.
/// Do not "simplify" this back to `param seal : T`.
///
/// **Reachability note (task #6798 amendment round 2 — supersedes round 1).**
/// Round 1 called the `param seal : T` hazard "pre-existing, already
/// reachable today via the multi-candidate case on either checker". Measured
/// re-verification falsified that: it holds for the MULTI-candidate shape
/// only. For the SINGLE-candidate + param-referencing-constraint shape
/// ([`AUTO_FAIL_UNSUBSTITUTED_TYPEPARAM_SRC`]) the stub compiles to ZERO
/// diagnostics AND evals cleanly, and only the real checker fails resolution.
/// This leaf's checker swap therefore made a genuinely NEW panic path
/// reachable from the LSP's own production entry points — not merely a wider
/// window on an old one.
///
/// **Root cause: task #6851.** `phase_auto_type_param_resolution` gates all
/// substitution behind `if !sigma.is_empty()`, so a failed resolution
/// synthesizes no monomorph and a `param seal : T` member keeps
/// `cell_type = Type::TypeParam("T")` into the evaluation graph. #6851 owns
/// the compiler-side fix (its "fix-site 1": stop leaving the sub pointing at
/// an un-substituted generic). What landed in this file is its **fix-site 2**,
/// caller-side defence-in-depth, scoped to `reify-lsp`'s three production
/// entry points only — [`crate::diagnostics::compiled_graph_has_unrepresentable_cell`]
/// skips the eval/check pass when the graph would carry such a cell.
/// `crates/reify-cli/src/mcp_context.rs`'s three ungated `engine.eval` sites
/// remain #6851's to fix.
///
/// **It IS test-pinned here**, contrary to round 1's claim that a test
/// reaching the hazard would itself panic: the guard is exactly what makes
/// such a test possible. See
/// [`crate::diagnostics::tests::auto_resolution_failure_does_not_panic_diagnostics_entry_points`]
/// and `analysis::tests::auto_resolution_failure_does_not_panic_analysis_context`,
/// both over [`AUTO_FAIL_UNSUBSTITUTED_TYPEPARAM_SRC`].
///
/// **Release-build asymmetry — this is not debug-only cosmetics.**
/// `assert_value_cell_types_representable` is `#[cfg(debug_assertions)]` and
/// fully elided in release, so release builds do not crash: they proceed with
/// the unrepresentable cell live and yield a confusing `TypeKindMismatch` /
/// `Undef` (#6851's release-side finding). The same guard contains both
/// halves. Debug surfaces where the crash half is reachable:
/// `scripts/run-gui-dev.sh`, a locally built `reify lsp`, `reify gui --debug`
/// / `gui-debug`, and the per-worktree `reify-debug` MCP server.
pub(crate) const BT8_CONSTANT_CONSTRAINT_SRC: &str = r#"trait Seal {}
structure def GasketSeal : Seal { param d : Real = 2.0 }
structure def OringSeal : Seal { param d : Real = 3.0 }
structure def Bearing<T: Seal> {
    param bore : Real = 1.0
    constraint 0 > 1
}
structure def Assembly { sub b = Bearing<auto: Seal>() }
"#;

/// Containment fixture for the `auto:`-resolution-failure panic hazard (task
/// #6798 amendment round 2; the underlying compiler defect is owned by task
/// **#6851**).
///
/// A SINGLE-candidate `auto:` bound whose constraint references a param with a
/// LITERAL default. Three measured properties make this the right fixture, and
/// none of them may be "simplified" away:
///
/// 1. **`param seal : T` is REQUIRED** — deliberately unlike
///    [`BT8_CONSTANT_CONSTRAINT_SRC`], which leaves `T` unused for exactly the
///    opposite reason. Using `T` in the body is what creates the
///    `TypeParam("T")`-typed value cell that a failed resolution leaves
///    unsubstituted, and therefore what makes the hazard reachable at all.
///
/// 2. **Exactly ONE candidate (`GasketSeal`) plus a PARAM-REFERENCING
///    constraint (`constraint bore > 10.0`, not a constant)** are what make
///    the hazard NEWLY reachable by task #6798's checker swap rather than
///    pre-existing. Measured: under the compile-time
///    `CompileTimeIndeterminateChecker` stub, `bore > 10.0` has `ValueRef`
///    leaves → `Indeterminate` → the sole candidate is feasible → resolution
///    SUCCEEDS → `compile_with_stdlib` emits ZERO diagnostics and eval
///    completes cleanly. Under the real `SimpleConstraintChecker`, `bore`
///    binds to its literal default `1.0` → `1.0 > 10.0` → `Bool(false)` →
///    `Violated` → zero feasible candidates → `E_AUTO_TYPE_PARAM_NO_CANDIDATE`
///    → resolution fails → unsubstituted cell → panic. Make the constraint
///    constant, or add a second candidate, and the shape collapses into the
///    pre-existing multi-candidate case that panics on either checker — which
///    would no longer test what this fixture exists to test.
///
/// 3. **The trigger is not contrived.** A literal-default param plus a
///    momentarily-violated constraint is the single most common transient
///    state while typing in an editor — which is why containment belongs in
///    this diff rather than being deferred wholesale to #6851.
pub(crate) const AUTO_FAIL_UNSUBSTITUTED_TYPEPARAM_SRC: &str = r#"trait Seal {}
structure def GasketSeal : Seal { param d : Real = 2.0 }
structure def Bearing<T: Seal> {
    param bore : Real = 1.0
    param seal : T
    constraint bore > 10.0
}
structure def Assembly { sub b = Bearing<auto: Seal>() }
"#;

/// Anti-vacuity guard shared by the containment tests over
/// [`AUTO_FAIL_UNSUBSTITUTED_TYPEPARAM_SRC`].
///
/// Deliberately NOT [`assert_bt8_fixture_still_diverges`]: that guard pins the
/// AMBIGUOUS↔NO_CANDIDATE divergence of the two-candidate constant-constraint
/// fixture, which is a DIFFERENT claim. This one pins the
/// clean↔NO_CANDIDATE divergence that makes the panic hazard NEWLY reachable:
/// the stub compiles the fixture with zero errors, and only the real checker
/// fails resolution.
///
/// Shared between `crate::diagnostics::tests::…_does_not_panic_diagnostics_entry_points`
/// and `crate::analysis::tests::…_does_not_panic_analysis_context` so the two cannot
/// drift apart.
pub(crate) fn assert_auto_fail_fixture_is_newly_reachable() {
    let parsed = reify_compiler::parse_with_stdlib(
        AUTO_FAIL_UNSUBSTITUTED_TYPEPARAM_SRC,
        ModulePath::single("test"),
    );
    let stub = reify_compiler::compile_with_stdlib(&parsed);
    let real = reify_compiler::compile_with_stdlib_checked(&parsed, &SimpleConstraintChecker);
    assert!(
        !stub
            .diagnostics
            .iter()
            .any(|d| d.severity == reify_core::Severity::Error),
        "anti-vacuity guard: the compile-time stub must compile \
         AUTO_FAIL_UNSUBSTITUTED_TYPEPARAM_SRC with ZERO Error-severity \
         diagnostics — that is what makes the panic hazard NEWLY reachable \
         via task #6798's checker swap rather than pre-existing. If the stub \
         now errors too, this fixture has stopped demonstrating newly-reachable \
         failure and the containment tests below are vacuous. stub \
         diagnostics: {:#?}",
        stub.diagnostics
    );
    assert!(
        real.diagnostics
            .iter()
            .any(|d| d.severity == reify_core::Severity::Error
                && d.code == Some(DiagnosticCode::AutoTypeParamNoCandidate)),
        "anti-vacuity guard: the real SimpleConstraintChecker must fail \
         `auto:` resolution on AUTO_FAIL_UNSUBSTITUTED_TYPEPARAM_SRC with an \
         Error-severity AutoTypeParamNoCandidate — that failed resolution is \
         what leaves the unsubstituted TypeParam cell the containment guard \
         exists to keep away from the engine (task #6851). If it no longer \
         does, this fixture has stopped demonstrating newly-reachable failure \
         and the containment tests below are vacuous. real-checker \
         diagnostics: {:#?}",
        real.diagnostics
    );
}

/// Anti-vacuity guard shared by every BT8 forward test: assert the
/// compile-time stub and the real `SimpleConstraintChecker` still
/// genuinely diverge on [`BT8_CONSTANT_CONSTRAINT_SRC`] before any
/// downstream assertion compares an LSP entry point against the real
/// checker's verdict.
///
/// Extracted (task #6798 amendment, reviewer finding "duplication") so the
/// guard cannot drift between its two call sites —
/// `crate::diagnostics::tests::lsp_constant_constraint_agrees_with_reify_check_real_checker`
/// and `crate::analysis::tests::analysis_context_uses_real_constraint_checker` —
/// which is exactly the kind of drift sharing the fixture const was already
/// meant to prevent.
///
/// If a future compiler change collapses AMBIGUOUS/NO_CANDIDATE into the
/// same verdict, this fails loudly instead of a caller's LSP assertion
/// passing vacuously.
pub(crate) fn assert_bt8_fixture_still_diverges() {
    let parsed = reify_compiler::parse_with_stdlib(
        BT8_CONSTANT_CONSTRAINT_SRC,
        ModulePath::single("test"),
    );
    // Real-checker call shape matches `reify-cli`'s `parse_and_compile`
    // verbatim (`crates/reify-cli/src/main.rs:200`).
    let stub = reify_compiler::compile_with_stdlib(&parsed);
    let real = reify_compiler::compile_with_stdlib_checked(&parsed, &SimpleConstraintChecker);
    let stub_codes: std::collections::HashSet<DiagnosticCode> =
        stub.diagnostics.iter().filter_map(|d| d.code).collect();
    let real_codes: std::collections::HashSet<DiagnosticCode> =
        real.diagnostics.iter().filter_map(|d| d.code).collect();
    assert!(
        stub_codes.contains(&DiagnosticCode::AutoTypeParamAmbiguous),
        "anti-vacuity guard: BT8_CONSTANT_CONSTRAINT_SRC has stopped \
         reproducing the stub's AutoTypeParamAmbiguous verdict — the \
         fixture is no longer divergent and BT8 forward tests would pass \
         vacuously. stub diagnostics: {:#?}",
        stub.diagnostics
    );
    assert!(
        real_codes.contains(&DiagnosticCode::AutoTypeParamNoCandidate)
            && !real_codes.contains(&DiagnosticCode::AutoTypeParamAmbiguous),
        "anti-vacuity guard: BT8_CONSTANT_CONSTRAINT_SRC has stopped \
         reproducing the real checker's AutoTypeParamNoCandidate verdict — \
         the fixture is no longer divergent and BT8 forward tests would \
         pass vacuously. real-checker diagnostics: {:#?}",
        real.diagnostics
    );
    assert_ne!(
        stub_codes, real_codes,
        "anti-vacuity guard: stub and real-checker DiagnosticCode sets must \
         differ on BT8_CONSTANT_CONSTRAINT_SRC — a constant constraint is \
         the one compile-time shape where they diverge; stub: {:#?}, real: \
         {:#?}",
        stub.diagnostics, real.diagnostics
    );
}
