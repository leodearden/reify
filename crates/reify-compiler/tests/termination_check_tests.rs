//! Termination check tests for recursive structures (Task 204).
//!
//! Tests that:
//! 1. Sub where-clauses compile into SubComponentDecl.guard_expr.
//! 2. Recursive subs without guards emit errors.
//! 3. Recursive subs with valid guards and decremented params produce no errors.
//! 4. Guard that doesn't reference Int/Bool params emits error.
//! 5. Guard that references unmodified Int param emits error.
//! 6. Bool param with negation is valid.
//! 7. undef in guard-referenced recursive sub args is forbidden.
//! 8. Mutual recursion without guards emits errors for both sides.
//! 9. Non-recursive structures with subs are NOT flagged.
//! 10. Block-level guards satisfy termination requirement.

use reify_ir::*;
use reify_compiler::*;
use reify_test_support::{compile_first_template, compile_source};
use reify_core::*;

/// Helper: compile and destructure into templates + diagnostics.
fn compile_all(source: &str) -> (Vec<TopologyTemplate>, Vec<Diagnostic>) {
    let compiled = compile_source(source);
    (compiled.templates, compiled.diagnostics)
}

// ─── Step 1: sub where-clause compiles into guard_expr ───────────────────────

/// A sub with `where n > 0` should have guard_expr set to Some(BinOp::Gt).
#[test]
fn sub_where_clause_compiles_to_guard_expr() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n - 1) where n > 0
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // Task 410 item 1: assert compilation produces no errors.
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors for valid recursive sub with guard, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let child_sub = template
        .sub_components
        .iter()
        .find(|s| s.name == "child")
        .expect("expected sub named 'child'");

    assert!(
        matches!(child_sub.guard_state, GuardState::Compiled(_)),
        "sub 'child' with where clause should have GuardState::Compiled(_), got {:?}",
        child_sub.guard_state
    );

    // The compiled guard should be BinOp::Gt (n > 0)
    let guard = child_sub.guard_state.compiled().unwrap();
    assert!(
        matches!(&guard.kind, CompiledExprKind::BinOp { op: BinOp::Gt, .. }),
        "guard should be BinOp::Gt for 'n > 0', got {:?}",
        guard.kind
    );
}

/// A sub without a where-clause should have GuardState::None.
#[test]
fn sub_without_where_clause_has_guard_state_none() {
    let source = r#"
structure Inner { param x : Int = 1 }
structure Outer {
    sub inner = Inner(x: 5)
}
"#;

    let (templates, _) = compile_all(source);
    let outer = templates
        .iter()
        .find(|t| t.name == "Outer")
        .expect("expected Outer");

    let inner_sub = outer
        .sub_components
        .iter()
        .find(|s| s.name == "inner")
        .expect("expected sub named 'inner'");

    assert!(
        matches!(inner_sub.guard_state, GuardState::None),
        "sub 'inner' without where clause should have GuardState::None, got {:?}",
        inner_sub.guard_state
    );
}

// ─── Step 3: recursive sub without guard emits error ─────────────────────────

/// A recursive sub without a where-clause guard should emit an error.
#[test]
fn recursive_sub_without_guard_emits_error() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n - 1)
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        !errors.is_empty(),
        "expected error for recursive sub without termination guard, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // Error message should mention recursive sub and missing termination
    let msg_ok = errors.iter().any(|d| {
        let msg = d.message.to_lowercase();
        (msg.contains("recursive") || msg.contains("recursion"))
            && (msg.contains("termination")
                || msg.contains("no termination")
                || msg.contains("guard"))
    });
    assert!(
        msg_ok,
        "error message should mention recursive sub and missing termination, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── Step 5: recursive sub with valid guard and decremented param — no error ──

/// A recursive sub with `where n > 0` and arg `n: n - 1` should produce NO error.
#[test]
fn recursive_sub_with_valid_guard_no_error() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n - 1) where n > 0
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        errors.is_empty(),
        "expected no errors for recursive sub with valid guard and decremented param, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── Step 7: guard that doesn't reference Int/Bool param emits error ──────────

