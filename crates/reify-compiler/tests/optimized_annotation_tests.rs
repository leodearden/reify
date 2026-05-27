//! Compiler-level tests for the `@optimized` annotation on constraint defs
//! (Task 273 — @optimized: plumbing).
//!
//! Exercises:
//!   - `@optimized("target")` on a `constraint def` is accepted by the validator
//!     (no "@optimized is not valid" warning).
//!   - Instantiating such a def in a structure propagates the target onto the
//!     resulting `CompiledConstraint::optimized_target`.
//!   - An un-annotated constraint def yields `optimized_target = None`.

use reify_compiler::{CompiledConstraint, CompiledFunction, CompiledModule, TopologyTemplate};
use reify_test_support::compile_source;
use reify_core::{Diagnostic, Severity};

// ── Helpers ─────────────────────────────────────────────────────────────────

fn error_diags(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect()
}

fn warning_diags(diags: &[Diagnostic]) -> Vec<&Diagnostic> {
    diags
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect()
}

fn template_named<'a>(module: &'a CompiledModule, name: &str) -> &'a TopologyTemplate {
    module
        .templates
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("expected template '{name}' in compiled module"))
}

// ── Step 3: acceptance + field existence ────────────────────────────────────

#[test]
fn optimized_annotation_on_constraint_def_is_accepted() {
    let source = r#"
@optimized("kernel::foo")
constraint def MinWall {
    param w: Length
    w > 0mm
}
structure S {
    param t: Length
    constraint MinWall(w: t)
}
"#;
    let module = compile_source(source);

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // No warning should claim @optimized is invalid on a constraint_def.
    let bad_optimized_warnings: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| d.message.contains("@optimized is not valid"))
        .collect();
    assert!(
        bad_optimized_warnings.is_empty(),
        "@optimized on constraint_def should not warn; got: {:?}",
        bad_optimized_warnings
    );

    // Compile-time field assertion: if `CompiledConstraint::optimized_target`
    // is missing this test fails to compile, satisfying step-3's contract.
    let tmpl = template_named(&module, "S");
    assert_eq!(
        tmpl.constraints.len(),
        1,
        "expected 1 compiled constraint in S, got {}",
        tmpl.constraints.len()
    );
    let cc: &CompiledConstraint = &tmpl.constraints[0];
    // This read forces the field to exist on the struct — the test files a
    // compile error if the field is removed or renamed.
    let _target: &Option<String> = &cc.optimized_target;
}

// ── Step 5: @optimized target flows through to CompiledConstraint ───────────

#[test]
fn optimized_target_flows_from_constraint_def_to_compiled_constraint() {
    let source = r#"
@optimized("geo::coincident")
constraint def Coincident {
    param a: Real
    param b: Real
    a == b
}
structure S {
    param x: Real
    param y: Real
    constraint Coincident(a: x, b: y)
}
"#;
    let module = compile_source(source);

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let tmpl = template_named(&module, "S");
    assert_eq!(
        tmpl.constraints.len(),
        1,
        "expected 1 compiled constraint in S, got {}",
        tmpl.constraints.len()
    );
    let cc: &CompiledConstraint = &tmpl.constraints[0];
    assert_eq!(
        cc.optimized_target,
        Some("geo::coincident".to_string()),
        "expected optimized_target to be propagated from the constraint def's @optimized annotation"
    );
}

#[test]
fn constraint_def_without_optimized_has_none_target() {
    let source = r#"
constraint def Plain {
    param a: Real
    param b: Real
    a == b
}
structure S {
    param x: Real
    param y: Real
    constraint Plain(a: x, b: y)
}
"#;
    let module = compile_source(source);

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let tmpl = template_named(&module, "S");
    assert_eq!(tmpl.constraints.len(), 1);
    let cc: &CompiledConstraint = &tmpl.constraints[0];
    assert_eq!(
        cc.optimized_target, None,
        "un-annotated constraint def should yield optimized_target=None"
    );
}

// ── Step 13: structure/occurrence contexts still accept @optimized ──────────

