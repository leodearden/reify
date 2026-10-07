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
/// **Reachability.** The `param seal : T` panic hazard is pre-existing for the
/// MULTI-candidate shape (it fires on either checker), but NOT for the
/// SINGLE-candidate + param-referencing-constraint shape
/// ([`AUTO_FAIL_UNSUBSTITUTED_TYPEPARAM_SRC`]): measured, the stub compiles
/// that one to ZERO diagnostics AND evals cleanly, and only the real checker
/// fails resolution. This leaf's checker swap therefore made a genuinely NEW
/// panic path reachable from the LSP's own production entry points — not
/// merely a wider window on an old one.
///
/// **Root cause: task #6851.** `phase_auto_type_param_resolution` gates all
/// substitution behind `if !sigma.is_empty()`, so a failed resolution
/// synthesizes no monomorph and a `param seal : T` member keeps
/// `cell_type = Type::TypeParam("T")` into the evaluation graph. #6851 owns
/// the compiler-side fix (its "fix-site 1": stop leaving the sub pointing at
/// an un-substituted generic). What landed in this file is its **fix-site 2**,
/// caller-side defence-in-depth, scoped to `reify-lsp`'s three production
/// entry points only — [`crate::diagnostics::eval_guard::compiled_graph_has_unrepresentable_cell`]
/// skips the eval/check pass when the graph would carry such a cell.
/// `crates/reify-cli/src/mcp_context.rs`'s three ungated `engine.eval` sites
/// remain #6851's to fix.
///
/// **It IS test-pinned here** — the guard is exactly what makes a test that
/// reaches the hazard possible without itself panicking. See
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

/// Containment fixture for the `auto:`-resolution-failure panic hazard (the
/// underlying compiler defect is owned by task **#6851**).
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

/// Containment fixture for the same `auto:`-resolution-failure hazard as
/// [`AUTO_FAIL_UNSUBSTITUTED_TYPEPARAM_SRC`], with the `param seal : T` member
/// moved inside a GUARDED group — the shape the containment predicate's
/// stage-1 absence proof used to miss entirely.
///
/// Verbatim [`AUTO_FAIL_UNSUBSTITUTED_TYPEPARAM_SRC`] except that `seal` now
/// lives in a `where bore > 0.5 { ... }` block. That is ordinary `.ri`, not a
/// contrivance: it is the same `where` form `examples/m5_guarded_enum.ri`
/// uses. Every property point 2 of the sibling fixture's doc establishes
/// carries over unchanged and was re-measured on this shape — stub: ZERO
/// diagnostics and a SUCCEEDING resolution (its graph carries no
/// unrepresentable cell at all); real `SimpleConstraintChecker`:
/// `E_AUTO_TYPE_PARAM_NO_CANDIDATE`, "'GasketSeal' rejected by constraint
/// Bearing#0". So this crash path, like its sibling's, is genuinely NEW to
/// task #6798's checker swap rather than pre-existing.
///
/// **Why a second fixture rather than a parameter on the first.** A guarded
/// member is not merely a different spelling: `crates/reify-compiler/src/`
/// collects it into `CompiledGuardedGroup::members`, a Vec that never appears
/// in `TopologyTemplate::value_cells`. `EvaluationGraph::from_templates`
/// nevertheless inserts it into `graph.value_cells` (the
/// `guarded_groups[*].members` / `[*].else_members` arms of
/// `crates/reify-eval/src/graph.rs`), cloning its `cell_type` verbatim. A
/// scan of `value_cells` alone therefore proves ABSENCE over a strict subset
/// of what reaches the graph, and answers "safe" on a graph that is not —
/// measured on this fixture: stage 1 all-representable → `None`, while the
/// graph in fact carries `Bearing.seal : TypeParam("T")` and
/// `compute_diagnostics` PANICS at `crates/reify-eval/src/engine_eval.rs:210`.
///
/// Keep both fixtures: the unguarded one pins the plain arm, this one pins the
/// guarded arms, and only together do they span the collections stage 1 must
/// scan.
pub(crate) const GUARDED_GROUP_AUTO_FAIL_TYPEPARAM_SRC: &str = r#"trait Seal {}
structure def GasketSeal : Seal { param d : Real = 2.0 }
structure def Bearing<T: Seal> {
    param bore : Real = 1.0
    where bore > 0.5 {
        param seal : T
    }
    constraint bore > 10.0
}
structure def Assembly { sub b = Bearing<auto: Seal>() }
"#;