/// A recursive sub with a guard that only uses literals (no param refs) should emit an error.
#[test]
fn recursive_sub_guard_without_param_ref_emits_error() {
    let source = r#"
structure S {
    param n : Int = 5
    param active : Bool = true
    sub child = S(n: n - 1) where 1 > 0
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        !errors.is_empty(),
        "expected error when guard does not reference any Int/Bool param, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // Should mention guard not referencing a param
    let msg_ok = errors.iter().any(|d| {
        let msg = d.message.to_lowercase();
        msg.contains("guard")
            && (msg.contains("param") || msg.contains("int") || msg.contains("bool"))
    });
    assert!(
        msg_ok,
        "error should mention guard not referencing Int/Bool param, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── Step 9: guard references Int param but param is NOT modified ─────────────

/// A recursive sub with guard referencing n but passing n unchanged should emit an error.
#[test]
fn recursive_sub_guard_param_not_modified_emits_error() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n) where n > 0
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        !errors.is_empty(),
        "expected error when guard-referenced param is passed unchanged, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // Should mention param not being decremented
    let msg_ok = errors.iter().any(|d| {
        let msg = d.message.to_lowercase();
        msg.contains("decrement")
            || msg.contains("unchanged")
            || msg.contains("modif")
            || msg.contains("toward")
    });
    assert!(
        msg_ok,
        "error should mention param not being decremented toward base case, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── Step 11: Bool param with negation is valid ───────────────────────────────

/// A recursive sub with Bool param guard and negation (`!active`) is a valid pattern.
#[test]
fn recursive_sub_bool_guard_with_negation_no_error() {
    let source = r#"
structure S {
    param active : Bool = true
    sub child = S(active: !active) where active
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        errors.is_empty(),
        "expected no errors for Bool param guard with negation, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── Step 13: undef in recursive sub args is forbidden ───────────────────────

/// A recursive sub using `undef` in args should emit an explicit error.
#[test]
fn recursive_sub_with_undef_arg_emits_error() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: undef) where n > 0
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        !errors.is_empty(),
        "expected error for recursive sub using undef in args, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // Should mention undef
    let msg_ok = errors.iter().any(|d| {
        let msg = d.message.to_lowercase();
        msg.contains("undef")
    });
    assert!(
        msg_ok,
        "error should mention undef being forbidden, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── Step 15: mutual recursion without guards emits errors for both ───────────

/// Mutual recursion (A→B, B→A) without guards should emit errors for both recursive subs.
#[test]
fn mutual_recursion_without_guards_emits_errors() {
    let source = r#"
structure A {
    sub b = B()
}
structure B {
    sub a = A()
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        errors.len() >= 2,
        "expected at least 2 errors for mutual recursion without guards (one per recursive sub), got {} errors: {:?}",
        errors.len(),
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── Step 17: non-recursive structure with sub is NOT flagged ─────────────────

/// A non-recursive structure with a sub should produce zero termination errors.
#[test]
fn non_recursive_sub_not_flagged() {
    let source = r#"
structure Inner { param x : Int = 1 }
structure Outer {
    sub inner = Inner(x: 5)
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        errors.is_empty(),
        "expected no errors for non-recursive structure, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── Step 19: recursive sub inside block guard (when n > 0 { sub child = S(...) }) ──

// ─── Step 21: block guard sub — documents compiler limitation ────────────────

/// Sub declarations inside `where {}` blocks are now explicitly rejected with a
/// 'not yet supported' diagnostic error (instead of being silently dropped).
/// As a result, `sub_components` is still empty (the sub is not compiled into the
/// template) and `is_recursive` remains false, but a diagnostic error is now emitted.
#[test]
fn block_guard_sub_not_yet_compiled() {
    let source = r#"
structure S {
    param n : Int = 5
    where n > 0 {
        sub child = S(n: n - 1)
    }
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // Sub inside where{} block is rejected with a diagnostic — sub_components is still empty.
    assert!(
        template.sub_components.is_empty(),
        "expected sub_components to be empty because compile_guarded_members rejects Sub \
         declarations inside where{{}} blocks, but got: {:?}",
        template
            .sub_components
            .iter()
            .map(|s| &s.name)
            .collect::<Vec<_>>()
    );

    // Because no sub_components exist, Tarjan SCC finds no cycle → is_recursive == false.
    assert!(
        !template.is_recursive,
        "expected is_recursive == false because no sub_components exist for Tarjan to analyse"
    );

    // A 'not yet supported' error diagnostic must be emitted for the sub in the guard block.
    let unsupported_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == reify_core::Severity::Error
                && d.message.to_lowercase().contains("not yet supported")
                && d.message.to_lowercase().contains("sub")
        })
        .collect();
    assert_eq!(
        unsupported_errors.len(),
        1,
        "expected exactly one 'not yet supported' error for sub in block guard, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── Task 408 step 5: failed guard compilation must not cascade to extra error ─

/// A where-clause referencing an undefined name (`where unknown_var > 0`) should
/// emit only the "unresolved name" diagnostic from compile_expr. It must NOT also
/// emit a spurious "guard doesn't reference params" error. Currently both are emitted
/// because the Undef fallback is stored as `guard_expr: Some(Undef)`, causing the
/// termination check to find no ValueRefs and fire the "references no param" error.
#[test]
fn failed_guard_compilation_no_cascading_error() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n - 1) where unknown_var > 0
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    // Must NOT have the specific guard-references-no-param cascading error from the
    // termination check piling on top of the underlying "unresolved name" compile error.
    let guard_ref_error = errors.iter().any(|d| {
        let msg = d.message.to_lowercase();
        msg.contains("guard")
            && (msg.contains("param") || msg.contains("int") || msg.contains("bool"))
    });
    assert!(
        !guard_ref_error,
        "termination check should NOT emit 'guard references no param' when guard failed to compile; got errors: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // There should be at least the compile error for unknown_var.
    let has_compile_error = errors.iter().any(|d| {
        let msg = d.message.to_lowercase();
        msg.contains("unresolved") || msg.contains("unknown")
    });
    assert!(
        has_compile_error,
        "expected at least the 'unresolved name: unknown_var' compile error, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── Task 408 step 3: undef in non-guard-referenced arg should be allowed ────

/// A recursive sub `S(n: n - 1, label: undef) where n > 0` should NOT emit
/// the termination-specific "undef not allowed in recursive sub arguments" error.
/// `label` is not referenced by the guard `n > 0`, so it is termination-irrelevant.
///
/// Note: `undef` is an unresolved name so a generic "unresolved name" compile error
/// is expected — but the termination check must NOT pile on an extra error for it.
#[test]
fn undef_in_non_guard_arg_is_allowed() {
    let source = r#"
structure S {
    param n : Int = 5
    param label : Int = 0
    sub child = S(n: n - 1, label: undef) where n > 0
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    // The termination check must not emit its own "undef not allowed" error for label,
    // because label is not referenced by the guard.
    let termination_undef_error = diagnostics.iter().any(|d| {
        d.severity == Severity::Error
            && d.message
                .to_lowercase()
                .contains("undef is not allowed as a non-termination")
    });
    assert!(
        !termination_undef_error,
        "termination check should NOT flag undef in non-guard-referenced arg `label`; got: {:?}",
        diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ─── Task 408 step 8: undef in a guard-referenced arg is still rejected ──────

/// `S(n: undef) where n > 0` — `n` IS referenced by the guard, so undef in that
/// arg must still be caught by the scoped undef check. This verifies that step 4's
/// narrowing didn't accidentally remove the check for guard-relevant args.
///
/// Note: `undef` is an unresolved name, so there will also be a generic
/// "unresolved name" compile error — the test just checks for the termination-
/// specific "undef not allowed" diagnostic to confirm the scoped check fires.
#[test]
fn undef_in_guard_referenced_arg_still_rejected() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: undef) where n > 0
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    let termination_undef_error = diagnostics.iter().any(|d| {
        d.severity == Severity::Error
            && d.message
                .to_lowercase()
                .contains("undef is not allowed as a non-termination")
    });
    assert!(
        termination_undef_error,
        "termination check SHOULD flag undef in guard-referenced arg `n`; got errors: {:?}",
        diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

// ─── Task 408 step 7: subtraction still satisfies termination (regression) ───

/// Confirm that `S(n: n - 1) where n > 0` (the canonical decrement pattern)
/// still produces zero errors after the BinOp::Add removal in step 2.
/// This is a targeted regression guard named after the Add fix.
#[test]
fn subtraction_still_satisfies_termination() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n - 1) where n > 0
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        errors.is_empty(),
        "BinOp::Sub must still satisfy termination after BinOp::Add removal; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── Task 408 step 1: BinOp::Add is not a valid termination-modifier ─────────

/// A recursive sub with `S(n: n + 1) where n > 0` should emit an error because
/// addition diverges — `n` increases and never reaches 0. This is a false negative
/// in the current implementation (BinOp::Add is accepted as a modifying operation).
#[test]
fn add_op_does_not_satisfy_termination() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n + 1) where n > 0
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        !errors.is_empty(),
        "expected error for `n + 1` (diverges — n increases, never reaches base case), got no errors"
    );

    let msg_ok = errors.iter().any(|d| {
        let msg = d.message.to_lowercase();
        msg.contains("decrement") || msg.contains("modif") || msg.contains("toward")
    });
    assert!(
        msg_ok,
        "error should mention decrement/modifying/toward base case, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// A recursive sub inside a block guard `where n > 0 { sub child = S(n: n-1) }`
/// is now explicitly rejected with a 'not yet supported' diagnostic error.
///
/// Previously this test was ignored because it passed for the wrong reason:
/// `compile_guarded_members` silently dropped Sub declarations via `_ => {}`, leaving
/// `sub_components` empty so Tarjan SCC found no cycle and no error was emitted.
///
/// Now that `compile_guarded_members` emits a diagnostic for Sub in guarded blocks,
/// this test is un-ignored and verifies that exactly one 'not yet supported' error
/// is produced — confirming the sub is explicitly rejected rather than silently dropped.
#[test]
fn recursive_sub_inside_block_guard_emits_unsupported_error() {
    let source = r#"
structure S {
    param n : Int = 5
    where n > 0 {
        sub child = S(n: n - 1)
    }
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    // Exactly one 'not yet supported' error for the sub in the guarded block.
    let unsupported_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.message.to_lowercase().contains("not yet supported")
                && d.message.to_lowercase().contains("sub")
        })
        .collect();

    assert_eq!(
        unsupported_errors.len(),
        1,
        "expected exactly one 'not yet supported' error for sub in block guard, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── Task 410 item 2: is_recursive flag set on recursive structures ─────────

/// A directly recursive structure (S contains sub S) should have is_recursive == true
/// after the post-compilation SCC pass.
#[test]
fn recursive_structure_has_is_recursive_true() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n - 1) where n > 0
}
"#;

    let (template, _) = compile_first_template(source);

    assert!(
        template.is_recursive,
        "expected is_recursive == true for a directly recursive structure"
    );
}

// ─── Task 410 item 3: cycle detection warning diagnostic ────────────────────

/// The SCC pass should emit a warning diagnostic containing
/// "recursive structure cycle detected: ..." for recursive structures.
#[test]
fn recursive_structure_emits_cycle_warning() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n - 1) where n > 0
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    let cycle_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Warning
                && d.message
                    .to_lowercase()
                    .contains("recursive structure cycle detected")
        })
        .collect();

    assert!(
        !cycle_warnings.is_empty(),
        "expected 'recursive structure cycle detected' warning, got diagnostics: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── Task 410 item 4: undef in recursive sub args WITHOUT guard ─────────────

/// A recursive sub with undef args and NO guard should emit the "no termination
/// condition" error (not a undef-specific error). The existing test at step 13
/// covers undef WITH a guard; this covers the guardless case.
#[test]
fn recursive_sub_with_undef_arg_without_guard_emits_error() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: undef)
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        !errors.is_empty(),
        "expected error for recursive sub with undef arg and no guard, got: {:?}",
        diagnostics.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // Should get the "no termination condition" error since there's no guard at all.
    let has_termination_error = errors.iter().any(|d| {
        let msg = d.message.to_lowercase();
        (msg.contains("recursive") || msg.contains("recursion"))
            && (msg.contains("termination")
                || msg.contains("no termination")
                || msg.contains("guard"))
    });
    assert!(
        has_termination_error,
        "error should mention missing termination condition, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── Task 1296: broken guard must not cascade to "no termination" error ───────

/// A where-clause that fails to compile (`where unknown_var > 0`) should NOT cause
/// the termination check to emit "recursive sub has no termination condition: add a
/// where clause". The user DID write a where clause — it just failed to compile.
/// Seeing both the underlying "unresolved name" error AND the "add a where clause"
/// error is misleading and actionably wrong.
///
/// Currently this test FAILS because `sub_guard_expr` is set to `None` when guard
/// compilation emits any diagnostic, and the termination check cannot distinguish
/// "no guard" from "broken guard" — it emits the "no termination condition" error
/// unconditionally whenever `guard_expr == None`.
#[test]
fn broken_guard_does_not_emit_no_termination_error() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n - 1) where unknown_var > 0
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    // (a) Sanity check: compilation must actually fail. We don't pin the
    // specific wording of the underlying compile error — the goal here is
    // only to confirm the broken `where unknown_var > 0` clause produced a
    // diagnostic. Substring-matching on "unresolved"/"unknown" silently
    // passes if the message is reworded (e.g. to "undeclared identifier"),
    // which would mask a real regression. Asserting `!errors.is_empty()`
    // catches the only failure mode the (a) check was ever meant to catch.
    assert!(
        !errors.is_empty(),
        "expected at least one compile error for `where unknown_var > 0`, got none — \
         the (b) negative-assertion below is only meaningful when compilation actually fails"
    );

    // (b) The termination check must NOT also emit "no termination condition: add a where clause".
    // The user wrote a where clause — it just failed to compile. The "add a where clause"
    // message is incorrect and actionably misleading.
    let has_no_termination_error = errors.iter().any(|d| {
        let msg = d.message.to_lowercase();
        msg.contains("no termination")
            || (msg.contains("recursive sub") && msg.contains("where clause"))
    });
    assert!(
        !has_no_termination_error,
        "termination check must NOT emit 'no termination condition: add a where clause' when the \
         user already wrote a where clause that failed to compile; got errors: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── Task 2271 structural: GuardState enum wiring ────────────────────────────

/// Verifies the three variants of `GuardState` on a compiled `SubComponentDecl`:
///
/// (a) Valid guard:  `GuardState::Compiled(_)` — user wrote `where n > 0` and it compiled.
/// (b) Broken guard: `GuardState::Broken`      — user wrote `where unknown_var > 0`; compile failed.
/// (c) No guard:     `GuardState::None`         — user wrote no `where` clause.
///
/// Unlike the old (Option, bool) pair where (b) and (c) both had `guard_expr == None`
/// and only differed in `guard_compile_failed`, the enum makes each state a distinct variant.
#[test]
fn compiled_sub_carries_guard_state_enum_three_states() {
    // (a) Valid guard: should produce GuardState::Compiled.
    let source_valid = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n - 1) where n > 0
}
"#;
    let (template, _) = compile_first_template(source_valid);
    let child = template
        .sub_components
        .iter()
        .find(|s| s.name == "child")
        .expect("expected sub named 'child'");
    assert!(
        matches!(child.guard_state, GuardState::Compiled(_)),
        "(a) valid guard: expected GuardState::Compiled(_), got {:?}",
        child.guard_state
    );

    // (b) Broken guard: should produce GuardState::Broken.
    // compile_first_template uses partial-recovery semantics: it returns the template
    // even when guard compilation emits Severity::Error diagnostics. The sub is still
    // present in sub_components with GuardState::Broken, and the compile error is already
    // recorded in diagnostics (verified below).
    let source_broken = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n - 1) where unknown_var > 0
}
"#;
    let (template, diags) = compile_first_template(source_broken);
    let child = template
        .sub_components
        .iter()
        .find(|s| s.name == "child")
        .expect("expected sub named 'child'");
    assert!(
        matches!(child.guard_state, GuardState::Broken),
        "(b) broken guard: expected GuardState::Broken, got {:?}",
        child.guard_state
    );
    // Broken guards must still emit a compile error (not silently swallow it).
    assert!(
        diags.iter().any(|d| d.severity == Severity::Error),
        "(b) broken guard: expected at least one Severity::Error diagnostic for the failed compile, got: {:?}",
        diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // (c) No guard: should produce GuardState::None.
    let source_no_guard = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n - 1)
}
"#;
    let (template, _) = compile_first_template(source_no_guard);
    let child = template
        .sub_components
        .iter()
        .find(|s| s.name == "child")
        .expect("expected sub named 'child'");
    assert!(
        matches!(child.guard_state, GuardState::None),
        "(c) no guard: expected GuardState::None, got {:?}",
        child.guard_state
    );
}
