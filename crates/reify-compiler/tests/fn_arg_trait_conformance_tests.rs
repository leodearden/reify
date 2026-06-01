//! Integration tests for function-call-argument trait conformance (task-4081).
//!
//! Tests the end-to-end path: entity body `let bad = couple(FixedThing())` should
//! emit a `TypeNotConformingToTrait` error.  The module under test is
//! `phase_fn_arg_conformance` (wired into lib.rs after
//! `phase_pending_bound_checks`), which walks compiled exprs and calls
//! `check_fn_arg_conformance` for every `UserFunctionCall` whose params carry a
//! trait object.
//!
//! ## Why `compile_source` rather than `compile_source_with_stdlib`
//!
//! The source is self-contained (defines its own trait, structures, function, and
//! entity) so there is no need for the stdlib prelude.  Using `compile_source`
//! keeps the test fast and the dependency minimal.
//!
//! ## Why entity body, not free-function body
//!
//! SIR-α lowers `Foo()` to `StructureInstanceCtor` with `result_type =
//! StructureRef("Foo")` only when the template registry contains `Foo`.  That
//! registry is set in ENTITY scope (`entity.rs:401`, includes local templates)
//! but not in FUNCTION scope (functions compile before entities).  The integration
//! tests therefore drive conformance from an entity `let` body where the ctor arg
//! acquires its `StructureRef` type.

use reify_core::DiagnosticCode;
use reify_test_support::compile_source;

/// Common source preamble: trait, two structures, one fn.
fn preamble() -> &'static str {
    r#"
trait DrivingJoint {}

structure RevoluteJoint : DrivingJoint {
    param x : Real = 0.0
}

structure FixedThing {
    param y : Real = 0.0
}

fn couple(joint : DrivingJoint) -> Real {
    0.0
}
"#
}