/// `@optimized` on a structure must still be accepted after broadening the
/// validator to include `constraint_def`. This guards against a regression
/// where the new arm might have accidentally replaced the old list.
#[test]
fn optimized_on_structure_is_accepted() {
    let module = compile_source(
        r#"
@optimized("kernel::fast")
structure S {
    param x: Real
}
"#,
    );

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let bad: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| d.message.contains("@optimized is not valid"))
        .collect();
    assert!(
        bad.is_empty(),
        "@optimized on structure should not warn; got: {:?}",
        bad
    );
}

/// `@optimized` on an occurrence must still be accepted after broadening the
/// validator.
#[test]
fn optimized_on_occurrence_is_accepted() {
    let module = compile_source(
        r#"
@optimized("kernel::fast")
occurrence def Outer {
    param y: Real = 1.0
}
"#,
    );

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let bad: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| d.message.contains("@optimized is not valid"))
        .collect();
    assert!(
        bad.is_empty(),
        "@optimized on occurrence should not warn; got: {:?}",
        bad
    );
}

/// `@optimized` on a truly-unsupported context (`trait`) should still emit a
/// warning — the broadening should not silently accept all contexts.
///
/// Migrated from `fn` to `trait` in task 3377 because `function` is now in
/// the allow-list; `trait` is still excluded and calls the same diagnostic
/// path via `validate_annotations(_, "trait", _)` in traits.rs.
#[test]
fn optimized_on_unsupported_context_still_warns() {
    let module = compile_source(
        r#"
@optimized("x")
trait T {
    param x: Real
}
"#,
    );

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let warnings: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| d.message.contains("@optimized is not valid"))
        .collect();
    assert!(
        !warnings.is_empty(),
        "expected a warning about @optimized on trait (unsupported context), got none; all diags: {:?}",
        module.diagnostics
    );
}

// ── Missing-target warning must not fire on non-consuming contexts ───────────

/// `@optimized` with no string-literal arg on a *structure* must NOT emit the
/// missing-target warning — the target is only consumed in constraint_def context.
/// Telling a user to add a string that nothing reads is actively harmful.
#[test]
fn optimized_missing_target_on_structure_does_not_warn() {
    let module = compile_source(
        r#"
@optimized
structure S {
    param x: Real
}
"#,
    );

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let missing_target_warnings: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| d.message.contains("requires a string literal target"))
        .collect();
    assert!(
        missing_target_warnings.is_empty(),
        "@optimized (no target) on structure must not warn about missing target; got: {:?}",
        missing_target_warnings
    );
}

/// `@optimized` with no string-literal arg on an *occurrence def* must NOT emit
/// the missing-target warning — same reasoning as for structures above.
#[test]
fn optimized_missing_target_on_occurrence_does_not_warn() {
    let module = compile_source(
        r#"
@optimized
occurrence def O {
    param y: Real = 1.0
}
"#,
    );

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let missing_target_warnings: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| d.message.contains("requires a string literal target"))
        .collect();
    assert!(
        missing_target_warnings.is_empty(),
        "@optimized (no target) on occurrence must not warn about missing target; got: {:?}",
        missing_target_warnings
    );
}

// ── Malformed-annotation diagnostics (reviewer suggestion S4) ───────────────

/// `@optimized` with no string-literal first arg silently routes to the
/// language-level checker; warn the user so they don't spend an afternoon
/// wondering why their registered impl isn't being called.
#[test]
fn optimized_without_string_target_warns() {
    let module = compile_source(
        r#"
@optimized()
constraint def Plain {
    param a: Real
    param b: Real
    a == b
}
"#,
    );

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let relevant: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| {
            d.message
                .contains("@optimized requires a string literal target")
        })
        .collect();
    assert!(
        !relevant.is_empty(),
        "expected a missing-target warning, got none; all diags: {:?}",
        module.diagnostics
    );
}

/// `@optimized(123)` (non-string first arg) should trip the same warning.
#[test]
fn optimized_with_non_string_target_warns() {
    let module = compile_source(
        r#"
@optimized(123)
constraint def Plain {
    param a: Real
    param b: Real
    a == b
}
"#,
    );

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let relevant: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| {
            d.message
                .contains("@optimized requires a string literal target")
        })
        .collect();
    assert!(
        !relevant.is_empty(),
        "expected a missing-target warning for non-string arg, got none; all diags: {:?}",
        module.diagnostics
    );
}

