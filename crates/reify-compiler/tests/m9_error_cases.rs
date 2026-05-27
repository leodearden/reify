//! M9 error case coverage — dedicated integration tests for every M9-category
//! error path in the compiler.
//!
//! Each test exercises a specific error diagnostic message emitted by the
//! compiler, asserting that:
//!   (a) the error message contains the expected substring, and
//!   (b) the first diagnostic label has a non-empty span.
//!
//! Source files covered:
//!   - conformance.rs  — trait conformance errors
//!   - entity.rs       — constraint instantiation, port/meta errors, type param bounds
//!   - expr.rs         — meta key access errors
//!   - termination.rs  — recursive termination errors
//!   - scc.rs          — duplicate template name (internal; tested via lib.rs path)
//!   - lib.rs          — duplicate entity definitions, duplicate unit declarations

use reify_test_support::{compile_source, compile_source_with_stdlib, errors_only};
use reify_core::{Diagnostic, DiagnosticCode};

/// Assert that `errors` contains at least one diagnostic whose `code` is `code`
/// and whose message contains every string in `msg_contains`.  That same
/// diagnostic must have at least one label with a non-empty span.
///
/// Combines the three-part `has_msg + has_code + label` pattern used in the
/// conformance test cases into a single failure-reporting assertion so each
/// test expresses its intent in one line rather than ten.
fn assert_has_diagnostic(errors: &[&Diagnostic], code: DiagnosticCode, msg_contains: &[&str]) {
    let matching: Vec<_> = errors
        .iter()
        .filter(|d| d.code == Some(code) && msg_contains.iter().all(|s| d.message.contains(s)))
        .collect();
    assert!(
        !matching.is_empty(),
        "expected a diagnostic with code {:?} whose message contains all of {:?}, got: {:?}",
        code,
        msg_contains,
        errors
            .iter()
            .map(|d| (d.code, d.message.as_str()))
            .collect::<Vec<_>>()
    );
    let d = matching[0];
    assert!(
        !d.labels.is_empty(),
        "expected the matching {:?} diagnostic to have at least one label; message: {:?}",
        code,
        d.message
    );
    assert!(
        !d.labels[0].span.is_empty(),
        "expected the matching {:?} diagnostic's first label to have a non-empty span; message: {:?}",
        code,
        d.message
    );
}

// ── Step 1: Trait conformance error tests ─────────────────────────────────────

/// A structure that declares a trait but omits the required param member
/// should produce "missing required member" diagnostic.
///
/// Exercises conformance.rs line 214.
#[test]
fn missing_required_param_member() {
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
    assert_has_diagnostic(
        &errors,
        DiagnosticCode::MissingRequiredMember,
        &["missing required member", "width"],
    );
}

/// A structure provides the required param but with the wrong type
/// should produce "type mismatch for trait member" diagnostic.
///
/// Exercises conformance.rs line 173-177.
#[test]
fn type_mismatch_for_trait_member() {
    let source = r#"
trait Shaped {
    param count : Int
}

structure def S : Shaped {
    param count : Length = 5mm
}
"#;

    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for type mismatch, got: {:?}",
        module.diagnostics
    );
    assert_has_diagnostic(
        &errors,
        DiagnosticCode::TypeMismatchForTraitMember,
        &["type mismatch for trait member", "count"],
    );
}

/// A structure that declares a trait requiring a sub-component of a specific type
/// but does not provide it should produce "missing required sub-component" diagnostic.
///
/// Exercises conformance.rs line 238.
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
    assert_has_diagnostic(
        &errors,
        DiagnosticCode::MissingRequiredSubComponent,
        &["missing required sub-component", "engine"],
    );
}

/// A structure passed as a trait-typed param argument that does not declare the
/// required trait should produce "does not conform to trait" diagnostic.
///
/// Exercises conformance/mod.rs check_trait_arg_conformance StructureRef arm
/// (Type::StructureRef path, conformance/mod.rs:183).
///
/// The TraitObject arm is covered by `trait_object_arg_does_not_conform` and
/// the fallback `_` arm by `fallback_arm_dimensional_param_as_trait_arg`, both below.
#[test]
fn type_does_not_conform_to_trait() {
    // Plain does not declare `: Material`, so passing Plain() where Material
    // is required triggers the StructureRef arm at conformance/mod.rs:189.
    let source = r#"
trait Material {}
structure def Plain { param density : Real = 1.0 }
structure def Host { param m : Material }
structure def Top { sub h = Host(m: Plain()) }
"#;

    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for non-conforming trait arg, got: {:?}",
        module.diagnostics
    );
    assert_has_diagnostic(
        &errors,
        DiagnosticCode::TypeNotConformingToTrait,
        &["does not conform to trait", "Material"],
    );
}

