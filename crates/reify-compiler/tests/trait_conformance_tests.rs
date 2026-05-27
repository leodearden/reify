//! Trait conformance compilation tests.
//!
//! Tests for compiling trait declarations, conformance checking,
//! default merging, and composition conflict detection.

use reify_compiler::*;
use reify_test_support::{compile_first_template, compile_source};
use reify_core::*;

/// Assert that `template.value_cells` contains exactly one cell whose member name equals
/// `member`. Prints `context` in the failure message for easy diagnosis.
/// Returns a reference to the matched cell so callers can inspect its properties.
#[must_use = "inspect the returned cell or use `let _ =` to explicitly discard"]
fn assert_single_value_cell<'a>(
    template: &'a TopologyTemplate,
    member: &str,
    context: &str,
) -> &'a ValueCellDecl {
    let cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == member)
        .collect();
    assert_eq!(
        cells.len(),
        1,
        "{}: expected exactly 1 value cell '{}', got {}: {:?}",
        context,
        member,
        cells.len(),
        cells
    );
    cells[0]
}

/// Step 1: Compile a trait declaration produces CompiledTrait in CompiledModule.trait_defs.
#[test]
fn compile_trait_produces_compiled_trait() {
    let source = r#"
trait Fastener {
    param thread_pitch : Length
}
"#;

    let module = compile_source(source);

    // Should have 1 trait def
    assert_eq!(module.trait_defs.len(), 1, "expected 1 trait def");
    let trait_def = &module.trait_defs[0];

    // Name should be "Fastener"
    assert_eq!(trait_def.name, "Fastener");

    // Should have 1 required member named "thread_pitch"
    assert_eq!(
        trait_def.required_members.len(),
        1,
        "expected 1 required member"
    );
    let req = &trait_def.required_members[0];
    assert_eq!(req.name, "thread_pitch");

    // Requirement kind should be Param with type Scalar{LENGTH}
    match &req.kind {
        RequirementKind::Param(ty) => {
            assert_eq!(
                *ty,
                Type::Scalar {
                    dimension: DimensionVector::LENGTH
                }
            );
        }
        other => panic!("expected RequirementKind::Param, got {:?}", other),
    }
}

