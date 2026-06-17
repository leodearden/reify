//! M8–M11 diagnostic coverage checkpoint.
//!
//! **File**: `diagnostic_coverage_checkpoint.rs`  (covers milestones M8 through M11;
//! originally named `m11_diagnostic_coverage.rs`, renamed to clarify cross-milestone scope)
//!
//! This file is the authoritative integration-coverage checkpoint ensuring that every
//! major compiler error and warning category introduced across milestones M8–M11 has at
//! least one dedicated test case in this binary. It is intentionally kept self-contained
//! so that retiring or refactoring individual per-milestone test files (m9_error_cases.rs,
//! annotation_compile_tests.rs, pragma_compile_tests.rs, guard_compilation.rs, …) does
//! not silently remove cross-milestone diagnostic coverage.
//!
//! Categories covered (task 294 requirements):
//!   M8: circular type alias, dimension mismatches (binary op + range)
//!   M9: trait conformance violations, member merge conflicts, constraint def errors,
//!       no termination condition, meta key access errors, duplicate declarations
//!   M10: guard reference safety violations (unguarded / differently-guarded warnings,
//!        sub/minimize in guarded block errors), generic type argument errors
//!   M11: annotation context errors, unknown annotation warnings, multiple @optimized,
//!        unknown pragma warnings
//!
//! Pattern (mirrors m9_error_cases.rs verbatim):
//!   1. Build a small Reify source string with the error-producing construct.
//!   2. Call `compile_source(source)` (or `compile_source_with_stdlib` when needed).
//!   3. Filter diagnostics via `errors_only` or `warnings_only`.
//!   4. Assert the filtered list is non-empty (with `{:?}` dump on failure).
//!   5. Assert a specific substring is present in at least one matching diagnostic.
//!   6. Assert the first matching diagnostic has at least one label with a non-empty span.
//!
//! Source references:
//!   type_resolution.rs  — circular alias, cannot resolve dimension
//!   conformance.rs      — trait conformance errors
//!   entity.rs           — constraint, port, duplicate, guard safety, type param errors
//!   termination.rs      — recursive sub termination errors
//!   expr.rs             — dimension mismatch, meta key access
//!   guards.rs           — sub/minimize in guarded block errors
//!   annotations.rs      — annotation context, unknown annotation, unknown pragma
//!   lib.rs              — duplicate entity/unit/type-alias declarations
//!
//! Scope boundary — COMPILER-time diagnostics only. Every case here drives a
//! diagnostic through `compile_source(_with_stdlib)` and asserts on
//! `errors_only` / `warnings_only`, so this checkpoint can only cover codes
//! emitted during compilation. EVAL-time codes — those produced by the runtime
//! after a successful compile — are out of scope and covered by their own
//! eval/e2e suites. In particular the v0.3 flexure codes (task 3871:
//! `FlexureYielding`, `FlexurePrbOutOfRange`, `FlexureFatigueCheckMissing`,
//! `FlexureGeometryInvalid`) are emitted by the reify-expr PRB-ctor
//! `flexure_diagnose` hook at eval time, never by the compiler, so their
//! emission coverage lives in `crates/reify-eval/tests/flexure_e2e.rs` and
//! their reify-core round-trip/severity/serde coverage in
//! `crates/reify-core/src/diagnostics.rs` — not here.

use reify_test_support::{compile_source, compile_source_with_stdlib, errors_only, warnings_only};
use reify_core::DiagnosticCode;

// ── Smoke test ────────────────────────────────────────────────────────────────

/// Smoke test: a trivially valid structure compiles with zero errors. Confirms
/// that the test binary target is wired and the helpers are usable.
#[test]
fn diagnostic_coverage_file_compiles() {
    let source = r#"
structure def S {
    param x : Real = 1.0
}
"#;
    let module = compile_source(source);
    assert!(
        errors_only(&module).is_empty(),
        "expected no errors for trivially valid structure, got: {:?}",
        module.diagnostics
    );
}

// ── M8: Type-alias & dimensional diagnostic tests ─────────────────────────────