/// Multiple `@optimized` annotations on the same decl: only the first wins in
/// `optimized_target`, so warn on every duplicate past the first.
#[test]
fn multiple_optimized_annotations_warn() {
    let module = compile_source(
        r#"
@optimized("new_target")
@optimized("legacy_target")
constraint def Plain {
    param a: Real
    param b: Real
    a == b
}
"#,
    );

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let relevant: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| d.message.contains("multiple @optimized annotations"))
        .collect();
    assert_eq!(
        relevant.len(),
        1,
        "expected exactly one duplicate-@optimized warning (for the shadowed second annotation), got: {:?}",
        relevant
    );
}

/// `@optimized` (malformed, no args) followed by `@optimized("kernel::foo")` on
/// a constraint_def must resolve to `Some("kernel::foo")` — the extractor must
/// continue scanning past the malformed first entry rather than returning None.
///
/// The user sees a missing-target warning on the malformed first @optimized, but
/// NOT a duplicate-@optimized warning on the second: the malformed entry doesn't
/// count as "first valid", so the valid second annotation is treated as the sole
/// well-formed @optimized rather than a duplicate. This avoids the contradictory
/// signal of "entry #1 is malformed" + "entry #2 is shadowed by entry #1".
#[test]
fn malformed_then_valid_optimized_resolves_to_valid_target() {
    let source = r#"
@optimized()
@optimized("kernel::foo")
constraint def PlainC {
    param a: Real
    param b: Real
    a == b
}
structure S {
    param x: Real
    param y: Real
    constraint PlainC(a: x, b: y)
}
"#;
    let module = compile_source(source);

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // The valid second @optimized target must be returned by the extractor.
    let tmpl = template_named(&module, "S");
    assert_eq!(tmpl.constraints.len(), 1);
    let cc: &CompiledConstraint = &tmpl.constraints[0];
    assert_eq!(
        cc.optimized_target,
        Some("kernel::foo".to_string()),
        "expected extractor to skip past malformed @optimized() and return the valid target; got: {:?}",
        cc.optimized_target
    );

    // The missing-target warning must still fire (on the malformed first @optimized).
    let missing_target_warnings: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| {
            d.message
                .contains("@optimized requires a string literal target")
        })
        .collect();
    assert!(
        !missing_target_warnings.is_empty(),
        "expected a missing-target warning for the malformed first @optimized, got none; all diags: {:?}",
        module.diagnostics
    );

    // No duplicate-@optimized warning: the malformed entry doesn't count as
    // "seen valid", so the second (valid) annotation is the first valid one,
    // not a duplicate.
    let duplicate_warnings: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| d.message.contains("multiple @optimized annotations"))
        .collect();
    assert!(
        duplicate_warnings.is_empty(),
        "a malformed @optimized() before a valid one must not trigger duplicate warning; got: {:?}",
        duplicate_warnings
    );
}

/// `@optimized("foo")` followed by `@optimized()` (malformed) on a constraint_def
/// must still return `Some("foo")` — the valid first entry wins and the malformed
/// second is not a duplicate-eligible entry.
///
/// The malformed second `@optimized()` fires a missing-target warning (it's on a
/// constraint_def without a string arg), but NOT a duplicate warning, because the
/// duplicate check only counts *valid* entries.
#[test]
fn valid_then_malformed_optimized_keeps_valid_target() {
    let source = r#"
@optimized("kernel::foo")
@optimized()
constraint def PlainC {
    param a: Real
    param b: Real
    a == b
}
structure S {
    param x: Real
    param y: Real
    constraint PlainC(a: x, b: y)
}
"#;
    let module = compile_source(source);

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // The valid first @optimized target must be preserved.
    let tmpl = template_named(&module, "S");
    assert_eq!(tmpl.constraints.len(), 1);
    let cc: &CompiledConstraint = &tmpl.constraints[0];
    assert_eq!(
        cc.optimized_target,
        Some("kernel::foo".to_string()),
        "expected valid first @optimized target to be preserved; got: {:?}",
        cc.optimized_target
    );

    // The malformed second @optimized() must fire a missing-target warning.
    let missing_target_warnings: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| {
            d.message
                .contains("@optimized requires a string literal target")
        })
        .collect();
    assert!(
        !missing_target_warnings.is_empty(),
        "expected a missing-target warning for the malformed second @optimized(), got none; all diags: {:?}",
        module.diagnostics
    );

    // The malformed second @optimized() does NOT count as a valid duplicate,
    // so no duplicate warning should fire.
    let duplicate_warnings: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| d.message.contains("multiple @optimized annotations"))
        .collect();
    assert!(
        duplicate_warnings.is_empty(),
        "a malformed @optimized() after a valid one must not trigger duplicate warning; got: {:?}",
        duplicate_warnings
    );
}

