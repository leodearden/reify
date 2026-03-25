//! Termination check tests for recursive structures (Task 204).
//!
//! Tests that:
//! 1. Sub where-clauses compile into SubComponentDecl.guard_expr.
//! 2. Recursive subs without guards emit errors.
//! 3. Recursive subs with valid guards and decremented params produce no errors.
//! 4. Guard that doesn't reference Int/Bool params emits error.
//! 5. Guard that references unmodified Int param emits error.
//! 6. Bool param with negation is valid.
//! 7. undef in recursive sub args is forbidden.
//! 8. Mutual recursion without guards emits errors for both sides.
//! 9. Non-recursive structures with subs are NOT flagged.
//! 10. Block-level guards satisfy termination requirement.

use reify_compiler::*;
use reify_types::*;

/// Helper: parse source and compile, returning all templates and diagnostics.
fn compile_all(source: &str) -> (Vec<TopologyTemplate>, Vec<Diagnostic>) {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);
    let compiled = reify_compiler::compile(&parsed);
    (compiled.templates, compiled.diagnostics)
}

/// Helper: parse source and compile, returning first template.
fn compile_first_template(source: &str) -> (TopologyTemplate, Vec<Diagnostic>) {
    let (templates, diags) = compile_all(source);
    let template = templates.into_iter().next().expect("expected 1 template");
    (template, diags)
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

    let (template, _diagnostics) = compile_first_template(source);

    let child_sub = template.sub_components.iter()
        .find(|s| s.name == "child")
        .expect("expected sub named 'child'");

    assert!(
        child_sub.guard_expr.is_some(),
        "sub 'child' with where clause should have guard_expr set, got None"
    );

    // The guard_expr should be BinOp::Gt (n > 0)
    let guard = child_sub.guard_expr.as_ref().unwrap();
    assert!(
        matches!(&guard.kind, CompiledExprKind::BinOp { op: BinOp::Gt, .. }),
        "guard_expr should be BinOp::Gt for 'n > 0', got {:?}", guard.kind
    );
}

/// A sub without a where-clause should have guard_expr == None.
#[test]
fn sub_without_where_clause_has_no_guard_expr() {
    let source = r#"
structure Inner { param x : Int = 1 }
structure Outer {
    sub inner = Inner(x: 5)
}
"#;

    let (templates, _) = compile_all(source);
    let outer = templates.iter().find(|t| t.name == "Outer").expect("expected Outer");

    let inner_sub = outer.sub_components.iter()
        .find(|s| s.name == "inner")
        .expect("expected sub named 'inner'");

    assert!(
        inner_sub.guard_expr.is_none(),
        "sub 'inner' without where clause should have guard_expr == None"
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

    let errors: Vec<_> = diagnostics.iter()
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
        (msg.contains("recursive") || msg.contains("recursion")) &&
        (msg.contains("termination") || msg.contains("no termination") || msg.contains("guard"))
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

    let errors: Vec<_> = diagnostics.iter()
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

    let errors: Vec<_> = diagnostics.iter()
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
        msg.contains("guard") && (msg.contains("param") || msg.contains("int") || msg.contains("bool"))
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

    let errors: Vec<_> = diagnostics.iter()
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
        msg.contains("decrement") || msg.contains("unchanged") || msg.contains("modif") || msg.contains("toward")
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

    let errors: Vec<_> = diagnostics.iter()
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

    let errors: Vec<_> = diagnostics.iter()
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

    let errors: Vec<_> = diagnostics.iter()
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

    let errors: Vec<_> = diagnostics.iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        errors.is_empty(),
        "expected no errors for non-recursive structure, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── Step 19: recursive sub inside block guard (when n > 0 { sub child = S(...) }) ──

/// A recursive sub inside a block guard `when n > 0 { sub child = S(n: n-1) }`
/// should be recognized as having a termination condition via the enclosing block guard.
/// Assert NO error.
#[test]
fn recursive_sub_inside_block_guard_no_error() {
    let source = r#"
structure S {
    param n : Int = 5
    where n > 0 {
        sub child = S(n: n - 1)
    }
}
"#;

    let (_templates, diagnostics) = compile_all(source);

    let errors: Vec<_> = diagnostics.iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        errors.is_empty(),
        "expected no errors for recursive sub protected by enclosing block guard, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}
