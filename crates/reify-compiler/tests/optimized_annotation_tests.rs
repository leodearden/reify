//! Compiler-level tests for the `@optimized` annotation on constraint defs
//! (Task 273 — @optimized: plumbing).
//!
//! Exercises:
//!   - `@optimized("target")` on a `constraint def` is accepted by the validator
//!     (no "@optimized is not valid" warning).
//!   - Instantiating such a def in a structure propagates the target onto the
//!     resulting `CompiledConstraint::optimized_target`.
//!   - An un-annotated constraint def yields `optimized_target = None`.

use reify_compiler::{CompiledConstraint, CompiledModule, TopologyTemplate};
use reify_types::{Diagnostic, ModulePath, Severity};

// ── Helpers ─────────────────────────────────────────────────────────────────

fn compile_module(source: &str) -> CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("optimized_ann_test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );
    reify_compiler::compile(&parsed)
}

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
    let module = compile_module(source);

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
    let module = compile_module(source);

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
    let module = compile_module(source);

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
    let module = compile_module(
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
    let module = compile_module(
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

/// `@optimized` on a function (an unsupported context) should still emit a
/// warning — the broadening should have added `constraint_def`, not silenced
/// the entire context check.
#[test]
fn optimized_on_unsupported_context_still_warns() {
    let module = compile_module(r#"@optimized("x") fn f(x: Real) -> Real { x }"#);

    let errors = error_diags(&module.diagnostics);
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    let warnings: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| d.message.contains("@optimized is not valid"))
        .collect();
    assert!(
        !warnings.is_empty(),
        "expected a warning about @optimized on function, got none; all diags: {:?}",
        module.diagnostics
    );
}

// ── Missing-target warning must not fire on non-consuming contexts ───────────

/// `@optimized` with no string-literal arg on a *structure* must NOT emit the
/// missing-target warning — the target is only consumed in constraint_def context.
/// Telling a user to add a string that nothing reads is actively harmful.
#[test]
fn optimized_missing_target_on_structure_does_not_warn() {
    let module = compile_module(
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
    let module = compile_module(
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
    let module = compile_module(
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
        .filter(|d| d.message.contains("@optimized requires a string literal target"))
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
    let module = compile_module(
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
        .filter(|d| d.message.contains("@optimized requires a string literal target"))
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
    let module = compile_module(
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
/// The user still sees:
///   - a missing-target warning on the first (malformed) @optimized
///   - a duplicate-@optimized warning on the second
/// so the annotation is not silently condoned, but the valid target is plumbed through.
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
    let module = compile_module(source);

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
        .filter(|d| d.message.contains("@optimized requires a string literal target"))
        .collect();
    assert!(
        !missing_target_warnings.is_empty(),
        "expected a missing-target warning for the malformed first @optimized, got none; all diags: {:?}",
        module.diagnostics
    );

    // The duplicate-@optimized warning must also fire (on the second @optimized).
    let duplicate_warnings: Vec<_> = warning_diags(&module.diagnostics)
        .into_iter()
        .filter(|d| d.message.contains("multiple @optimized annotations"))
        .collect();
    assert!(
        !duplicate_warnings.is_empty(),
        "expected a duplicate-@optimized warning for the second annotation, got none; all diags: {:?}",
        module.diagnostics
    );
}

/// A valid single `@optimized("target")` must NOT trip any of the new
/// malformed-annotation warnings.
#[test]
fn well_formed_optimized_has_no_malformed_warnings() {
    let module = compile_module(
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
            !d.message.contains("@optimized requires a string literal target"),
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