/// `@optimized(123)` (non-string first arg) followed by `@optimized("kernel::foo")`
/// on a constraint_def must resolve to `Some("kernel::foo")`. This exercises the
/// non-string-arg branch of the StringLiteral match in `optimized_target` — distinct
/// from the no-args case tested in `malformed_then_valid_optimized_resolves_to_valid_target`.
///
/// Same duplicate-warning contract: the non-string first entry is not counted as a
/// valid @optimized, so the valid second one is the sole well-formed annotation.
#[test]
fn nonstring_then_valid_optimized_resolves_to_valid_target() {
    let source = r#"
@optimized(123)
@optimized("kernel::foo")
constraint def PlainC {
    param a: Real
    param b: Real
    a == b
}
structure S {
    param x: Real
    param y: Real
    constraint PlainC(a: x, b: y)
}
"#;
    let module = compile_source(source);

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // The valid second @optimized target must be returned.
    let tmpl = template_named(&module, "S");
    assert_eq!(tmpl.constraints.len(), 1);
    let cc: &CompiledConstraint = &tmpl.constraints[0];
    assert_eq!(
        cc.optimized_target,
        Some("kernel::foo".to_string()),
        "expected extractor to skip non-string @optimized(123) and return the valid target; got: {:?}",
        cc.optimized_target
    );

    // The missing-target warning must still fire (on the non-string first @optimized).
    let missing_target_warnings: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| {
            d.message
                .contains("@optimized requires a string literal target")
        })
        .collect();
    assert!(
        !missing_target_warnings.is_empty(),
        "expected a missing-target warning for the non-string first @optimized(123), got none; all diags: {:?}",
        module.diagnostics
    );

    // No duplicate warning: the non-string first annotation is not a valid
    // @optimized entry, so the valid second one is not a duplicate.
    let duplicate_warnings: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| d.message.contains("multiple @optimized annotations"))
        .collect();
    assert!(
        duplicate_warnings.is_empty(),
        "a non-string @optimized before a valid one must not trigger duplicate warning; got: {:?}",
        duplicate_warnings
    );
}

/// Multiple valid `@optimized` annotations on a *structure* must NOT emit the
/// duplicate-@optimized warning — the target string is not consumed on
/// structure contexts (`optimized_target` is only called by entity.rs when
/// lowering constraint defs), so there is nothing being "shadowed".
#[test]
fn multiple_valid_optimized_on_structure_does_not_warn() {
    let module = compile_source(
        r#"
@optimized("kernel::fast")
@optimized("kernel::slow")
structure S {
    param x: Real
}
"#,
    );

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let duplicate_warnings: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| d.message.contains("multiple @optimized annotations"))
        .collect();
    assert!(
        duplicate_warnings.is_empty(),
        "multiple @optimized on structure must not warn about duplicates (target not consumed); got: {:?}",
        duplicate_warnings
    );
}

// ── Step 3 (task 3377): CompiledFunction.optimized_target field existence ────