/// A trait-object-typed parameter passed where a different (non-refining) trait
/// is required should produce "does not conform to trait" diagnostic.
///
/// Exercises conformance/mod.rs check_trait_arg_conformance TraitObject arm
/// (Type::TraitObject path, conformance/mod.rs:205).  `Electrical` does not
/// refine `Mechanical`, so `trait_satisfies` returns false and the diagnostic
/// fires with TypeNotConformingToTrait.
#[test]
fn trait_object_arg_does_not_conform() {
    let source = r#"
trait Mechanical {}
trait Electrical {}
structure def Host { param m : Mechanical }
structure def Top {
    param e : Electrical
    sub h = Host(m: e)
}
"#;

    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for incompatible trait-object arg, got: {:?}",
        module.diagnostics
    );
    assert_has_diagnostic(
        &errors,
        DiagnosticCode::TypeNotConformingToTrait,
        &["does not conform to trait", "Mechanical"],
    );
}

/// Passing a non-structure, non-trait-object argument (a dimensional param) to a
/// trait-typed param exercises the fallback `_` arm of `check_trait_arg_conformance`
/// in conformance/mod.rs.
///
/// `thickness : Length` is not a function call, so `arg_call_name` is `None`.
/// The anti-cascade guard (`arg_type is Real/Int AND arg_call_name.is_some()`) does
/// not fire, so the fallback emits `TypeNotConformingToTrait` regardless of how
/// the expression compiler represents the dimensional type internally.
#[test]
fn fallback_arm_dimensional_param_as_trait_arg() {
    // `thickness` is a Length-typed param reference, not a StructureRef or
    // TraitObject.  Passing it where `Material` is required hits the `_` arm.
    let source = r#"
trait Material {}
structure def Host { param m : Material }
structure def Top {
    param thickness : Length = 5mm
    sub h = Host(m: thickness)
}
"#;

    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for dimensional-value-as-trait-arg, got: {:?}",
        module.diagnostics
    );
    assert_has_diagnostic(
        &errors,
        DiagnosticCode::TypeNotConformingToTrait,
        &["does not conform to trait", "Material"],
    );
}

/// A trait with a let binding whose explicit type annotation is incompatible
/// with the expression's inferred type should produce a TypeMismatchForTraitMember
/// diagnostic when the default is injected into a conforming structure.
///
/// Exercises conformance/checker.rs check_phase_inject_defaults annotation
/// cross-check (checker.rs line ~1176).  The annotation `Mass` is incompatible
/// with the Real-typed literal `1.5`, so type_compatible(Mass, Real) returns
/// false and the diagnostic fires.
#[test]
fn trait_let_annotation_type_mismatch() {
    let source = r#"
trait BadAnnotation {
    let score : Mass = 1.5
}
structure def S : BadAnnotation { }
"#;

    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for trait let annotation mismatch, got: {:?}",
        module.diagnostics
    );
    assert_has_diagnostic(
        &errors,
        DiagnosticCode::TypeMismatchForTraitMember,
        &["type mismatch for trait let", "score"],
    );
}

// ── Step 3: Unresolved trait error test ──────────────────────────────────────