/// Containment fixture for the PRE-EXISTING, compile-CLEAN half of the
/// guard's blast radius: a generic structure merely DECLARED and never
/// instantiated.
///
/// Nothing here fails, and nothing here involves `auto:` — the compiler emits
/// ZERO diagnostics under BOTH checkers — yet `Bearing`'s own template reaches
/// the evaluation graph carrying `Bearing.seal : TypeParam("T")`, which is
/// exactly what `assert_value_cell_types_representable` panics on. This shape
/// is therefore NOT newly reachable via task #6798's checker swap (contrast
/// [`AUTO_FAIL_UNSUBSTITUTED_TYPEPARAM_SRC`], whose whole point is that it
/// is); it is the pre-existing crash the containment guard also happens to
/// contain, named in
/// [`crate::diagnostics::eval_guard::first_unrepresentable_cell`]'s
/// "## Blast radius" section.
///
/// Its load-bearing property is the ABSENCE of a compile diagnostic. The
/// guard's original justification for degrading quietly was "the compile-stage
/// `E_AUTO_TYPE_PARAM_*` error is still delivered, and it is the actionable
/// signal" — false here, which is why the guard must say something in the
/// editor on its own account. Pinned by
/// [`assert_compile_clean_typeparam_fixtures_carry_no_error`].
pub(crate) const DECLARED_ONLY_GENERIC_TYPEPARAM_SRC: &str = r#"trait Seal {}
structure def GasketSeal : Seal { param d : Real = 2.0 }
structure def Bearing<T: Seal> {
    param bore : Real = 1.0
    param seal : T
}
"#;

/// Containment fixture for the second compile-CLEAN shape: an EXPLICIT generic
/// instantiation, `Bearing<GasketSeal>()` rather than `Bearing<auto: Seal>()`.
///
/// Sibling of [`DECLARED_ONLY_GENERIC_TYPEPARAM_SRC`] and kept alongside it
/// rather than folded into it: the two reach the same hazard through
/// different compiler paths (no instantiation at all, versus an instantiation
/// that names its type argument outright), and measured, this one leaves TWO
/// unrepresentable cells — `Assembly.b.seal` as well as `Bearing.seal` —
/// where the declared-only shape leaves one. Both compile CLEAN under both
/// checkers.
pub(crate) const EXPLICIT_GENERIC_INSTANTIATION_TYPEPARAM_SRC: &str = r#"trait Seal {}
structure def GasketSeal : Seal { param d : Real = 2.0 }
structure def Bearing<T: Seal> {
    param bore : Real = 1.0
    param seal : T
}
structure def Assembly { sub b = Bearing<GasketSeal>() }
"#;

/// Anti-vacuity guard for the two compile-CLEAN containment fixtures, pinning
/// the OPPOSITE claim to [`assert_auto_fail_fixture_is_newly_reachable`]'s.
///
/// That guard asserts a fixture's failure is NEWLY reachable (stub clean, real
/// checker errors). This one asserts these two are compile-clean under BOTH
/// checkers — i.e. that the crash they contain is PRE-EXISTING, and, the part
/// the tests actually lean on, that no compile diagnostic accompanies the
/// skipped eval pass. If a future compiler change started erroring on either
/// fixture, every "the editor would otherwise be told nothing" assertion over
/// them would go quietly vacuous.
pub(crate) fn assert_compile_clean_typeparam_fixtures_carry_no_error() {
    for (name, src) in [
        (
            "DECLARED_ONLY_GENERIC_TYPEPARAM_SRC",
            DECLARED_ONLY_GENERIC_TYPEPARAM_SRC,
        ),
        (
            "EXPLICIT_GENERIC_INSTANTIATION_TYPEPARAM_SRC",
            EXPLICIT_GENERIC_INSTANTIATION_TYPEPARAM_SRC,
        ),
    ] {
        let parsed = reify_compiler::parse_with_stdlib(src, ModulePath::single("test"));
        assert!(
            parsed.errors.is_empty(),
            "anti-vacuity guard: {name} must still PARSE — it is ordinary \
             `.ri`, and a parse error would make every containment assertion \
             over it vacuous. parse errors: {:#?}",
            parsed.errors
        );
        for (checker_name, compiled) in [
            (
                "compile-time stub",
                reify_compiler::compile_with_stdlib(&parsed),
            ),
            (
                "real SimpleConstraintChecker",
                reify_compiler::compile_with_stdlib_checked(&parsed, &SimpleConstraintChecker),
            ),
        ] {
            let errors: Vec<_> = compiled
                .diagnostics
                .iter()
                .filter(|d| d.severity == reify_core::Severity::Error)
                .collect();
            assert!(
                errors.is_empty(),
                "anti-vacuity guard: {name} must compile with ZERO \
                 Error-severity diagnostics under the {checker_name} — the \
                 ABSENCE of a compile diagnostic is what makes this fixture \
                 the case the guard cannot ride on someone else's error \
                 message, and the reason it must report itself in the editor. \
                 errors: {errors:#?}"
            );
        }
    }
}