/// `@optimized("kernel::foo")` on an annotated function must populate
/// `CompiledFunction::optimized_target` with `Some("kernel::foo")`, and an
/// un-annotated function yields `None`.  A `Clone` of the annotated function
/// must carry the same value (exercises the `#[derive(Clone)]` on
/// `CompiledFunction` for the new field).
///
/// RED (step-3): fails to compile because `CompiledFunction` has no
/// `optimized_target` field yet. The compile error is the regression-guard.
#[test]
fn optimized_target_field_on_compiled_function() {
    // (a) annotated function
    let source_annotated = r#"@optimized("kernel::foo") fn annotated(x: Real) -> Real { x }"#;
    let module_a = compile_source(source_annotated);

    let errors = error_diags(&module_a.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let annotated_fn: &CompiledFunction = module_a
        .functions
        .iter()
        .find(|f| f.name == "annotated")
        .expect("function 'annotated' not found in compiled module");
    // This read forces the field to exist — compile error if missing.
    assert_eq!(
        annotated_fn.optimized_target,
        Some("kernel::foo".to_string()),
        "expected optimized_target = Some(\"kernel::foo\") on annotated function"
    );
    // Clone round-trip: the field must survive Clone.
    let cloned = annotated_fn.clone();
    assert_eq!(
        cloned.optimized_target, annotated_fn.optimized_target,
        "optimized_target must survive Clone"
    );

    // (b) plain function
    let source_plain = r#"fn plain(x: Real) -> Real { x }"#;
    let module_b = compile_source(source_plain);

    let errors = error_diags(&module_b.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let plain_fn: &CompiledFunction = module_b
        .functions
        .iter()
        .find(|f| f.name == "plain")
        .expect("function 'plain' not found in compiled module");
    assert!(
        plain_fn.optimized_target.is_none(),
        "un-annotated function should yield optimized_target=None, got: {:?}",
        plain_fn.optimized_target
    );
}

// ── Step 1 (task 3377): function context allow-list ─────────────────────────

/// `@optimized("kernel::foo")` on a function declaration must be accepted by
/// the validator — no "@optimized is not valid on function" warning.
///
/// RED: fails before `"function"` is added to the OPTIMIZED allow-list in
/// `annotations.rs` because `validate_annotations` currently emits exactly that
/// warning for `context == "function"`.
#[test]
fn optimized_annotation_on_function_is_accepted() {
    let source = r#"@optimized("kernel::foo") fn f(x: Real) -> Real { x }"#;
    let module = compile_source(source);

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let bad_optimized_warnings: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| d.message.contains("@optimized is not valid"))
        .collect();
    assert!(
        bad_optimized_warnings.is_empty(),
        "@optimized on function should not warn; got: {:?}",
        bad_optimized_warnings
    );
}

// ── Step 7 (task 3377): duplicate-@optimized warning on function context ─────

/// Multiple `@optimized` annotations stacked on the same `fn` must emit exactly
/// one duplicate warning — the second valid @optimized is shadowed by the first.
///
/// RED: fails before the duplicate-check gate at annotations.rs is widened from
/// `context == "constraint_def"` to `matches!(context, "constraint_def" | "function")`.
#[test]
fn multiple_optimized_annotations_on_function_warns() {
    let module = compile_source(
        r#"@optimized("new_target") @optimized("legacy_target") fn f(x: Real) -> Real { x }"#,
    );

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let relevant: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| d.message.contains("multiple @optimized annotations"))
        .collect();
    assert_eq!(
        relevant.len(),
        1,
        "expected exactly one duplicate-@optimized warning on fn, got: {:?}",
        relevant
    );
}

// ── Step 5 (task 3377): missing-target warning on function context ───────────

/// `@optimized` (no target) on a `fn` must emit the same missing-target warning
/// that fires on a `constraint_def` — the target is now consumed in both contexts.
///
/// RED: fails before the missing-target gate at annotations.rs is widened from
/// `context == "constraint_def"` to `matches!(context, "constraint_def" | "function")`.
#[test]
fn optimized_without_target_on_function_warns() {
    let module = compile_source(r#"@optimized() fn f(x: Real) -> Real { x }"#);

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let relevant: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| {
            d.message
                .contains("@optimized requires a string literal target")
        })
        .collect();
    assert!(
        !relevant.is_empty(),
        "expected a missing-target warning on @optimized() on fn, got none; all diags: {:?}",
        module.diagnostics
    );
}

/// A valid single `@optimized("target")` must NOT trip any of the new
/// malformed-annotation warnings.
#[test]
fn well_formed_optimized_has_no_malformed_warnings() {
    let module = compile_source(
        r#"
@optimized("kernel::foo")
constraint def Plain {
    param a: Real
    param b: Real
    a == b
}
"#,
    );

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    for d in warning_diags(&module.diagnostics) {
        assert!(
            !d.message
                .contains("@optimized requires a string literal target"),
            "well-formed @optimized should not trip missing-target warning: {:?}",
            d
        );
        assert!(
            !d.message.contains("multiple @optimized annotations"),
            "well-formed @optimized should not trip duplicate warning: {:?}",
            d
        );
    }
}