/// Step 3: Simple conformance — structure satisfies trait requirement.
#[test]
fn simple_conformance_no_errors() {
    let source = r#"
trait Fastener {
    param thread_pitch : Length
}

structure def Bolt : Fastener {
    param thread_pitch : Length = 20mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    // No error-severity diagnostics expected
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

/// Step 15: Diamond inheritance — requirement from C reachable via both A and B.
#[test]
fn diamond_inheritance_deduplication() {
    let source = r#"
trait C {
    param x : Length
}

trait A : C {
}

trait B : C {
}

structure def X : A + B {
    param x : Length = 5mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

/// Step 5: Missing member — error diagnostic about missing required member.
#[test]
fn missing_member_error() {
    let source = r#"
trait Fastener {
    param thread_pitch : Length
}

structure def Bolt : Fastener {
    param length : Length = 10mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected error diagnostic for missing member"
    );

    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("missing required member") && error_msg.contains("thread_pitch"),
        "error should mention 'missing required member' and 'thread_pitch', got: {}",
        error_msg
    );
}

/// Step 7: Type mismatch — member has wrong type.
#[test]
fn type_mismatch_error() {
    let source = r#"
trait Weighted {
    param mass : Mass
}

structure def S : Weighted {
    param mass : Length = 5mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected error diagnostic for type mismatch"
    );

    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("type mismatch"),
        "error should mention 'type mismatch', got: {}",
        error_msg
    );
}

/// Step 9: Default merging — trait provides default, structure doesn't override.
#[test]
fn default_merging_injects_value_cell() {
    let source = r#"
trait HasSize {
    param size : Length = 10mm
}

structure def S : HasSize {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // No error-severity diagnostics expected
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // The template should contain a value cell for 'size' injected from the trait default.
    let size_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "size");
    assert!(
        size_cell.is_some(),
        "expected 'size' value cell from trait default, got cells: {:?}",
        template
            .value_cells
            .iter()
            .map(|vc| &vc.id.member)
            .collect::<Vec<_>>()
    );

    let size_cell = size_cell.unwrap();
    assert_eq!(size_cell.kind, ValueCellKind::Param);
    assert_eq!(
        size_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::LENGTH
        }
    );
    assert!(
        size_cell.default_expr.is_some(),
        "expected default expression for 'size'"
    );
}

/// Step 11: Default override — structure provides its own value, no error, only one cell.
#[test]
fn default_override_uses_structure_value() {
    let source = r#"
trait HasSize {
    param size : Length = 10mm
}

structure def S : HasSize {
    param size : Length = 20mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // No error-severity diagnostics expected
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Only one 'size' value cell should exist (the structure's, not the trait default).
    let size_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "size")
        .collect();
    assert_eq!(
        size_cells.len(),
        1,
        "expected exactly 1 'size' value cell, got {}",
        size_cells.len()
    );
}

/// Step 13: Multiple trait bounds — structure satisfies both traits.
#[test]
fn multiple_trait_bounds_satisfied() {
    let source = r#"
trait A {
    param a : Length
}

trait B {
    param b : Length
}

structure def X : A + B {
    param a : Length = 1mm
    param b : Length = 2mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected errors for multi-trait: {:?}",
        errors
    );
}

/// Step 17: Composition conflict — same name, different types across traits.
#[test]
fn composition_conflict_error() {
    let source = r#"
trait A {
    param size : Length
}

trait B {
    param size : Mass
}

structure def X : A + B {
    param size : Length = 5mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected error for conflicting requirements"
    );

    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("conflicting"),
        "error should mention 'conflicting', got: {}",
        error_msg
    );
}

/// Step 19: Deep trait chain — C→B→A, structure must satisfy all.
#[test]
fn deep_trait_chain() {
    let source = r#"
trait A {
    param x : Length
}

trait B : A {
    param y : Length
}

trait C : B {
    param z : Length
}

structure def S : C {
    param x : Length = 1mm
    param y : Length = 2mm
    param z : Length = 3mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected errors for deep chain: {:?}",
        errors
    );
}

/// Step 21: Constraint from trait — default constraint is injected.
#[test]
fn constraint_from_trait_injected() {
    let source = r#"
trait Safe {
    param x : Length
    constraint x > 0mm
}

structure def S : Safe {
    param x : Length = 5mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // The constraint from the trait should be injected
    assert!(
        !template.constraints.is_empty(),
        "expected at least 1 constraint from trait default"
    );
}

/// Step 23: Duplicate default injection — two distinct traits with same-named default param.
/// Currently `collect_all_requirements` pushes defaults unconditionally, producing TWO
/// ValueCellDecl entries for 'size'. Test asserts exactly one 'size' value cell exists.
#[test]
fn duplicate_default_injection_deduped() {
    let source = r#"
trait A {
    param size : Length = 10mm
}

trait B {
    param size : Length = 5mm
}

structure def X : A + B {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // No error-severity diagnostics expected (same name + same type → dedup, not conflict).
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Exactly one 'size' value cell should exist (not two).
    let size_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "size")
        .collect();
    assert_eq!(
        size_cells.len(),
        1,
        "expected exactly 1 'size' value cell after dedup, got {}",
        size_cells.len()
    );
}

/// Step 25a: Default conflict across traits with different types.
/// Two traits provide defaults for 'size' with different types → conflict diagnostic.
#[test]
fn default_conflict_different_types() {
    let source = r#"
trait A {
    param size : Length = 10mm
}

trait B {
    param size : Mass = 5kg
}

structure def X : A + B {
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected conflict diagnostic");

    let error_msg = format!("{:?}", errors);
    assert!(
        error_msg.contains("conflicting") && error_msg.contains("size"),
        "error should mention 'conflicting' and 'size', got: {}",
        error_msg
    );
}

/// Step 25b: Default conflict resolution — structure overrides the conflicting default.
/// When the structure provides its own member, the conflict is moot — no diagnostic.
#[test]
fn default_conflict_resolved_by_override() {
    let source = r#"
trait A {
    param size : Length = 10mm
}

trait B {
    param size : Mass = 5kg
}

structure def Y : A + B {
    param size : Length = 7mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // No error diagnostics — the structure provides 'size', resolving the conflict.
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected errors when structure overrides: {:?}",
        errors
    );

    // Only one 'size' value cell.
    let size_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "size")
        .collect();
    assert_eq!(size_cells.len(), 1, "expected exactly 1 'size' value cell");
}

/// Step 27a: Unlabeled constraint defaults from two traits — both injected.
/// Since labeled constraints are not yet supported in the grammar (label is always None),
/// unlabeled constraints from distinct traits are both injected (no dedup for unnamed).
#[test]
fn unlabeled_constraint_defaults_from_two_traits() {
    let source = r#"
trait A {
    param x : Length
    constraint x > 0mm
}

trait B {
    param x : Length
    constraint x > 0mm
}

structure def X : A + B {
    param x : Length = 5mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Both unlabeled constraints are injected (unnamed defaults always push).
    assert!(
        template.constraints.len() >= 2,
        "expected at least 2 constraints from two traits, got {}",
        template.constraints.len()
    );
}

/// Step 27b: Structure provides its own constraint — trait constraints still injected
/// (since all are unlabeled and there's no label-based override).
#[test]
fn structure_constraint_with_trait_constraints() {
    let source = r#"
trait A {
    param x : Length
    constraint x > 0mm
}

structure def X : A {
    param x : Length = 5mm
    constraint x > 1mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Structure's constraint + trait's unlabeled constraint = at least 2.
    assert!(
        template.constraints.len() >= 2,
        "expected at least 2 constraints (structure + trait), got {}",
        template.constraints.len()
    );
}

/// Task-189 step-1: Single-bound deep diamond satisfied — D's requirement collected once.
/// Topology: S:A, A:B+C, B:D, C:D, D has `param x : Length`.  S provides x.
#[test]
fn diamond_single_bound_satisfied() {
    let source = r#"
trait D {
    param x : Length
}

trait B : D {
}

trait C : D {
}

trait A : B + C {
}

structure def S : A {
    param x : Length = 5mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // x comes from D, reachable via both B->D and C->D paths; must appear exactly once.
    let x_cell = assert_single_value_cell(&template, "x", "diamond_satisfies_once");
    assert_eq!(
        x_cell.cell_type,
        Type::length(),
        "diamond_satisfies_once: x must be Length"
    );
}

/// Task-189 step-3: Missing member in deep diamond produces exactly 1 error.
/// Same hierarchy (S:A, A:B+C, B:D, C:D, D has `param x : Length`) but S omits x.
#[test]
fn diamond_missing_member_single_error() {
    let source = r#"
trait D {
    param x : Length
}

trait B : D {
}

trait C : D {
}

trait A : B + C {
}

structure def S : A {
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected at least 1 missing-member error"
    );

    // Exactly 1 error (not 2 from B-path and C-path to D).
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 error for missing x, got {}: {:?}",
        errors.len(),
        errors
    );

    let msg = &errors[0].message;
    assert!(
        msg.contains("missing required member 'x'"),
        "expected 'missing required member 'x'' in error message, got: {}",
        msg
    );
}

/// Task-189 step-5: Default from D injected exactly once in deep diamond.
/// D provides `param x : Length = 10mm`, S:A with no override.
#[test]
fn diamond_default_injected_once() {
    let source = r#"
trait D {
    param x : Length = 10mm
}

trait B : D {
}

trait C : D {
}

trait A : B + C {
}

structure def S : A {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Exactly 1 value cell named 'x' (no duplication from diamond).
    let x_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "x")
        .collect();
    assert_eq!(
        x_cells.len(),
        1,
        "expected exactly 1 value cell 'x', got {}: {:?}",
        x_cells.len(),
        x_cells
    );
}

/// Task-189 step-7: Constraint from D injected exactly once in deep diamond.
/// D has `param x : Length` and `constraint x > 0mm`, S:A provides x.
#[test]
fn diamond_constraint_from_root() {
    let source = r#"
trait D {
    param x : Length
    constraint x > 0mm
}

trait B : D {
}

trait C : D {
}

trait A : B + C {
}

structure def S : A {
    param x : Length = 5mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Exactly 1 constraint injected from D (not 2 or 3 from multiple paths).
    assert_eq!(
        template.constraints.len(),
        1,
        "expected exactly 1 constraint from D, got {}: {:?}",
        template.constraints.len(),
        template.constraints
    );
}

/// Task-189 step-9: Members at every level of the deep diamond collected correctly.
/// D has param d, B has param b, C has param c, A has param a. S:A provides all four.
#[test]
fn diamond_members_at_every_level() {
    let source = r#"
trait D {
    param d : Length
}

trait B : D {
    param b : Length
}

trait C : D {
    param c : Length
}

trait A : B + C {
    param a : Length
}

structure def S : A {
    param d : Length = 1mm
    param b : Length = 2mm
    param c : Length = 3mm
    param a : Length = 4mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // d comes from trait D, reachable via both B->D and C->D; must appear exactly once.
    let d_cell = assert_single_value_cell(&template, "d", "diamond_default_from_D_once");
    assert_eq!(
        d_cell.cell_type,
        Type::length(),
        "diamond_default_from_D_once: d must be Length"
    );
}

/// Task-189 step-11: Let default from D injected exactly once in deep diamond.
/// D provides `let y = 42`, S:A (no override).
#[test]
fn diamond_let_default_deduped() {
    let source = r#"
trait D {
    let y = 42
}

trait B : D {
}

trait C : D {
}

trait A : B + C {
}

structure def S : A {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Exactly 1 value cell named 'y' (let default dedup in diamond).
    let y_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "y")
        .collect();
    assert_eq!(
        y_cells.len(),
        1,
        "expected exactly 1 value cell 'y', got {}: {:?}",
        y_cells.len(),
        y_cells
    );
}

/// Task-189 step-13: Deep 5-level diamond — E:D, B:E, C:D, A:B+C, S:A.
/// D has param x, E has param y. S provides both. Verify no errors, no duplication.
#[test]
fn diamond_deep_five_level() {
    let source = r#"
trait D {
    param x : Length
}

trait E : D {
    param y : Length
}

trait B : E {
}

trait C : D {
}

trait A : B + C {
}

structure def S : A {
    param x : Length = 1mm
    param y : Length = 2mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // x comes from D, reachable via B->E->D and C->D; must appear exactly once.
    let x_cell = assert_single_value_cell(&template, "x", "deep_diamond_A_B_E_D_C_D");

    // y comes from E, reachable via one path (B->E); must also appear exactly once.
    let y_cell = assert_single_value_cell(&template, "y", "deep_diamond_A_B_E_D_C_D");

    assert_eq!(
        x_cell.cell_type,
        Type::length(),
        "deep_diamond x cell_type mismatch"
    );
    assert_eq!(
        y_cell.cell_type,
        Type::length(),
        "deep_diamond y cell_type mismatch"
    );
}

/// Task-384 step-1: Diamond with conflicting param types produces exactly 1 error.
/// Topology: D {}, B:D { param x : Length }, C:D { param x : Angle }, A:B+C {}, S:A { x provided }.
/// S provides x : Length to satisfy B's requirement; only the conflict diagnostic remains.
/// The seen_names conflict detection in collect_all_requirements should fire once.
#[test]
fn diamond_type_conflict_produces_error() {
    let source = r#"
trait D {
}

trait B : D {
    param x : Length
}

trait C : D {
    param x : Angle
}

trait A : B + C {
}

structure def S : A {
    param x : Length = 5mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 conflict error, got {}: {:?}",
        errors.len(),
        errors
    );

    assert_eq!(
        errors[0].code,
        Some(DiagnosticCode::ConflictingTraitRequirements),
        "expected DiagnosticCode::ConflictingTraitRequirements, got: {:?}",
        errors[0]
    );

    // Keep the 'x' name-token assertion: when multiple conflicts could share the
    // same code, the member name carries semantic content beyond wording.
    assert!(
        errors[0].message.contains("x"),
        "expected mention of 'x' in conflict message, got: {}",
        errors[0].message
    );

    assert!(
        !errors[0].labels.is_empty(),
        "expected at least one label on conflict diagnostic"
    );
    assert!(
        !errors[0].labels[0].span.is_empty(),
        "expected non-empty span on conflict diagnostic label"
    );

    // Even with a type conflict error, dedup must still produce exactly one cell for 'x'.
    let x_cell = assert_single_value_cell(&template, "x", "diamond_type_conflict");
    assert_eq!(
        x_cell.cell_type,
        Type::length(),
        "diamond_type_conflict: x cell_type should be Length (structure's own declaration wins)"
    );
}

/// Step 21b: Trait with constraint and param — both injected correctly.
#[test]
fn trait_constraint_and_param_both_injected() {
    let source = r#"
trait Safe {
    param x : Length = 5mm
    constraint x > 0mm
}

structure def S : Safe {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Both param default and constraint should be injected.
    let has_x = template.value_cells.iter().any(|vc| vc.id.member == "x");
    assert!(has_x, "expected value cell 'x' from trait default");

    assert!(
        !template.constraints.is_empty(),
        "expected constraint from trait default"
    );
}

/// Suggestion #17 (incomplete_coverage): A parent trait's default satisfies a child
/// trait's inherited requirement via the refinement chain — no error expected.
///
/// trait Parent { param x : Length = 10mm }
/// trait Child : Parent {}
/// structure def S : Child {}
///
/// The parent provides a default for `x`; `Child` inherits the requirement.
/// `collect_all_requirements` walks the refinement chain depth-first, so the
/// default from `Parent` is visible when checking `S : Child`.
/// Assert: no Error-severity diagnostics.
#[test]
fn parent_default_satisfies_child_requirement_via_refinement_chain() {
    let source = r#"
trait Parent {
    param x : Length = 10mm
}

trait Child : Parent {}

structure def S : Child {}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "parent default should satisfy child requirement via refinement chain; \
         unexpected errors: {:?}",
        errors
    );
}

/// Suggestion #4 (semantic_gap_in_api): A structure `let` member must NOT silently
/// satisfy a trait `param` requirement.
///
/// `param` in a trait means "an externally-settable slot"; `let` in a structure
/// is a computed binding that cannot be set from outside. Accepting a let-in-structure
/// as satisfying a param-in-trait requirement would allow consumers to treat a
/// non-settable binding as a settable parameter, which is semantically wrong.
///
/// Source:
///   trait A { param x : Length }
///   structure def S : A { let x : Length = 5mm }
///
/// Expected: Error-severity diagnostic whose message mentions "missing required" and "x"
/// (or explicitly mentions the param-vs-let kind mismatch).
///
/// NOTE: This test is expected to FAIL on current HEAD — `structure_members` currently
/// stores both param and let entries under the same name→type key, so the let silently
/// satisfies the param requirement. Step-8 implements the kind-aware lookup fix.
#[test]
fn structure_let_does_not_satisfy_param_requirement() {
    let source = r#"
trait A {
    param x : Length
}

structure def S : A {
    let x : Length = 5mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        !errors.is_empty(),
        "expected an error: let x does not satisfy param x requirement, but got no errors"
    );

    let mentions_missing = errors
        .iter()
        .any(|d| d.message.contains("missing required member") && d.message.contains("'x'"));
    assert!(
        mentions_missing,
        "error should mention missing required member 'x', got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // Also verify the diagnostic names the required kind so users can see WHY the
    // let member didn't satisfy the param requirement (not just that it's missing).
    let mentions_param_kind = errors
        .iter()
        .any(|d| d.message.contains("param") || d.message.contains("requires a"));
    assert!(
        mentions_param_kind,
        "error should explain that a `param` slot is required (not just that the member is missing), \
         got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// [#21 RED] Two traits A and B both declare `sub hole = Hole()`; structure S : A + B {}
/// has no `sub hole` member. Currently (without the Sub dedup fix) TWO duplicate
/// "missing required sub-component" errors fire because `collect_all_requirements`
/// only deduplicates Param/Let requirements via `seen_names`, not Sub requirements.
/// After the fix in step-2 (`seen_sub_names: HashSet<String>` added to `MergeContext`),
/// exactly ONE error should fire.
///
/// Asserts `error_count == 1` — this will FAIL on current HEAD (before step-2's fix).
#[test]
fn duplicate_sub_requirement_emits_one_missing_error() {
    let source = r#"
structure def Hole {}

trait A {
    sub hole = Hole()
}

trait B {
    sub hole = Hole()
}

structure def S : A + B {
}
"#;

    let module = compile_source(source);

    let missing_sub_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.message.contains("missing required sub-component")
                && d.message.contains("'hole'")
        })
        .collect();

    assert_eq!(
        missing_sub_errors.len(),
        1,
        "expected exactly 1 'missing required sub-component' error for 'hole' \
         (Sub dedup via seen_sub_names), got {}: {:?}",
        missing_sub_errors.len(),
        missing_sub_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );
}

/// [#21 conflict] Two traits A and B both declare a sub-component named `hole` but
/// with *different* structure types: A requires `sub hole = Hole`, B requires
/// `sub hole = Rectangle`. The dedup-by-name path in `collect_all_requirements`
/// must detect the mismatch and emit a "conflicting trait sub requirements" diagnostic
/// rather than silently dropping one of them.
///
/// This covers the regression gap identified in the amendment review: without
/// `seen_sub_names` tracking (structure_name, trait_name), a structure satisfying A's
/// `sub hole = Hole` would silently pass B's `sub hole = Rectangle` requirement.
#[test]
fn conflicting_sub_requirement_emits_conflict_error() {
    let source = r#"
structure def Hole {}
structure def Rectangle {}

trait A {
    sub hole = Hole()
}

trait B {
    sub hole = Rectangle()
}

structure def S : A + B {
    sub hole = Hole()
}
"#;

    let module = compile_source(source);

    let conflict_errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.code == Some(DiagnosticCode::ConflictingTraitSubRequirements)
                // Keep the 'hole' name-token check: more than one Sub conflict could
                // share the same code, and the member name carries semantic content.
                && d.message.contains("'hole'")
        })
        .collect();

    assert!(
        !conflict_errors.is_empty(),
        "expected at least one DiagnosticCode::ConflictingTraitSubRequirements error \
         mentioning 'hole' (different structure types in A vs B), got no matching errors. \
         All diagnostics: {:?}",
        module
            .diagnostics
            .iter()
            .map(|d| (d.code, &d.message))
            .collect::<Vec<_>>()
    );
}