/// Step-5 primary: calling `couple(FixedThing())` in an entity `let` binding
/// emits a `TypeNotConformingToTrait` error mentioning `FixedThing` and
/// `DrivingJoint`.
///
/// RED until step-6: step-2 makes the call resolve to a `UserFunctionCall` but
/// no conformance post-pass runs yet, so no `TypeNotConformingToTrait` is present.
#[test]
fn entity_let_bad_arg_emits_type_not_conforming_to_trait() {
    let source = format!(
        r#"{}
structure Test {{
    let bad = couple(FixedThing())
}}
"#,
        preamble()
    );
    let module = compile_source(&source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert_eq!(
        conformance_errors.len(),
        1,
        "expected exactly 1 TypeNotConformingToTrait diagnostic, got {}: {:?}",
        conformance_errors.len(),
        module.diagnostics
    );
    let msg = &conformance_errors[0].message;
    assert!(
        msg.contains("FixedThing"),
        "diagnostic should mention 'FixedThing'; got: {}",
        msg
    );
    assert!(
        msg.contains("DrivingJoint"),
        "diagnostic should mention 'DrivingJoint'; got: {}",
        msg
    );
}

// ── Step-7: constraint, wrapper-param, positive, and eval-builtin cases ─────

/// (a) A non-conforming call inside an entity CONSTRAINT emits TypeNotConformingToTrait.
///
/// RED until step-8: step-6 only walks value-cell default_exprs; constraint
/// exprs are not yet walked.
#[test]
fn entity_constraint_bad_arg_emits_type_not_conforming_to_trait() {
    let source = format!(
        r#"{}
structure T2 {{
    constraint couple(FixedThing()) > 0.0
}}
"#,
        preamble()
    );
    let module = compile_source(&source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert_eq!(
        conformance_errors.len(),
        1,
        "expected 1 TypeNotConformingToTrait in constraint, got {}: {:?}",
        conformance_errors.len(),
        module.diagnostics
    );
}

/// (b) Option<DrivingJoint> param called with some(FixedThing()) emits a conformance error.
///
/// RED until step-8: step-6 value-cell walk won't trigger Option wrapper
/// recursion into a struct built via couple_opt(some(FixedThing())).
/// Actually step-6 does walk value-cells — this MAY already pass. The plan
/// says RED because constraints are not yet walked; but the wrapper test is
/// for a value-cell. Let's keep it as a wrapper regression guard.
///
/// The wrapper param exercises `walk_param_against_arg`'s `Option` arm.
#[test]
fn option_wrapped_trait_param_bad_arg_emits_conformance_error() {
    let source = format!(
        r#"{}
fn couple_opt(joint : Option<DrivingJoint>) -> Real {{
    0.0
}}

structure T3 {{
    let bad = couple_opt(some(FixedThing()))
}}
"#,
        preamble()
    );
    let module = compile_source(&source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert_eq!(
        conformance_errors.len(),
        1,
        "expected 1 TypeNotConformingToTrait for Option<DrivingJoint> param with FixedThing arg, \
         got {}: {:?}",
        conformance_errors.len(),
        module.diagnostics
    );
}

/// (c) A conforming call produces NO TypeNotConformingToTrait error.
///
/// Positive guard: this must hold before and after step-8.
#[test]
fn entity_let_good_arg_emits_no_conformance_error() {
    let source = format!(
        r#"{}
structure TOk {{
    let ok = couple(RevoluteJoint())
}}
"#,
        preamble()
    );
    let module = compile_source(&source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert!(
        conformance_errors.is_empty(),
        "conforming call must produce no TypeNotConformingToTrait errors; got: {:?}",
        conformance_errors
    );
}

// ── Step-9: overload-collapse robustness ─────────────────────────────────────

/// Two same-name overloads (trait-first, concrete-second) — the name-keyed
/// HashMap in `phase_fn_arg_conformance` collapses both to the last-inserted
/// `couple(Real)` entry, so the conformance check is skipped and the
/// non-conforming call is missed.
///
/// This test is deterministically RED on the step-8 implementation because:
/// - `merge_prelude_functions` preserves declaration order, so
///   `resolution_functions == [couple(DrivingJoint), couple(Real), ...]`.
/// - A name-keyed `HashMap` collapses both to the LAST-inserted `couple(Real)`.
/// - `Real` is not trait-carrying → conformance check is skipped → error missed
///   (got 0, expected 1).
///
/// RED until step-10.
#[test]
fn overload_collapse_bad_arg_emits_type_not_conforming_to_trait() {
    let source = r#"
trait DrivingJoint {}

structure RevoluteJoint : DrivingJoint {
    param x : Real = 0.0
}

structure FixedThing {
    param y : Real = 0.0
}

fn couple(joint : DrivingJoint) -> Real { 0.0 }
fn couple(n : Real) -> Real { n }

structure Test {
    let bad = couple(FixedThing())
}

structure TOk {
    let ok = couple(RevoluteJoint())
}
"#;
    let module = compile_source(source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert_eq!(
        conformance_errors.len(),
        1,
        "expected exactly 1 TypeNotConformingToTrait diagnostic for couple(FixedThing()), \
         got {}: {:?}",
        conformance_errors.len(),
        module.diagnostics
    );
    let msg = &conformance_errors[0].message;
    assert!(
        msg.contains("FixedThing"),
        "diagnostic should mention 'FixedThing'; got: {}",
        msg
    );
    assert!(
        msg.contains("DrivingJoint"),
        "diagnostic should mention 'DrivingJoint'; got: {}",
        msg
    );
}

/// Companion positive guard for the overload-collapse scenario:
/// `couple(RevoluteJoint())` in `TOk` must NOT produce any
/// `TypeNotConformingToTrait` diagnostic.
///
/// This uses the same two-overload source as the RED test above but asserts
/// the conforming entity is clean.
#[test]
fn overload_collapse_good_arg_emits_no_conformance_error() {
    let source = r#"
trait DrivingJoint {}

structure RevoluteJoint : DrivingJoint {
    param x : Real = 0.0
}

structure FixedThing {
    param y : Real = 0.0
}

fn couple(joint : DrivingJoint) -> Real { 0.0 }
fn couple(n : Real) -> Real { n }

structure TOk {
    let ok = couple(RevoluteJoint())
}
"#;
    let module = compile_source(source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert!(
        conformance_errors.is_empty(),
        "conforming overloaded call must produce no TypeNotConformingToTrait errors; got: {:?}",
        conformance_errors
    );
}

/// Regression (task-4081 overload-resolution fix): with both a trait-object
/// overload `couple(DrivingJoint)` and a concrete overload `couple(Real)`, a
/// call with a concrete `Real` literal must resolve to `couple(Real)` and NOT
/// be reported as ambiguous.
///
/// Before the tie-break fix, the trait-object param acted as a wildcard that
/// also matched the `Real` arg, so `couple(2.0)` matched BOTH overloads →
/// `OverloadResolution::Ambiguous` → spurious "ambiguous function call"
/// compile error on previously-valid code. The fix prefers exact full-equality
/// matches over wildcard (trait-carrying) matches.
#[test]
fn concrete_arg_resolves_to_concrete_overload_not_ambiguous() {
    let source = r#"
trait DrivingJoint {}

structure RevoluteJoint : DrivingJoint {
    param x : Real = 0.0
}

fn couple(joint : DrivingJoint) -> Real { 0.0 }
fn couple(n : Real) -> Real { n }

structure Test {
    let r = couple(2.0)
}
"#;
    let module = compile_source(source);

    let ambiguous_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.message.contains("ambiguous function call"))
        .collect();

    assert!(
        ambiguous_errors.is_empty(),
        "concrete `couple(2.0)` must resolve to couple(Real), not Ambiguous; \
         got ambiguous diagnostics: {:?}",
        ambiguous_errors
    );

    // And the conforming concrete call must not trip any conformance error.
    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();
    assert!(
        conformance_errors.is_empty(),
        "concrete `couple(2.0)` must produce no TypeNotConformingToTrait errors; got: {:?}",
        conformance_errors
    );
}

// ── Soundness: scalar args to trait params must not be silently accepted ─────

/// Regression (task-4081 soundness, esc-4081-174): a non-conforming SCALAR arg
/// passed to a SOLE trait-object overload must still produce a diagnostic.
///
/// The wildcard relaxation in `resolve_function_overload` lets a trait param
/// match any arg type, so `couple(2.0)` against a lone `fn couple(DrivingJoint)`
/// resolves to that overload instead of producing a hard "no matching overload"
/// error. The conformance pass must then catch the scalar arg — otherwise the
/// previously-existing hard error is silently lost. The bare (unwrapped) leaf is
/// caught by the literal walker's fallback arm.
#[test]
fn bare_scalar_arg_to_sole_trait_overload_emits_conformance_error() {
    let source = r#"
trait DrivingJoint {}

fn couple(joint : DrivingJoint) -> Real { 0.0 }

structure Test {
    let r = couple(2.0)
}
"#;
    let module = compile_source(source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert_eq!(
        conformance_errors.len(),
        1,
        "scalar `couple(2.0)` against a sole trait overload must emit exactly 1 \
         TypeNotConformingToTrait; got {}: {:?}",
        conformance_errors.len(),
        module.diagnostics
    );
    assert!(
        conformance_errors[0].message.contains("DrivingJoint"),
        "diagnostic should mention 'DrivingJoint'; got: {}",
        conformance_errors[0].message
    );
}

/// Regression (task-4081 soundness, esc-4081-174): the WRAPPER-nested variant
/// reached through the type-level walker.
///
/// `couple_opt(joint : Option<DrivingJoint>)` called with a **non-literal**
/// (`ValueRef`) `Option<Real>` arg routes through `walk_param_against_arg_type`,
/// whose `Type::TraitObject` leaf previously delegated to
/// `emit_leaf_conformance_for_arg_type` and silently skipped non-struct/non-trait
/// leaf types. The scalar leaf must now emit a `TypeNotConformingToTrait`.
///
/// This is the path the literal walker does NOT cover (it only sees inline
/// `OptionSome(...)` literals, not a `ValueRef` bound to `some(2.0)`).
#[test]
fn wrapper_nested_valueref_scalar_arg_emits_conformance_error() {
    let source = r#"
trait DrivingJoint {}

fn couple_opt(joint : Option<DrivingJoint>) -> Real { 0.0 }

structure Test {
    let n = some(2.0)
    let r = couple_opt(n)
}
"#;
    let module = compile_source(source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert_eq!(
        conformance_errors.len(),
        1,
        "wrapper-nested ValueRef scalar arg `couple_opt(n)` must emit exactly 1 \
         TypeNotConformingToTrait; got {}: {:?}",
        conformance_errors.len(),
        module.diagnostics
    );
    assert!(
        conformance_errors[0].message.contains("DrivingJoint"),
        "diagnostic should mention 'DrivingJoint'; got: {}",
        conformance_errors[0].message
    );
}

/// Companion positive guard: a CONFORMING struct nested in an `Option<trait>`
/// param and passed as a `ValueRef` must NOT trip a conformance error — the new
/// scalar-leaf arm must not over-fire on the struct path that flows through the
/// type-level walker.
#[test]
fn wrapper_nested_valueref_conforming_struct_emits_no_error() {
    let source = r#"
trait DrivingJoint {}

structure RevoluteJoint : DrivingJoint {
    param x : Real = 0.0
}

fn couple_opt(joint : Option<DrivingJoint>) -> Real { 0.0 }

structure Test {
    let n = some(RevoluteJoint())
    let r = couple_opt(n)
}
"#;
    let module = compile_source(source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert!(
        conformance_errors.is_empty(),
        "conforming struct in Option<trait> via ValueRef must produce no \
         TypeNotConformingToTrait errors; got: {:?}",
        conformance_errors
    );
}

// ── Step-11: objective / realization / guarded-group coverage (soundness) ────
//
// The step-2 trait-object wildcard relaxation in `resolve_function_overload` is
// GLOBAL: a non-conforming `couple(FixedThing())` resolves to the trait overload
// at EVERY entity-scope call site, replacing the pre-change "no matching
// overload" hard error.  If `phase_fn_arg_conformance` walks ONLY value-cell
// defaults, constraints, and function bodies (the step-6/8/10 scope), the same
// non-conforming call placed in an objective / realization / guarded `where`
// block compiles with NO diagnostic — a silent loss of a previously-existing
// hard error (a soundness REGRESSION, not a merely-missing feature, because the
// resolution-level suppression ships in this same diff).
//
// Each test filters diagnostics by `DiagnosticCode::TypeNotConformingToTrait` so
// unrelated unit/type diagnostics never affect the count.
//
// Cases (a)/(b)/(c) are deterministically RED on the step-10 implementation
// (objective, realization, and guarded-group exprs are not walked); the two
// positive guards must hold both before and after step-12.

/// (a) A non-conforming call in an OBJECTIVE expr — `minimize couple(FixedThing())`
/// — emits TypeNotConformingToTrait.
///
/// The objective lives in `template.objective` = `Some(ObjectiveSet)` with a single
/// `Minimize` term (`minimize <expr>` is a valid structure member; boundary2_producer.rs:1303).
///
/// RED until step-12: `template.objective` is not walked by step-10.
#[test]
fn objective_bad_arg_emits_type_not_conforming_to_trait() {
    let source = format!(
        r#"{}
structure TObj {{
    param x : Real = 0.0
    minimize couple(FixedThing())
}}
"#,
        preamble()
    );
    let module = compile_source(&source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert_eq!(
        conformance_errors.len(),
        1,
        "expected exactly 1 TypeNotConformingToTrait in objective, got {}: {:?}",
        conformance_errors.len(),
        module.diagnostics
    );
    let msg = &conformance_errors[0].message;
    assert!(
        msg.contains("FixedThing") && msg.contains("DrivingJoint"),
        "diagnostic should mention 'FixedThing' and 'DrivingJoint'; got: {}",
        msg
    );
}

/// (b) A non-conforming call in a REALIZATION geometry-op arg —
/// `cylinder(couple(FixedThing()), 2.0)` — emits TypeNotConformingToTrait.
///
/// The geometry `let` is skipped from `value_cells` (`continue` at
/// entity.rs:1175-1177) and emitted as a realization, so the `couple(...)` call
/// sits in `realizations[*].operations[*].args` with NO double-count against a
/// value cell. (Bare-number primitive args are accepted — `cylinder(1, 2)`
/// precedent.)
///
/// RED until step-12: realizations are not walked by step-10.
#[test]
fn realization_bad_arg_emits_type_not_conforming_to_trait() {
    let source = format!(
        r#"{}
structure TReal {{
    let body = cylinder(couple(FixedThing()), 2.0)
}}
"#,
        preamble()
    );
    let module = compile_source(&source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert_eq!(
        conformance_errors.len(),
        1,
        "expected exactly 1 TypeNotConformingToTrait in realization, got {}: {:?}",
        conformance_errors.len(),
        module.diagnostics
    );
}

/// (c) A non-conforming call in a GUARDED `where` block member —
/// `where flag {{ let bad = couple(FixedThing()) }}` — emits TypeNotConformingToTrait.
///
/// Guard members live in `guarded_groups[*].members`, a vec distinct from
/// `value_cells`.
///
/// RED until step-12: guarded groups are not walked by step-10.
#[test]
fn guarded_group_bad_arg_emits_type_not_conforming_to_trait() {
    let source = format!(
        r#"{}
structure TGuard {{
    param flag : Bool = true
    where flag {{
        let bad = couple(FixedThing())
    }}
}}
"#,
        preamble()
    );
    let module = compile_source(&source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert_eq!(
        conformance_errors.len(),
        1,
        "expected exactly 1 TypeNotConformingToTrait in guarded group, got {}: {:?}",
        conformance_errors.len(),
        module.diagnostics
    );
}

/// Positive guard: a conforming `minimize couple(RevoluteJoint())` objective
/// produces NO TypeNotConformingToTrait error. Must hold before and after step-12.
#[test]
fn objective_good_arg_emits_no_conformance_error() {
    let source = format!(
        r#"{}
structure TObjOk {{
    param x : Real = 0.0
    minimize couple(RevoluteJoint())
}}
"#,
        preamble()
    );
    let module = compile_source(&source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert!(
        conformance_errors.is_empty(),
        "conforming objective call must produce no TypeNotConformingToTrait errors; got: {:?}",
        conformance_errors
    );
}

/// Positive guard: a conforming `where flag {{ let ok = couple(RevoluteJoint()) }}`
/// guarded member produces NO TypeNotConformingToTrait error.
#[test]
fn guarded_group_good_arg_emits_no_conformance_error() {
    let source = format!(
        r#"{}
structure TGuardOk {{
    param flag : Bool = true
    where flag {{
        let ok = couple(RevoluteJoint())
    }}
}}
"#,
        preamble()
    );
    let module = compile_source(&source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert!(
        conformance_errors.is_empty(),
        "conforming guarded call must produce no TypeNotConformingToTrait errors; got: {:?}",
        conformance_errors
    );
}

// ── Step-13: associated-function body coverage (soundness) ───────────────────
//
// The step-2 trait-object wildcard relaxation in `resolve_function_overload` is
// GLOBAL and ALSO reaches associated-function bodies: they are compiled through
// `compile_assoc_function -> compile_expr -> resolve_function_overload`
// (functions.rs:365), so a non-conforming `couple(self)` inside an assoc fn now
// resolves to a `UserFunctionCall` instead of the pre-change `no matching
// overload` hard error. But `phase_fn_arg_conformance` (step-12) walks ONLY
// `for_each_template_root_expr` (template ROOT exprs) + `ctx.functions` (free
// fns); it never walks `template.assoc_fns[*].function`, so that hard error is
// silently lost — a soundness REGRESSION shipping in this same diff.
//
// VEHICLE: `couple(self)`, NOT `couple(FixedThing())`. `compile_assoc_function`
// registers `self` as `Type::StructureRef(conformer)` (functions.rs:278/286/
// 348-351), giving the arg a clean `StructureRef` with NO dependency on the body
// scope's template registry — whereas a structure-ctor arg does NOT acquire a
// `StructureRef` in function/assoc-fn scope (the local template registry is set
// only in entity scope, entity.rs:401). Both cases use the trait DEFAULT-body
// injection form (proven by `default_assoc_fn_populates_template_table_end_to_end`
// in trait_assoc_fn_conformance_tests.rs): the default `measure` body lands in
// the conformer's `assoc_fns` with `self = StructureRef(conformer)`.

/// (a) A non-conforming `couple(self)` in a trait-default assoc-fn body injected
/// onto a NON-DrivingJoint conformer emits TypeNotConformingToTrait.
///
/// `trait Sized { fn measure(self) -> Real { couple(self) } }` provides the
/// default; `structure def Holder : Sized` injects it with `self =
/// StructureRef("Holder")`. `Holder` conforms only to `Sized` (not
/// `DrivingJoint`), so `couple(self)` is non-conforming.
///
/// RED until step-14: `template.assoc_fns[*].function` bodies are not walked by
/// step-12 (assoc-fn bodies are neither template root exprs nor in
/// `ctx.functions`), so the call is checked NOWHERE → got 0, expected 1.
#[test]
fn assoc_fn_body_bad_arg_emits_type_not_conforming_to_trait() {
    let source = format!(
        r#"{}
trait Sized {{
    fn measure(self) -> Real {{ couple(self) }}
}}

structure def Holder : Sized {{
    param y : Real = 0.0
}}
"#,
        preamble()
    );
    let module = compile_source(&source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert_eq!(
        conformance_errors.len(),
        1,
        "expected exactly 1 TypeNotConformingToTrait in assoc-fn body, got {}: {:?}",
        conformance_errors.len(),
        module.diagnostics
    );
    let msg = &conformance_errors[0].message;
    assert!(
        msg.contains("Holder") && msg.contains("DrivingJoint"),
        "diagnostic should mention 'Holder' and 'DrivingJoint'; got: {}",
        msg
    );
}

/// (b) Positive guard: a CONFORMING `couple(self)` in an assoc-fn body produces
/// NO TypeNotConformingToTrait error.
///
/// `trait DrivingSized : DrivingJoint { fn measure(self) -> Real { couple(self) } }`
/// refines `DrivingJoint`; `structure def Spinner : DrivingSized` conforms to
/// `DrivingSized` and therefore transitively to `DrivingJoint` (exercising
/// `satisfies_trait_bound`), so `couple(self)` with `self = StructureRef("Spinner")`
/// is conforming. Must hold both before and after step-14.
#[test]
fn assoc_fn_body_conforming_self_no_error() {
    let source = format!(
        r#"{}
trait DrivingSized : DrivingJoint {{
    fn measure(self) -> Real {{ couple(self) }}
}}

structure def Spinner : DrivingSized {{
    param x : Real = 0.0
}}
"#,
        preamble()
    );
    let module = compile_source(&source);

    let conformance_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.code == Some(DiagnosticCode::TypeNotConformingToTrait))
        .collect();

    assert!(
        conformance_errors.is_empty(),
        "conforming assoc-fn `couple(self)` must produce no TypeNotConformingToTrait \
         errors; got: {:?}",
        conformance_errors
    );
}