/// Narrowness fixture: a FAILED `auto:` resolution that is provably SAFE to
/// evaluate, sharing a document with independent eval-time diagnostics.
///
/// [`BT8_CONSTANT_CONSTRAINT_SRC`]'s `auto:` half verbatim (two candidates, a
/// constant `constraint 0 > 1`, `T` deliberately UNUSED in `Bearing`'s body,
/// so no `TypeParam`-typed value cell is ever created), plus an entirely
/// unrelated `Other` structure carrying two eval-time findings: a circular
/// let-binding and its own violated constraint.
///
/// This is the measured OVER-FIRE case for the `AutoTypeParam*`
/// diagnostic-code proxy the containment guard used to be. Under that proxy
/// the single `auto:` clause suppressed the whole document's eval pass, losing
/// three real diagnostics — `circular let-binding dependency in template
/// Other: [a, b]`, `constraint Other#constraint[0] violated`, and
/// `constraint Bearing#constraint[0] violated` — and, via the identical guard
/// in `AnalysisContext::from_parsed`, blanking `check_result.values` so
/// hover/completion showed no computed values file-wide.
///
/// Do not "simplify" either half away: `T` must stay UNUSED (that is what
/// makes eval provably safe and the suppression provably wrong), and `Other`
/// must stay INDEPENDENT of `Bearing` (a dependency would make "eval still
/// ran" ambiguous).
pub(crate) const UNUSED_TYPEPARAM_AUTO_FAIL_WITH_EVAL_DIAGS_SRC: &str = r#"trait Seal {}
structure def GasketSeal : Seal { param d : Real = 2.0 }
structure def OringSeal : Seal { param d : Real = 3.0 }
structure def Bearing<T: Seal> {
    param bore : Real = 1.0
    constraint 0 > 1
}
structure def Assembly { sub b = Bearing<auto: Seal>() }
structure Other {
    let a = b + 1
    let b = a + 1
    constraint 0 > 1
}
"#;

/// Narrowness fixture: an Error-severity compile diagnostic that is NOT an
/// `auto:` failure, sharing a document with an eval-time diagnostic.
///
/// `nope` is unresolved → `E_UNRESOLVED_NAME` at Error severity, while the
/// graph stays fully representable; `S`'s cyclic let-bindings produce
/// `circular let-binding dependency in template S: [a, b]` at eval time (the
/// same engine finding `eval_diagnostics_surfaced_in_stateful_pipeline`
/// exercises).
///
/// Exists so the "the LSP evaluates THROUGH non-fatal compile errors on
/// purpose" claim is pinned at the production entry points rather than only at
/// the predicate. A future widening of the call sites to the CLI's blanket
/// `any(|d| d.severity == Severity::Error)` gate would go RED here.
pub(crate) const NON_AUTO_COMPILE_ERROR_WITH_EVAL_DIAG_SRC: &str = r#"structure S {
    let a = b + 1
    let b = a + 1
}
structure T { param x : Real = nope }
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

/// Anti-vacuity guard for [`GUARDED_GROUP_AUTO_FAIL_TYPEPARAM_SRC`], mirroring
/// [`assert_auto_fail_fixture_is_newly_reachable`]'s clean-vs-NO_CANDIDATE
/// claim on the guarded-member shape.
///
/// Not a call through to that function with a different const: the two pin the
/// same claim over two different fixtures, and keeping them separate is what
/// lets each name its own fixture in its failure message. Both are needed —
/// if the guarded shape ever stopped diverging, the containment tests over it
/// would pass vacuously while the plain shape's guard stayed green and hid it.
pub(crate) fn assert_guarded_group_fixture_is_newly_reachable() {
    let parsed = reify_compiler::parse_with_stdlib(
        GUARDED_GROUP_AUTO_FAIL_TYPEPARAM_SRC,
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
         GUARDED_GROUP_AUTO_FAIL_TYPEPARAM_SRC with ZERO Error-severity \
         diagnostics — that is what makes the panic hazard NEWLY reachable \
         via task #6798's checker swap rather than pre-existing. If the stub \
         now errors too, this fixture has stopped demonstrating \
         newly-reachable failure and the containment tests over it are \
         vacuous. stub diagnostics: {:#?}",
        stub.diagnostics
    );
    assert!(
        real.diagnostics
            .iter()
            .any(|d| d.severity == reify_core::Severity::Error
                && d.code == Some(DiagnosticCode::AutoTypeParamNoCandidate)),
        "anti-vacuity guard: the real SimpleConstraintChecker must fail \
         `auto:` resolution on GUARDED_GROUP_AUTO_FAIL_TYPEPARAM_SRC with an \
         Error-severity AutoTypeParamNoCandidate — that failed resolution is \
         what leaves the unsubstituted TypeParam cell inside the guarded \
         group, which is the cell the containment guard exists to keep away \
         from the engine (task #6851). real-checker diagnostics: {:#?}",
        real.diagnostics
    );
}

/// Anti-vacuity guard shared by every BT8 forward test: assert the
/// compile-time stub and the real `SimpleConstraintChecker` still
/// genuinely diverge on [`BT8_CONSTANT_CONSTRAINT_SRC`] before any
/// downstream assertion compares an LSP entry point against the real
/// checker's verdict.
///
/// Extracted so the guard cannot drift between its two call sites —
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