/// Two mutually-referencing type aliases should produce "circular type alias" errors.
///
/// Exercises type_resolution.rs line 1080.
#[test]
fn circular_type_alias_a_b_a() {
    let source = r#"
type A = B
type B = A
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for circular type alias A↔B, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors
        .iter()
        .any(|d| d.message.contains("circular type alias"));
    assert!(
        has_msg,
        "expected 'circular type alias' error, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("circular type alias"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// A self-referential type alias (`type X = X`) should produce "circular type alias".
///
/// Exercises type_resolution.rs line 1080.
#[test]
fn self_referential_type_alias() {
    let source = r#"
type X = X
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for self-referential type alias, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors
        .iter()
        .any(|d| d.message.contains("circular type alias"));
    assert!(
        has_msg,
        "expected 'circular type alias' error, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("circular type alias"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// A type alias using a dimensional operator (`/`) where the left-hand dimension
/// name is unknown should produce "cannot resolve" error.
///
/// Exercises type_resolution.rs line 629.
#[test]
fn type_alias_with_unknown_dimension_component() {
    let source = r#"
type Velocity = NotARealDim / Time
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for unknown dimension in alias expression, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors
        .iter()
        .any(|d| d.message.contains("cannot resolve") && d.message.contains("NotARealDim"));
    assert!(
        has_msg,
        "expected 'cannot resolve' error mentioning 'NotARealDim', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("cannot resolve"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Adding quantities with incompatible dimensions (Length + Mass) in a param default
/// should produce "dimension mismatch in" error.
///
/// Exercises expr.rs line 234–235.
#[test]
fn dimension_mismatch_in_binary_op_length_plus_mass() {
    let source = r#"
structure def S {
    param p : Length = 1mm + 1kg
}
"#;
    let module = compile_source_with_stdlib(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for Length+Mass dimension mismatch, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors
        .iter()
        .any(|d| d.message.contains("dimension mismatch in"));
    assert!(
        has_msg,
        "expected 'dimension mismatch in' error, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("dimension mismatch in"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// A range literal whose bounds have incompatible dimensions (Length..Mass) should
/// produce "dimension mismatch in range" error.
///
/// Exercises expr.rs line 340–341.
#[test]
fn dimension_mismatch_in_range_bounds() {
    let source = r#"
structure def S {
    let r = 1mm..1kg
}
"#;
    let module = compile_source_with_stdlib(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for Length..Mass range mismatch, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors
        .iter()
        .any(|d| d.message.contains("dimension mismatch in range"));
    assert!(
        has_msg,
        "expected 'dimension mismatch in range' error, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("dimension mismatch in range"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

// ── M9: Trait conformance diagnostic tests ────────────────────────────────────

/// A structure that declares a trait but omits the required param member
/// should produce "missing required member" diagnostic.
///
/// Exercises conformance.rs line 214.
#[test]
fn trait_missing_required_member() {
    let source = r#"
trait Shaped {
    param width : Length
}

structure def S : Shaped {
    param height : Length = 5mm
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for missing required trait member, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors
        .iter()
        .any(|d| d.message.contains("missing required member") && d.message.contains("width"));
    assert!(
        has_msg,
        "expected 'missing required member' mentioning 'width', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("missing required member"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// A structure provides the required param but with the wrong type
/// should produce "type mismatch for trait member" diagnostic.
///
/// Exercises conformance.rs line 173.
#[test]
fn trait_member_type_mismatch() {
    let source = r#"
trait Countable {
    param count : Int
}

structure def S : Countable {
    param count : Length = 5mm
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for type mismatch in trait member, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors.iter().any(|d| {
        d.message.contains("type mismatch for trait member") && d.message.contains("count")
    });
    assert!(
        has_msg,
        "expected 'type mismatch for trait member' mentioning 'count', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("type mismatch for trait member"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// A structure declaring a nonexistent trait bound should produce "unresolved trait".
///
/// Exercises conformance.rs line 374.
#[test]
fn unresolved_trait_name() {
    let source = r#"
structure def S : NonExistentTrait {
    param x : Length = 1mm
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for unresolved trait, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors.iter().any(|d| {
        d.code == Some(DiagnosticCode::UnresolvedTrait)
            // Keep the 'NonExistentTrait' name-token check: it carries semantic content
            // beyond wording.
            && d.message.contains("NonExistentTrait")
    });
    assert!(
        has_msg,
        "expected DiagnosticCode::UnresolvedTrait mentioning 'NonExistentTrait', got: {:?}",
        errors
            .iter()
            .map(|d| (d.code, &d.message))
            .collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::UnresolvedTrait))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Two traits requiring the same member with incompatible types should produce
/// "conflicting trait requirements" diagnostic.
///
/// Exercises conformance.rs line 408.
#[test]
fn conflicting_trait_requirements() {
    let source = r#"
trait HasX {
    param x : Length
}

trait HasXInt {
    param x : Int
}

structure def S : HasX + HasXInt {
    param x : Length = 1mm
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for conflicting trait requirements, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors.iter().any(|d| {
        d.code == Some(DiagnosticCode::ConflictingTraitRequirements)
            // Keep the 'x' name-token check: more than one conflict could share
            // this code; the member name carries semantic content beyond wording.
            && d.message.contains("x")
    });
    assert!(
        has_msg,
        "expected DiagnosticCode::ConflictingTraitRequirements mentioning 'x', got: {:?}",
        errors
            .iter()
            .map(|d| (d.code, &d.message))
            .collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ConflictingTraitRequirements))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Two traits providing `let` bindings with the same name but different expressions
/// should produce "conflicting trait let bindings" diagnostic.
///
/// Exercises conformance.rs line 445.
#[test]
fn conflicting_trait_let_bindings() {
    let source = r#"
trait TraitAlpha {
    let area : Real = width + 1.0
}

trait TraitBeta {
    let area : Real = width * 2.0
}

structure def S : TraitAlpha + TraitBeta {
    param width : Real = 5.0
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for conflicting trait let bindings, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors.iter().any(|d| {
        d.code == Some(DiagnosticCode::ConflictingTraitLetBindings)
            // Keep the 'area' name-token check: it carries semantic content beyond wording.
            && d.message.contains("area")
    });
    assert!(
        has_msg,
        "expected DiagnosticCode::ConflictingTraitLetBindings mentioning 'area', got: {:?}",
        errors
            .iter()
            .map(|d| (d.code, &d.message))
            .collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.code == Some(DiagnosticCode::ConflictingTraitLetBindings))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Two traits providing param defaults with the same name but different types
/// should produce a "conflicting trait" diagnostic.
///
/// Exercises conformance.rs line 478.
#[test]
fn conflicting_trait_defaults() {
    let source = r#"
trait ProvidesLength {
    param size : Length = 10mm
}

trait ProvidesMass {
    param size : Mass = 1kg
}

structure def S : ProvidesLength + ProvidesMass {
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for conflicting trait defaults, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors.iter().any(|d| {
        d.code == Some(DiagnosticCode::ConflictingTraitDefaults)
            // Keep the 'size' name-token check: it carries semantic content beyond wording.
            && d.message.contains("size")
    });
    assert!(
        has_msg,
        "expected DiagnosticCode::ConflictingTraitDefaults mentioning 'size', got: {:?}",
        errors
            .iter()
            .map(|d| (d.code, &d.message))
            .collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| {
            d.code == Some(DiagnosticCode::ConflictingTraitDefaults) && d.message.contains("size")
        })
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

// ── M9: Constraint-def & termination diagnostic tests ────────────────────────

/// Using an unknown constraint definition name should produce
/// "unknown constraint definition" diagnostic.
///
/// Exercises entity.rs line 1077.
#[test]
fn unknown_constraint_definition() {
    let source = r#"
structure def S {
    param x : Length = 5mm
    constraint NoSuchConstraint(x: x)
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for unknown constraint definition, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors.iter().any(|d| {
        d.message.contains("unknown constraint definition")
            && d.message.contains("NoSuchConstraint")
    });
    assert!(
        has_msg,
        "expected 'unknown constraint definition' mentioning 'NoSuchConstraint', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("unknown constraint definition"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Passing a bogus argument name to a known constraint definition should produce
/// "unknown argument" diagnostic.
///
/// Exercises entity.rs line 1103.
#[test]
fn unknown_constraint_argument() {
    let source = r#"
constraint def MinWall {
    param wall : Length
    wall > 0mm
}

structure def S {
    param t : Length = 5mm
    constraint MinWall(wall: t, bogus: t)
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for unknown constraint argument, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors
        .iter()
        .any(|d| d.message.contains("unknown argument") && d.message.contains("bogus"));
    assert!(
        has_msg,
        "expected 'unknown argument' mentioning 'bogus', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("unknown argument"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Omitting a required argument in a constraint instantiation should produce
/// "missing argument" diagnostic.
///
/// Exercises entity.rs line 1120.
#[test]
fn missing_constraint_argument() {
    let source = r#"
constraint def TwoParams {
    param a : Length
    param b : Length
    a > b
}

structure def S {
    param x : Length = 5mm
    constraint TwoParams(a: x)
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for missing constraint argument, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors
        .iter()
        .any(|d| d.message.contains("missing argument") && d.message.contains("b"));
    assert!(
        has_msg,
        "expected 'missing argument' mentioning 'b', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("missing argument"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// A recursive sub without any where-clause guard should produce
/// "no termination condition" diagnostic.
///
/// Exercises termination.rs line 39.
#[test]
fn recursive_sub_no_termination_condition() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n - 1)
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for recursive sub without guard, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors
        .iter()
        .any(|d| d.message.contains("no termination condition"));
    assert!(
        has_msg,
        "expected 'no termination condition' error, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("no termination condition"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// A recursive sub whose guarded Int param is passed unchanged (not decremented)
/// should produce "does not decrement parameter" diagnostic.
///
/// Exercises termination.rs line 98.
#[test]
fn recursive_sub_param_not_decremented() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n) where n > 0
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for param not decremented, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors
        .iter()
        .any(|d| d.message.contains("does not decrement parameter") && d.message.contains("n"));
    assert!(
        has_msg,
        "expected 'does not decrement parameter' mentioning 'n', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("does not decrement parameter"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// A recursive sub passing `undef` as the guarded Int param argument
/// should produce "undef is not allowed" diagnostic.
///
/// Exercises termination.rs line 78.
#[test]
fn recursive_sub_undef_not_allowed() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: undef) where n > 0
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for undef in recursive sub args, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors
        .iter()
        .any(|d| d.message.contains("undef is not allowed"));
    assert!(
        has_msg,
        "expected 'undef is not allowed' error, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("undef is not allowed"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

// ── M9: Meta key + duplicate-name diagnostic tests ───────────────────────────

/// Accessing `meta.key` when the entity has no meta block at all
/// should produce "no meta block" diagnostic.
///
/// Exercises expr.rs line 845.
#[test]
fn meta_access_no_meta_block() {
    let source = r#"
structure def S {
    param width : Length = 10mm
    let label : String = meta.description
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for meta access without meta block, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors.iter().any(|d| {
        d.message.contains("entity has no meta block") || d.message.contains("no meta block")
    });
    assert!(
        has_msg,
        "expected 'no meta block' error, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| {
            d.message.contains("entity has no meta block") || d.message.contains("no meta block")
        })
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Accessing `meta.key` when the meta block exists but the key is absent
/// should produce "meta block has no key" diagnostic.
///
/// Exercises expr.rs line 854.
#[test]
fn meta_access_unknown_key() {
    let source = r#"
structure def S {
    meta {
        description = "A structure"
    }
    param width : Length = 10mm
    let label : String = meta.part_number
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for nonexistent meta key, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors
        .iter()
        .any(|d| d.message.contains("meta block has no key") && d.message.contains("part_number"));
    assert!(
        has_msg,
        "expected 'meta block has no key' mentioning 'part_number', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("meta block has no key"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Two `structure def` declarations with the same name should produce
/// "duplicate entity definition" diagnostic.
///
/// Exercises lib.rs line 159.
#[test]
fn duplicate_entity_definition_same_name() {
    let source = r#"
structure def Widget {
    param x : Length = 1mm
}

structure def Widget {
    param y : Length = 2mm
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for duplicate entity definition, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors
        .iter()
        .any(|d| d.message.contains("duplicate entity definition") && d.message.contains("Widget"));
    assert!(
        has_msg,
        "expected 'duplicate entity definition' mentioning 'Widget', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("duplicate entity definition") && d.message.contains("Widget"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Two module-local unit declarations with the same name should produce
/// "duplicate unit declaration" diagnostic.
///
/// Exercises lib.rs line 297.
#[test]
fn duplicate_unit_declaration_local() {
    let source = r#"
unit myunit : Length
unit myunit : Length
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for duplicate local unit, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors
        .iter()
        .any(|d| d.message.contains("duplicate unit declaration") && d.message.contains("myunit"));
    assert!(
        has_msg,
        "expected 'duplicate unit declaration' mentioning 'myunit', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("duplicate unit declaration") && d.message.contains("myunit"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// A module-local unit declaration that shadows a stdlib prelude unit should
/// produce "already defined in stdlib prelude" diagnostic.
///
/// Exercises lib.rs line 282.
#[test]
fn duplicate_unit_shadows_stdlib() {
    let source = r#"
unit mm : Length = 0.001
"#;
    let module = compile_source_with_stdlib(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for stdlib unit shadowing, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors.iter().any(|d| {
        d.message.contains("already defined in stdlib prelude") && d.message.contains("mm")
    });
    assert!(
        has_msg,
        "expected 'already defined in stdlib prelude' mentioning 'mm', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| {
            d.message.contains("already defined in stdlib prelude") && d.message.contains("mm")
        })
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Two type alias declarations with the same name should produce
/// "duplicate type alias declaration" diagnostic.
///
/// Exercises lib.rs line 337.
#[test]
fn duplicate_type_alias_name() {
    let source = r#"
type Foo = Int
type Foo = Real
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for duplicate type alias, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors.iter().any(|d| {
        d.message.contains("duplicate type alias declaration") && d.message.contains("Foo")
    });
    assert!(
        has_msg,
        "expected 'duplicate type alias declaration' mentioning 'Foo', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| {
            d.message.contains("duplicate type alias declaration") && d.message.contains("Foo")
        })
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Two ports with the same name in the same structure should produce
/// "duplicate port name" diagnostic.
///
/// Exercises entity.rs line 349.
#[test]
fn duplicate_port_name() {
    let source = r#"
trait MechPort {
    param diameter : Length
}

structure def S {
    port mount : MechPort {
        param diameter : Length = 5mm
    }
    port mount : MechPort {
        param diameter : Length = 10mm
    }
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for duplicate port name, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors
        .iter()
        .any(|d| d.message.contains("duplicate port name") && d.message.contains("mount"));
    assert!(
        has_msg,
        "expected 'duplicate port name' mentioning 'mount', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("duplicate port name") && d.message.contains("mount"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

// ── M10: Guard reference-safety diagnostic tests (warnings + errors) ──────────

/// An unguarded `let` referencing a guarded `param` should produce an
/// "unguarded reference to guarded cell" warning.
///
/// Exercises entity.rs line 1341/1358.
#[test]
fn unguarded_reference_to_guarded_cell_warning() {
    let source = r#"
structure S {
    param active : Bool = true
    param x : Length = 5mm where active
    let y = x
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);

    assert!(
        !warnings.is_empty(),
        "expected at least one warning for unguarded reference to guarded cell, got: {:?}",
        module.diagnostics
    );

    let has_msg = warnings.iter().any(|d| {
        d.message.contains("unguarded reference to guarded cell") && d.message.contains("x")
    });
    assert!(
        has_msg,
        "expected 'unguarded reference to guarded cell' warning mentioning 'x', got: {:?}",
        warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = warnings
        .iter()
        .find(|d| d.message.contains("unguarded reference to guarded cell"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// A member in an `else` block referencing a cell guarded by a different guard
/// should produce a "differently-guarded cell" warning.
///
/// Exercises entity.rs line 1378/1398/1417/1435.
#[test]
fn cross_guard_differently_guarded_reference() {
    let source = r#"
structure S {
    param a : Bool = true
    param b : Bool = true
    where a {
        param x : Length = 5mm
    }
    where b {
    } else {
        let y = x
    }
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);

    assert!(
        !warnings.is_empty(),
        "expected at least one warning for cross-guard reference, got: {:?}",
        module.diagnostics
    );

    let has_msg = warnings
        .iter()
        .any(|d| d.message.contains("differently-guarded cell") && d.message.contains("x"));
    assert!(
        has_msg,
        "expected 'differently-guarded cell' warning mentioning 'x', got: {:?}",
        warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = warnings
        .iter()
        .find(|d| d.message.contains("differently-guarded cell"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// A `sub` declaration inside a `where {}` block should emit a
/// "sub declarations in guarded blocks are not yet supported" error.
///
/// Exercises guards.rs line 420.
#[test]
fn sub_in_guarded_block_error() {
    let source = r#"
structure S {
    param active : Bool = true
    where active {
        sub child = S()
    }
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let has_msg = errors.iter().any(|d| {
        d.message.contains("not yet supported") && d.message.to_lowercase().contains("sub")
    });
    assert!(
        has_msg,
        "expected 'sub declarations in guarded blocks are not yet supported' error, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| {
            d.message.contains("not yet supported") && d.message.to_lowercase().contains("sub")
        })
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// A `minimize` declaration inside a `where {}` block should emit a
/// "minimize declarations in guarded blocks are not yet supported" error.
///
/// Exercises guards.rs line 428.
#[test]
fn minimize_in_guarded_block_error() {
    let source = r#"
structure S {
    param active : Bool = true
    param x : Length = 5mm
    where active {
        minimize x
    }
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    let has_msg = errors.iter().any(|d| {
        d.message.contains("not yet supported") && d.message.to_lowercase().contains("minimize")
    });
    assert!(
        has_msg,
        "expected 'minimize declarations in guarded blocks are not yet supported' error, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| {
            d.message.contains("not yet supported") && d.message.to_lowercase().contains("minimize")
        })
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

// ── M11: Annotation-context diagnostic tests (warnings) ──────────────────────

/// `@test` annotation on a field declaration (invalid context) should produce
/// a warning containing "@test is not valid on" and "field".
///
/// Exercises annotations.rs line 73–77.
#[test]
fn annotation_test_on_field_is_invalid_context() {
    let source = r#"
@test field def f : Point3 -> Real { source = analytical { |p| 0.0 } }
"#;
    let module = compile_source(source);
    // No compile errors expected — only a warning
    let warnings = warnings_only(&module);

    assert!(
        !warnings.is_empty(),
        "expected at least one warning for @test on field, got: {:?}",
        module.diagnostics
    );

    let has_msg = warnings
        .iter()
        .any(|d| d.message.contains("@test is not valid on") && d.message.contains("field"));
    assert!(
        has_msg,
        "expected '@test is not valid on' warning mentioning 'field', got: {:?}",
        warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = warnings
        .iter()
        .find(|d| d.message.contains("@test is not valid on"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// An unknown annotation `@foobar` on a structure should produce a warning
/// containing "unknown annotation" and "foobar".
///
/// Exercises annotations.rs line 119.
#[test]
fn unknown_annotation_foobar_warning() {
    let source = r#"
@foobar structure def S {
    param x : Real = 1.0
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);

    assert!(
        !warnings.is_empty(),
        "expected at least one warning for unknown annotation @foobar, got: {:?}",
        module.diagnostics
    );

    let has_msg = warnings
        .iter()
        .any(|d| d.message.contains("unknown annotation") && d.message.contains("foobar"));
    assert!(
        has_msg,
        "expected 'unknown annotation' warning mentioning 'foobar', got: {:?}",
        warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = warnings
        .iter()
        .find(|d| d.message.contains("unknown annotation"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// `@optimized` annotation on a `fn` declaration (invalid context) should produce
/// a warning containing "@optimized is not valid on".
///
/// `@optimized` is valid on structure/occurrence/constraint_def/function.
/// `trait` is NOT in the allow-list, so @optimized on a trait emits the warning.
///
/// Migrated from `fn` to `trait` in task 3377 because `function` is now an
/// allow-listed context for `@optimized` (CompiledFunction::optimized_target).
/// Exercises annotations.rs OPTIMIZED arm context check.
#[test]
fn optimized_on_function_warns() {
    let source = r#"
@optimized("x")
trait T {
    param x: Real
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);

    assert!(
        !warnings.is_empty(),
        "expected at least one warning for @optimized on trait (unsupported context), got: {:?}",
        module.diagnostics
    );

    let has_msg = warnings
        .iter()
        .any(|d| d.message.contains("@optimized is not valid on"));
    assert!(
        has_msg,
        "expected '@optimized is not valid on' warning, got: {:?}",
        warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = warnings
        .iter()
        .find(|d| d.message.contains("@optimized is not valid on"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Two `@optimized` annotations on the same constraint def should produce
/// "multiple @optimized annotations" warning on the second occurrence.
///
/// The duplicate check is scoped to `constraint_def` context because
/// `optimized_target` is only consumed there; on structure/occurrence
/// contexts the target string has no downstream consumer so warning about
/// shadowing would be misleading (see annotations.rs duplicate-check block).
///
/// Exercises annotations.rs duplicate-annotation check.
#[test]
fn multiple_optimized_annotations_warning() {
    let source = r#"
@optimized("target_a") @optimized("target_b") constraint def D {
    param x : Length
    x > 0mm
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);

    assert!(
        !warnings.is_empty(),
        "expected at least one warning for multiple @optimized annotations, got: {:?}",
        module.diagnostics
    );

    let has_msg = warnings
        .iter()
        .any(|d| d.message.contains("multiple @optimized annotations"));
    assert!(
        has_msg,
        "expected 'multiple @optimized annotations' warning, got: {:?}",
        warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = warnings
        .iter()
        .find(|d| d.message.contains("multiple @optimized annotations"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

// ── M11: Pragma-warning diagnostic tests ─────────────────────────────────────

/// Unknown module-level pragma `#optimize` should emit an "unknown pragma" warning.
///
/// Exercises annotations.rs line 163 (validate_pragmas) and lib.rs module pragma pass.
#[test]
fn unknown_module_pragma_warning() {
    let module = compile_source("#optimize\nstructure S { param x : Real }");
    let warnings = warnings_only(&module);

    assert!(
        !warnings.is_empty(),
        "expected at least one warning for unknown module pragma #optimize, got: {:?}",
        module.diagnostics
    );

    let has_msg = warnings
        .iter()
        .any(|d| d.message.contains("unknown pragma") && d.message.contains("optimize"));
    assert!(
        has_msg,
        "expected 'unknown pragma' warning mentioning 'optimize', got: {:?}",
        warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = warnings
        .iter()
        .find(|d| d.message.contains("unknown pragma"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Unknown pragma `#turbo` inside a structure body should emit an "unknown pragma" warning.
///
/// Exercises annotations.rs line 163.
#[test]
fn unknown_structure_pragma_warning() {
    let module = compile_source(r#"structure S { #turbo param x : Real }"#);
    let warnings = warnings_only(&module);

    assert!(
        !warnings.is_empty(),
        "expected at least one warning for unknown structure pragma #turbo, got: {:?}",
        module.diagnostics
    );

    let has_msg = warnings
        .iter()
        .any(|d| d.message.contains("unknown pragma") && d.message.contains("turbo"));
    assert!(
        has_msg,
        "expected 'unknown pragma' warning mentioning 'turbo', got: {:?}",
        warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = warnings
        .iter()
        .find(|d| d.message.contains("unknown pragma"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Unknown pragma `#fast` inside a trait body should emit an "unknown pragma" warning.
///
/// Exercises annotations.rs line 163.
#[test]
fn unknown_trait_pragma_warning() {
    let module = compile_source(r#"trait T { #fast param x : Real }"#);
    let warnings = warnings_only(&module);

    assert!(
        !warnings.is_empty(),
        "expected at least one warning for unknown trait pragma #fast, got: {:?}",
        module.diagnostics
    );

    let has_msg = warnings
        .iter()
        .any(|d| d.message.contains("unknown pragma") && d.message.contains("fast"));
    assert!(
        has_msg,
        "expected 'unknown pragma' warning mentioning 'fast', got: {:?}",
        warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = warnings
        .iter()
        .find(|d| d.message.contains("unknown pragma"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Unknown pragma `#accel` inside a purpose body should emit an "unknown pragma" warning.
///
/// Exercises annotations.rs line 163.
#[test]
fn unknown_purpose_pragma_warning() {
    let source = r#"
structure S { param x : Real = 0.0 }
purpose p(s : Structure) {
    #accel
    constraint 1 > 0
}
"#;
    let module = compile_source(source);
    let warnings = warnings_only(&module);

    assert!(
        !warnings.is_empty(),
        "expected at least one warning for unknown purpose pragma #accel, got: {:?}",
        module.diagnostics
    );

    let has_msg = warnings
        .iter()
        .any(|d| d.message.contains("unknown pragma") && d.message.contains("accel"));
    assert!(
        has_msg,
        "expected 'unknown pragma' warning mentioning 'accel', got: {:?}",
        warnings.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = warnings
        .iter()
        .find(|d| d.message.contains("unknown pragma"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

// ── M10/M11: Generic type-argument diagnostic tests ───────────────────────────

/// Providing more type arguments than a generic structure has type parameters
/// should produce "too many type arguments" diagnostic.
///
/// Exercises entity.rs line 1581.
#[test]
fn too_many_type_arguments() {
    let source = r#"
trait Rigid { param mass : Mass }
structure def Bolt : Rigid { param mass : Mass = 1kg }
structure def Box<T: Rigid> { param width : Length = 10mm }
structure def Assembly { sub part = Box<Bolt, Bolt>() }
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for too many type arguments, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors
        .iter()
        .any(|d| d.message.contains("too many type arguments") && d.message.contains("Box"));
    assert!(
        has_msg,
        "expected 'too many type arguments' mentioning 'Box', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("too many type arguments") && d.message.contains("Box"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Omitting a required type argument (type parameter with no default)
/// should produce "missing type argument" diagnostic.
///
/// Exercises entity.rs line 1605.
#[test]
fn missing_type_argument_no_default() {
    let source = r#"
trait Rigid { param mass : Mass }
structure def Box<T: Rigid> { param width : Length = 10mm }
structure def Assembly { sub part = Box() }
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for missing type argument, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors
        .iter()
        .any(|d| d.message.contains("missing type argument") && d.message.contains("T"));
    assert!(
        has_msg,
        "expected 'missing type argument' mentioning 'T', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("missing type argument") && d.message.contains("T"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Passing a type argument that does not satisfy the trait bound on the type parameter
/// should produce "does not satisfy bound" diagnostic.
///
/// Exercises entity.rs line 1643.
#[test]
fn type_argument_does_not_satisfy_bound() {
    let source = r#"
trait Rigid { param mass : Mass }
structure def Widget { param x : Length = 5mm }
structure def Box<T: Rigid> { param width : Length = 10mm }
structure def Assembly { sub part = Box<Widget>() }
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for type arg not satisfying bound, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors
        .iter()
        .any(|d| d.message.contains("does not satisfy bound") && d.message.contains("Widget"));
    assert!(
        has_msg,
        "expected 'does not satisfy bound' mentioning 'Widget', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("does not satisfy bound") && d.message.contains("Widget"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

// ── Missing-from-prior-checkpoints coverage ───────────────────────────────────

/// A recursive sub with a where-clause guard that references only a non-Int/non-Bool
/// parameter should produce "guard does not reference any Int or Bool parameter".
///
/// Uses a literal guard expression (`1 > 0`) so no parameter is referenced at all.
/// Exercises termination.rs line 63–67.
#[test]
fn recursive_sub_guard_no_int_or_bool_param() {
    let source = r#"
structure S {
    param n : Int = 5
    sub child = S(n: n - 1) where 1 > 0
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for guard not referencing Int/Bool param, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors.iter().any(|d| {
        d.message
            .contains("guard does not reference any Int or Bool")
    });
    assert!(
        has_msg,
        "expected 'guard does not reference any Int or Bool' error, got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| {
            d.message
                .contains("guard does not reference any Int or Bool")
        })
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// A structure declaring a trait that requires a sub-component but not providing
/// that sub should produce "missing required sub-component" diagnostic.
///
/// Exercises conformance.rs line 237–238.
#[test]
fn missing_required_sub_component() {
    let source = r#"
trait HasEngine {
    sub engine = Engine()
}

structure def Engine {
    param hp : Int = 100
}

structure def Vehicle : HasEngine {
    param speed : Int = 60
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for missing required sub-component, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors.iter().any(|d| {
        d.message.contains("missing required sub-component") && d.message.contains("engine")
    });
    assert!(
        has_msg,
        "expected 'missing required sub-component' mentioning 'engine', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("missing required sub-component"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// A `structure def` and an `occurrence def` with the same name should produce
/// "duplicate entity definition" — verifying that cross-kind collisions (not just
/// same-kind) are caught by the lib.rs pass-1 deduplication.
///
/// Exercises lib.rs line 180.
#[test]
fn duplicate_entity_structure_and_occurrence_collision() {
    let source = r#"
structure def Gadget {
    param x : Length = 1mm
}

occurrence def Gadget {
    param x : Length = 2mm
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for structure/occurrence name collision, got: {:?}",
        module.diagnostics
    );

    let has_msg = errors
        .iter()
        .any(|d| d.message.contains("duplicate entity definition") && d.message.contains("Gadget"));
    assert!(
        has_msg,
        "expected 'duplicate entity definition' mentioning 'Gadget', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let first = errors
        .iter()
        .find(|d| d.message.contains("duplicate entity definition") && d.message.contains("Gadget"))
        .unwrap();
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Circular trait refinement (A refines B, B refines A) should not panic.
///
/// The compiler uses a visited-set to break the cycle, so compilation completes
/// without infinite recursion. This test documents the "no crash" invariant
/// and anchors the conformance.rs cycle-detection path in this checkpoint file
/// so retiring m9_error_cases.rs does not silently drop the coverage.
///
/// Exercises conformance.rs line 367 (visited-set dedup).
#[test]
fn circular_trait_refinement_no_panic() {
    let source = r#"
trait A : B {
    param x : Length
}

trait B : A {
    param y : Length
}

structure def S : A {
    param x : Length = 1mm
    param y : Length = 2mm
}
"#;
    // Must not panic — the visited-set prevents infinite recursion.
    // Whether errors are emitted is implementation-defined (no assertion on count).
    let module = compile_source(source);
    // Document: compilation completes without panic.
    let _ = errors_only(&module);
}

// ── Coverage summary ──────────────────────────────────────────────────────────

/// Document-only: this constant records the exact test count for this checkpoint
/// file. It is an author-maintained invariant — not a runtime assertion.
/// Update when adding or removing tests from this file.
///
/// Current coverage (task 294 + amendments):
///   1  smoke test
///   5  M8  type-alias & dimension-mismatch
///   6  M9  trait conformance
///   6  M9  constraint-def & termination
///   7  M9  meta, duplicate, port
///   4  M10 guard reference-safety
///   4  M11 annotation context
///   4  M11 pragma unknown
///   3  M10/M11 generic type argument
///   4  additional coverage (guard no-int/bool, missing sub-component,
///      cross-kind duplicate entity, circular trait refinement)
///   ───
///  44  total
///
/// This value equals the actual test count — keeping it at the exact count
/// (rather than a floor) makes discrepancies immediately visible during review.
#[allow(dead_code)]
const EXPECTED_MIN_TESTS: usize = 44;