/// A structure declaring a trait bound that does not exist in the module
/// should produce "unresolved trait" diagnostic.
///
/// Exercises conformance.rs line 373.
#[test]
fn unresolved_trait() {
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

// ── Step 5: Conflicting trait requirement tests ───────────────────────────────

/// Two traits require the same member name with different types. The structure
/// implementing both should produce "conflicting trait requirements" diagnostic.
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
            // Keep the 'x' name-token check: when multiple conflicts share the
            // same code, the member name carries semantic content beyond wording.
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Two traits provide `let` bindings with the same name but different expressions.
/// The structure implementing both (without overriding) should produce
/// "conflicting trait let bindings" diagnostic.
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Two traits provide param defaults with the same name but different types.
/// The structure implementing both should produce "conflicting trait defaults" diagnostic.
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

// ── Step 7: Constraint definition error tests ─────────────────────────────────

/// Using an unknown constraint definition name in a structure should produce
/// "unknown constraint definition" diagnostic.
///
/// Exercises entity.rs line 1075.
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Passing an argument name that does not exist in the constraint definition
/// should produce "unknown argument" diagnostic.
///
/// Exercises entity.rs line 1101.
#[test]
fn unknown_argument_in_constraint_instantiation() {
    let source = r#"
constraint def MinWall {
    param wall : Length
    wall > 0
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Omitting a required argument (one with no default) in a constraint instantiation
/// should produce "missing argument" diagnostic.
///
/// Exercises entity.rs line 1118.
#[test]
fn missing_argument_in_constraint_instantiation() {
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

// ── Step 9: Type parameter bound error tests ──────────────────────────────────

/// Providing more type arguments than a generic structure has type parameters
/// should produce "too many type arguments" diagnostic.
///
/// Exercises entity.rs line 1573.
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Omitting a required type argument (type parameter with no default)
/// should produce "missing type argument" diagnostic.
///
/// Exercises entity.rs line 1597.
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Passing a type argument that does not satisfy the bound on the type parameter
/// should produce "does not satisfy bound" diagnostic.
///
/// Exercises entity.rs line 1635.
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

// ── Step 11: Termination error tests ─────────────────────────────────────────

/// A recursive sub without any where-clause guard should produce
/// "no termination condition" diagnostic.
///
/// Exercises termination.rs line 39-43.
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// A recursive sub's where-clause guard that does not reference any Int or Bool
/// parameter should produce "guard does not reference any Int or Bool" diagnostic.
///
/// Exercises termination.rs line 63-67.
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// A recursive sub using `undef` for a guard-referenced param argument
/// should produce "undef is not allowed" diagnostic.
///
/// Exercises termination.rs line 79.
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// A recursive sub whose guard-referenced param is passed unchanged (not decremented)
/// should produce "does not decrement parameter" diagnostic.
///
/// Exercises termination.rs line 98-103.
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

// ── Step 13: Circular trait + duplicate template tests ────────────────────────

/// Circular trait refinement (A refines B, B refines A) should not panic.
/// The compiler uses a visited-set to avoid infinite loops, so it handles the
/// cycle gracefully. This test documents the observed behavior — either a cycle
/// error or silent incomplete requirement collection, but never a panic.
///
/// Exercises conformance.rs line 367 (visited-set dedup).
#[test]
fn circular_trait_refinement_no_panic() {
    // Note: Reify syntax for refinement is `trait A : B { ... }`.
    // Circular refinement: A refines B, B refines A.
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

    // Should not panic — the visited-set in collect_all_requirements prevents
    // infinite recursion. Behavior (errors or not) is implementation-defined.
    let module = compile_source(source);

    // Document: compilation completes without panic. Errors may or may not appear.
    // If no explicit cycle diagnostic is emitted, that's acceptable behavior.
    let _ = errors_only(&module);
}

// ── Task 403: Depth / visited guard ordering in collect_all_requirements ──────

/// A 130-level linear trait refinement chain (deeper than MAX_TRAIT_DEPTH=128)
/// should complete compilation without a stack overflow and emit exactly ONE
/// "trait refinement chain too deep" diagnostic.
///
/// Guards: (1) compilation returns (no panic/SO), (2) exactly one "too deep"
/// error, (3) that error's first label has a non-empty span.
///
/// Exercises conformance.rs `collect_all_requirements` depth guard.
#[test]
fn trait_refinement_chain_too_deep_single_path() {
    // Build a 130-level linear chain: T0, T1 : T0, T2 : T1, ..., T130 : T129
    let mut src = String::new();
    src.push_str("trait T0 { }\n");
    for i in 1..=130 {
        src.push_str(&format!("trait T{} : T{} {{ }}\n", i, i - 1));
    }
    // Structure bound to the deepest trait
    src.push_str("structure def S : T130 { }\n");

    // Must not panic / stack-overflow
    let module = compile_source(&src);

    let errors = errors_only(&module);
    let too_deep: Vec<_> = errors
        .iter()
        .filter(|d| d.message.contains("trait refinement chain too deep"))
        .collect();

    assert_eq!(
        too_deep.len(),
        1,
        "expected exactly one 'trait refinement chain too deep' diagnostic, got {}: {:?}",
        too_deep.len(),
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    let diag = too_deep[0];
    assert!(
        !diag.labels.is_empty(),
        "expected at least one label on the 'too deep' diagnostic"
    );
    assert!(
        !diag.labels[0].span.is_empty(),
        "expected a non-empty span on the 'too deep' diagnostic label"
    );
}

/// Diamond refinement convergence on a deep shared tail should produce exactly
/// ONE "trait refinement chain too deep" diagnostic, not two.
///
/// Both chains A128→A127→...→A0→Shared and B128→B127→...→B0→Shared reach
/// the shared trait `Shared` at depth 129 > MAX_TRAIT_DEPTH (128).
///
/// With the visited-FIRST ordering (task 403's fix): chain A's depth-limit
/// branch INSERTS `Shared` into the visited set before returning, so chain
/// B's subsequent walk of `Shared` hits the visited check and short-circuits
/// silently — exactly ONE diagnostic is emitted.
///
/// With the buggy depth-FIRST ordering, `Shared` would NOT be inserted on
/// chain A's depth-fail branch, so chain B would also reach depth 129 and
/// emit a second "too deep" diagnostic — a duplicate.
///
/// Exercises conformance.rs `collect_all_requirements` guard ordering (task 403).
#[test]
fn trait_refinement_too_deep_diamond_no_duplicate_diagnostic() {
    // Shared trait reached by both chains at depth 129 > MAX_TRAIT_DEPTH (128).
    let mut src = String::new();
    src.push_str("trait Shared { }\n");

    // Chain A: A0 refines Shared; A1 refines A0; ...; A128 refines A127.
    // Walking from A128 at depth 0 reaches Shared at depth 129.
    src.push_str("trait A0 : Shared { }\n");
    for i in 1..=128 {
        src.push_str(&format!("trait A{} : A{} {{ }}\n", i, i - 1));
    }

    // Chain B: identical structure, independent path to the same Shared trait.
    src.push_str("trait B0 : Shared { }\n");
    for i in 1..=128 {
        src.push_str(&format!("trait B{} : B{} {{ }}\n", i, i - 1));
    }

    // Structure bound to both deepest traits via '+' (Reify multi-bound syntax).
    src.push_str("structure def S : A128 + B128 { }\n");

    // Must not panic
    let module = compile_source(&src);

    let errors = errors_only(&module);
    let too_deep_count = errors
        .iter()
        .filter(|d| d.message.contains("trait refinement chain too deep"))
        .count();

    // Visited-first ordering (task 403 fix): Shared is inserted on chain A's
    // depth-limit branch, so chain B's walk of Shared short-circuits silently.
    assert_eq!(
        too_deep_count,
        1,
        "expected exactly ONE 'trait refinement chain too deep' diagnostic \
         (visited-first dedup); got {}: {:?}",
        too_deep_count,
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Two entity definitions with the same name should produce
/// "duplicate entity definition" diagnostic (from lib.rs pass 1).
///
/// Note: the scc.rs "duplicate template name" error (scc.rs line 22) is an
/// internal consistency check that is unreachable from valid user-level source —
/// lib.rs deduplicates structures before they reach the SCC pass.
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

// ── Step 15: Duplicate unit name tests ───────────────────────────────────────

/// Two module-local unit declarations with the same name should produce
/// "duplicate unit declaration" diagnostic.
///
/// Exercises lib.rs line 291.
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// A module-local unit declaration that shadows a stdlib prelude unit should
/// produce "already defined in stdlib prelude" diagnostic.
///
/// Exercises lib.rs line 279.
#[test]
fn duplicate_unit_declaration_shadows_stdlib() {
    // 'mm' is a stdlib prelude unit. Declaring it locally should produce an error.
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

// ── Step 17: Meta key error tests ─────────────────────────────────────────────

/// Accessing `meta.key` when the entity has no meta block at all
/// should produce "entity has no meta block" diagnostic.
///
/// Exercises expr.rs line 845.
#[test]
fn meta_access_entity_has_no_meta_block() {
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Accessing `meta.key` when the meta block exists but the key is absent
/// should produce "meta block has no key" diagnostic.
///
/// Exercises expr.rs line 854.
#[test]
fn meta_access_block_has_no_key() {
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

// ── Step 19: Duplicate entity name and duplicate port name tests ───────────────

/// A structure and an occurrence with the same name should produce
/// "duplicate entity definition" diagnostic (cross-kind collision in the
/// unified entity namespace per spec §4.2.1).
///
/// Exercises lib.rs line 159.
#[test]
fn duplicate_entity_structure_and_occurrence_collision() {
    let source = r#"
occurrence def Widget {
    param x : Length = 1mm
}

structure def Widget {
    param x : Length = 1mm
}
"#;

    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        !errors.is_empty(),
        "expected at least one error for occurrence/structure name collision, got: {:?}",
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}

/// Two ports with the same name in the same structure should produce
/// "duplicate port name" diagnostic.
///
/// Exercises entity.rs line 348.
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

    let first = errors[0];
    assert!(!first.labels.is_empty(), "expected at least one label");
    assert!(!first.labels[0].span.is_empty(), "expected non-empty span");
}
