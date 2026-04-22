//! Trait member merging compilation tests.
//!
//! Tests for merging members when a structure implements multiple traits:
//! let default deduplication, expression conflict detection, and
//! cross-trait requirement satisfaction by defaults.

use std::sync::atomic::Ordering;

use reify_compiler::*;
use reify_test_support::{CountingSubscriberBuilder, compile_first_template};
use reify_types::*;

/// Step 1a: Two traits each providing `let area : Real = width * height`.
/// Structure implements both — identical let defaults should be merged (dedup).
/// Expect 0 errors and exactly 1 'area' value cell.
#[test]
fn let_defaults_same_name_same_expr_merge() {
    let source = r#"
trait HasArea {
    let area : Real = width * height
}

trait AlsoHasArea {
    let area : Real = width * height
}

structure def S : HasArea + AlsoHasArea {
    param width : Real = 5.0
    param height : Real = 3.0
}
"#;

    // ASSUMPTION: Identical source text ("width * height") produces identical
    // ContentHash values. The dedup in seen_let_hashes relies on this property —
    // same expression text → same hash → treated as duplicate → only 1 cell injected.
    // If the hashing strategy changes (e.g., hashes become source-position-sensitive),
    // this test will catch the regression: it will either produce 2 cells (no dedup)
    // or an unexpected conflict diagnostic (hash mismatch despite same semantics).
    let (template, diagnostics) = compile_first_template(source);

    // No error-severity diagnostics expected.
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Exactly 1 'area' value cell should exist (dedup, not 2).
    let area_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "area")
        .collect();
    assert_eq!(
        area_cells.len(),
        1,
        "expected exactly 1 'area' value cell after merge, got {}",
        area_cells.len()
    );
}

/// Step 2: Trait A has `let x : Length = 5mm`, trait B has `let x : Mass = 1kg`.
/// Structure implements both — different types → 'conflicting' error.
/// Fixed by c6751bf1c: content_hash comparison for let-binding defaults.
#[test]
fn let_defaults_same_name_different_type_error() {
    let source = r#"
trait TraitAlpha {
    let x : Length = 5mm
}

trait TraitBeta {
    let x : Mass = 1kg
}

structure def U : TraitAlpha + TraitBeta {
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected conflict diagnostic for same-name different-type let defaults"
    );

    assert!(
        errors[0].message.contains("conflicting"),
        "error should mention 'conflicting', got: {}",
        errors[0].message
    );
    assert!(
        errors[0].message.contains("TraitAlpha") && errors[0].message.contains("TraitBeta"),
        "error should name both conflicting traits TraitAlpha and TraitBeta, got: {}",
        errors[0].message
    );
}

/// Step 4: Trait A has `let x : Real = width + 1`, trait B has `let x : Real = width * 2`.
/// Same name, same type, different expressions — expect 'conflicting' error.
/// Fixed by c6751bf1c: content_hash comparison catches expression differences.
#[test]
fn let_defaults_same_name_same_type_different_expr_error() {
    let source = r#"
trait TraitGamma {
    let x : Real = width + 1.0
}

trait TraitDelta {
    let x : Real = width * 2.0
}

structure def V : TraitGamma + TraitDelta {
    param width : Real = 5.0
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected conflict diagnostic for same-name same-type different-expression let defaults"
    );

    assert!(
        errors[0].message.contains("conflicting"),
        "error should mention 'conflicting', got: {}",
        errors[0].message
    );
    assert!(
        errors[0].message.contains("TraitGamma") && errors[0].message.contains("TraitDelta"),
        "error should name both conflicting traits TraitGamma and TraitDelta, got: {}",
        errors[0].message
    );
}

/// Step 6: Trait A requires `param x : Length` (no default),
/// trait B provides `param x : Length = 10mm` (default).
/// Structure implements both with empty body — the default from B satisfies A's requirement.
/// Fixed by d545080b3: available_defaults cross-check in check_trait_conformance.
#[test]
fn requirement_satisfied_by_cross_trait_default() {
    let source = r#"
trait NeedsX {
    param x : Length
}

trait ProvidesX {
    param x : Length = 10mm
}

structure def W : NeedsX + ProvidesX {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected 0 errors (default from ProvidesX satisfies NeedsX requirement), got: {:?}",
        errors
    );

    // The default from ProvidesX should be materialized as a Param value cell
    // on W, not silently discarded.
    let x_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "x")
        .collect();
    assert_eq!(
        x_cells.len(),
        1,
        "expected exactly 1 'x' value cell from cross-trait default, got {}",
        x_cells.len()
    );
    assert_eq!(
        x_cells[0].kind,
        ValueCellKind::Param,
        "cross-trait default was a param, should materialize as Param cell"
    );
    assert_eq!(
        x_cells[0].cell_type,
        Type::length(),
        "cross-trait default should preserve the Length type"
    );
    assert!(
        x_cells[0].default_expr.is_some(),
        "cross-trait default should preserve the default expression (10mm)"
    );
}

/// Trait A requires `param x : Length` (no default),
/// trait B provides `param x : Mass = 1kg` (wrong-typed default).
/// Structure implements both with empty body — the default from B has
/// the wrong type, so the cross-trait satisfaction path should emit a
/// type-mismatch error (the `available_defaults` wrong-type branch).
#[test]
fn requirement_type_mismatch_from_cross_trait_default() {
    let source = r#"
trait NeedsX {
    param x : Length
}

trait ProvidesWrongX {
    param x : Mass = 1kg
}

structure def X : NeedsX + ProvidesWrongX {
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected type-mismatch error for wrong-typed cross-trait default"
    );

    assert!(
        errors[0].message.contains("type mismatch") && errors[0].message.contains("'x'"),
        "error should mention 'type mismatch' and member 'x', got: {}",
        errors[0].message
    );
}

/// Trait A requires `param x : Real` (a param-shaped slot, no default),
/// trait B provides `let x : Real = 42.0` (a let default).
/// A param requirement cannot be satisfied by a let default — a let is
/// not externally settable. Expect a "missing required member" error.
///
/// Before the kind-mismatch fix, this silently type-checked because
/// `available_defaults` matched by name+type only.
#[test]
fn param_requirement_not_satisfied_by_let_default() {
    let source = r#"
trait NeedsParamX {
    param x : Real
}

trait ProvidesLetX {
    let x : Real = 42.0
}

structure def Y : NeedsParamX + ProvidesLetX {
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected error: let default should not satisfy param requirement"
    );

    assert!(
        errors[0].message.contains("missing required member") && errors[0].message.contains("'x'"),
        "error should report missing required member 'x', got: {}",
        errors[0].message
    );
}

// NOTE: The symmetric test `let_requirement_not_satisfied_by_param_default` is omitted
// because the reify trait DSL has no syntax for `let x : Type` without a value expression
// (let bindings always require `= expr`). Thus RequirementKind::Let is not currently
// reachable from the parser/compiler for this case. If that syntax is added in the future,
// add the symmetric test here.

/// Trait A requires `param x : Length`, trait B requires `param x : Mass`.
/// The two requirements conflict. The diagnostic must name BOTH traits —
/// not just say "conflicting traits" without identifying which.
#[test]
fn conflicting_param_requirements_names_traits() {
    let source = r#"
trait HasLengthX {
    param x : Length
}

trait HasMassX {
    param x : Mass
}

structure def C : HasLengthX + HasMassX {
    param x : Length = 5mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected conflict diagnostic");

    assert!(
        errors[0].message.contains("conflicting"),
        "error should mention 'conflicting', got: {}",
        errors[0].message
    );
    assert!(
        errors[0].message.contains("HasLengthX") && errors[0].message.contains("HasMassX"),
        "error should name both conflicting traits HasLengthX and HasMassX, got: {}",
        errors[0].message
    );
}

/// Trait A has `param x : Length = 1mm`, trait B has `param x : Mass = 1kg`.
/// Conflicting defaults (different types). Diagnostic must name both traits.
#[test]
fn conflicting_param_defaults_names_traits() {
    let source = r#"
trait LengthDefault {
    param x : Length = 1mm
}

trait MassDefault {
    param x : Mass = 1kg
}

structure def D : LengthDefault + MassDefault {
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected conflict diagnostic");

    assert!(
        errors[0].message.contains("LengthDefault") && errors[0].message.contains("MassDefault"),
        "error should name both conflicting traits LengthDefault and MassDefault, got: {}",
        errors[0].message
    );
}

/// Step 8a: Trait A has `let x : Real = a + 1`, trait B has `let x : Real = a * 2`.
/// Structure implements both and provides its own `let x : Real = a + a`.
/// Structure override resolves the conflict — expect 0 errors.
#[test]
fn let_conflict_resolved_by_structure_override() {
    let source = r#"
trait A {
    let x : Real = a + 1.0
}

trait B {
    let x : Real = a * 2.0
}

structure def R : A + B {
    param a : Real = 5.0
    let x : Real = a + a
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected 0 errors (structure override resolves let conflict), got: {:?}",
        errors
    );

    // Exactly 1 'x' value cell (the structure's own, not any trait default).
    let x_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "x")
        .collect();
    assert_eq!(
        x_cells.len(),
        1,
        "expected exactly 1 'x' value cell, got {}",
        x_cells.len()
    );
}

/// Step 8b: Trait A has `constraint x > 0mm`, trait B has `constraint x < 100mm`.
/// Structure provides `param x : Length = 5mm`. Both constraints should be injected.
#[test]
fn constraints_compose_conjunctively_across_traits() {
    let source = r#"
trait HasLowerBound {
    constraint x > 0mm
}

trait HasUpperBound {
    constraint x < 100mm
}

structure def Q : HasLowerBound + HasUpperBound {
    param x : Length = 5mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // At least 2 constraints injected (one from each trait).
    assert!(
        template.constraints.len() >= 2,
        "expected at least 2 constraints (one from each trait), got {}",
        template.constraints.len()
    );
}

/// Step 10: Comprehensive mixed-merging test.
/// Trait A: `param x : Length`, `let area : Real = x * x`, `constraint x > 0mm`.
/// Trait B: `param x : Length`, `let area : Real = x * x`, `constraint x < 1000mm`.
/// Structure implements A + B with `param x : Length = 5mm`.
/// Expect: 0 errors, exactly 1 'x' value cell, exactly 1 'area' value cell,
/// at least 2 constraints (one from each trait).
#[test]
fn mixed_merging_params_lets_constraints() {
    let source = r#"
trait GeomA {
    param x : Length
    let area = x * x
    constraint x > 0mm
}

trait GeomB {
    param x : Length
    let area = x * x
    constraint x < 1000mm
}

structure def M : GeomA + GeomB {
    param x : Length = 5mm
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // No error-severity diagnostics expected.
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);

    // Exactly 1 'x' value cell (the structure's own).
    let x_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "x")
        .collect();
    assert_eq!(
        x_cells.len(),
        1,
        "expected exactly 1 'x' value cell, got {}",
        x_cells.len()
    );

    // Exactly 1 'area' value cell (dedup of identical let defaults).
    let area_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "area")
        .collect();
    assert_eq!(
        area_cells.len(),
        1,
        "expected exactly 1 'area' value cell, got {}",
        area_cells.len()
    );

    // At least 2 constraints injected (one from each trait — both unlabeled).
    assert!(
        template.constraints.len() >= 2,
        "expected at least 2 constraints (one per trait), got {}",
        template.constraints.len()
    );
}

/// Diamond inheritance: `Base` requires `param x : Real`, `Left : Base` adds nothing,
/// `Right : Base` provides `param x : Real = 1.0`. Structure `S : Left + Right`.
///
/// The `visited` dedup skips `Base` when processing Right's refinement chain (Base
/// was already visited via Left). Right's *own* default must still be collected and
/// injected to satisfy the requirement from Base. This exercises the interaction
/// between the `visited` dedup and cross-trait satisfaction in one test.
///
/// Expect: 0 errors, exactly 1 `x` value cell, Param kind, default_expr present.
#[test]
fn diamond_inheritance_right_provides_default() {
    let source = r#"
trait Base {
    param x : Real
}

trait Left : Base {
}

trait Right : Base {
    param x : Real = 1.0
}

structure def S : Left + Right {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    assert!(
        diagnostics.is_empty(),
        "expected no diagnostics (Right provides default for Base requirement via diamond), got: {:?}",
        diagnostics
    );

    let x_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "x")
        .collect();
    assert_eq!(
        x_cells.len(),
        1,
        // >1 would indicate dedup failure: Base's requirement was processed
        // twice (once via Left, once via Right) and not collapsed by `visited`.
        "expected exactly 1 'x' value cell from diamond inheritance (>1 means dedup failed), got {}",
        x_cells.len()
    );
    assert_eq!(
        x_cells[0].kind,
        ValueCellKind::Param,
        "x should materialize as Param cell (Right's default is a param)"
    );
    assert_eq!(
        x_cells[0].cell_type,
        Type::Real,
        "x should be Real-typed (matching the param declaration in Right)"
    );
    assert!(
        x_cells[0].default_expr.is_some(),
        "x should carry the default expression (1.0) injected from Right"
    );
}

/// Kind-aware dedup: TraitA provides `param x : Real = 1`, TraitB provides
/// `let x : Real = 42`. Structure implements both with no override.
/// The Let default must NOT be silently discarded by the Param default's
/// name-only dedup in `seen_defaults`.
///
/// Expected: 2 value cells for 'x' (one Param from TraitA, one Let from TraitB).
/// Before the fix, `seen_defaults["x"]` is populated by TraitA's Param (type Real),
/// and TraitB's Let also has sentinel type Real — so the name-only dedup treats them
/// as duplicates and discards the Let, producing only 1 cell.
#[test]
fn let_default_not_discarded_by_param_default_same_name() {
    let source = r#"
trait ProvidesParamX {
    param x : Real = 1.0
}

trait ProvidesLetX {
    let x : Real = 42.0
}

structure def S : ProvidesParamX + ProvidesLetX {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected errors when collecting cross-kind defaults: {:?}",
        errors
    );

    let x_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "x")
        .collect();
    assert_eq!(
        x_cells.len(),
        2,
        "expected 2 'x' value cells (one Param, one Let) but got {}; \
         the Let default was silently discarded by the name-only seen_defaults dedup",
        x_cells.len()
    );

    let param_cells: Vec<_> = x_cells
        .iter()
        .filter(|vc| vc.kind == ValueCellKind::Param)
        .collect();
    assert_eq!(param_cells.len(), 1, "expected exactly 1 Param 'x' cell");

    let let_cells: Vec<_> = x_cells
        .iter()
        .filter(|vc| vc.kind == ValueCellKind::Let)
        .collect();
    assert_eq!(
        let_cells.len(),
        1,
        "expected exactly 1 Let 'x' cell (let_default_not_discarded)"
    );
}

/// Step 1b: Two traits each requiring `param x : Length`.
/// Structure provides `param x : Length = 5mm` — requirement dedup baseline.
/// Expect 0 errors (existing behavior).
#[test]
fn param_requirements_same_name_same_type_merge() {
    let source = r#"
trait NeedsX {
    param x : Length
}

trait AlsoNeedsX {
    param x : Length
}

structure def T : NeedsX + AlsoNeedsX {
    param x : Length = 5mm
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    // No error-severity diagnostics expected — same-type requirement dedup.
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "unexpected errors: {:?}", errors);
}

// ─── Kind-aware dedup guard tests (task 1287) ─────────────────────────────────

/// TraitA requires `param x : Real` (no default), TraitB provides `param x : Real = 1`,
/// TraitC provides `let x : Real = 42`. Structure implements all three with no override.
/// The Param requirement must still be satisfied by the Param default even though a
/// Let default for the same name is also present.
///
/// Guards against the `available_defaults` lookup regressing to name-only keying: if the
/// key were just the name, the HashMap might store only the last-written kind (collision),
/// causing the Param lookup to fail or return a Let type.
#[test]
fn param_requirement_still_satisfied_with_let_default_present() {
    let source = r#"
trait NeedsParam {
    param x : Real
}

trait ProvidesParamDefault {
    param x : Real = 1.0
}

trait ProvidesLetDefault {
    let x : Real = 42.0
}

structure def S : NeedsParam + ProvidesParamDefault + ProvidesLetDefault {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors: Param requirement must be satisfied by Param default \
         even when a Let default for the same name is also present, got: {:?}",
        errors
    );

    // Both the Param default and the Let default should be materialized.
    let x_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "x")
        .collect();
    assert_eq!(
        x_cells.len(),
        2,
        "expected 2 'x' cells (one Param from ProvidesParamDefault, one Let from \
         ProvidesLetDefault), got {}",
        x_cells.len()
    );
}

/// TraitA provides `param x : Real = 1`, TraitB provides `let x : Real = 42`.
/// Structure provides its own `param x : Real = 5` — the structure member overrides.
///
/// Verifies: no errors, exactly 1 'x' value cell (the structure's own), kind Param.
/// The structure override path must still work when both-kind defaults survive collection.
#[test]
fn cross_kind_defaults_structure_override_suppresses_both() {
    let source = r#"
trait ProvidesParamX {
    param x : Real = 1.0
}

trait ProvidesLetX {
    let x : Real = 42.0
}

structure def S : ProvidesParamX + ProvidesLetX {
    param x : Real = 5.0
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors when structure overrides cross-kind defaults, got: {:?}",
        errors
    );

    // Exactly 1 'x' cell — the structure's own (both trait defaults suppressed).
    let x_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "x")
        .collect();
    assert_eq!(
        x_cells.len(),
        1,
        "expected exactly 1 'x' cell (structure override, no trait defaults injected), \
         got {}",
        x_cells.len()
    );
    assert_eq!(
        x_cells[0].kind,
        ValueCellKind::Param,
        "the surviving cell should be the structure's Param, got {:?}",
        x_cells[0].kind
    );
}

/// TraitA provides `param x : Length = 1mm`, TraitB provides `let x : Real = 42`.
/// Structure implements both with no override.
///
/// Before the fix, the name-only `seen_defaults` compared Length (Param cell_type)
/// against Real (Let sentinel type) and emitted a false "conflicting trait defaults"
/// error. After the fix, the two defaults occupy separate `(name, kind)` slots and
/// do not interact.
#[test]
fn cross_kind_defaults_different_types_no_false_conflict() {
    let source = r#"
trait ProvidesLengthParam {
    param x : Length = 1mm
}

trait ProvidesRealLet {
    let x : Real = 42.0
}

structure def S : ProvidesLengthParam + ProvidesRealLet {
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let conflict_errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error && d.message.contains("conflicting"))
        .collect();
    assert!(
        conflict_errors.is_empty(),
        "expected no false conflict error for cross-kind different-type defaults, \
         got: {:?}",
        conflict_errors
    );
}

/// TraitA provides `param x : Real = 1`, TraitB provides `let x : Real = 42`.
/// Structure provides its own `let x : Real = 5.0` — the structure's let member overrides.
///
/// Complements `cross_kind_defaults_structure_override_suppresses_both` (which uses a
/// structure `param` override). This variant verifies that a structure `let` override also
/// suppresses both the Param and Let trait defaults, and that exactly 1 'x' cell survives
/// with kind Let.
#[test]
fn cross_kind_defaults_let_structure_override_suppresses_both() {
    let source = r#"
trait ProvidesParamX {
    param x : Real = 1.0
}

trait ProvidesLetX {
    let x : Real = 42.0
}

structure def S : ProvidesParamX + ProvidesLetX {
    let x : Real = 5.0
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors when structure let-overrides cross-kind defaults, got: {:?}",
        errors
    );

    // Exactly 1 'x' cell — the structure's own Let (both trait defaults suppressed).
    let x_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "x")
        .collect();
    assert_eq!(
        x_cells.len(),
        1,
        "expected exactly 1 'x' cell (structure let-override, no trait defaults injected), \
         got {}",
        x_cells.len()
    );
    assert_eq!(
        x_cells[0].kind,
        ValueCellKind::Let,
        "the surviving cell should be the structure's Let, got {:?}",
        x_cells[0].kind
    );
}

/// Two traits each providing `param x : Real = 1.0`. Structure implements both.
/// Same-kind, same-type dedup must still produce exactly 1 'x' value cell.
///
/// Guards against the kind-aware keying accidentally breaking the happy-path dedup
/// for same-kind defaults (regression: if we forgot the `continue` on a seen key, we
/// would inject two cells).
#[test]
fn same_kind_same_type_param_dedup_unchanged() {
    let source = r#"
trait FirstParam {
    param x : Real = 1.0
}

trait SecondParam {
    param x : Real = 1.0
}

structure def S : FirstParam + SecondParam {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected errors for same-kind same-type dedup: {:?}",
        errors
    );

    let x_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "x")
        .collect();
    assert_eq!(
        x_cells.len(),
        1,
        "expected exactly 1 'x' cell after same-kind dedup, got {}; \
         kind-aware keying must not break same-kind dedup",
        x_cells.len()
    );
    assert_eq!(
        x_cells[0].kind,
        ValueCellKind::Param,
        "the surviving cell should be Param"
    );
}

/// Cross-kind pre-registration must not overwrite an already-registered name.
///
/// Trait A provides `param x : Length = 10mm` and `constraint x - 1mm > 0mm`.
/// Trait B provides `let x = 5.0` (type Real).
/// Structure implements both with no override.
///
/// Without the guard, the pre-registration loop registers x as Length (from Param A),
/// then overwrites it with Real (from Let B). The subtraction `x - 1mm` then sees
/// x as Real and emits an 'incompatible types in subtraction: Real vs Length' error.
///
/// With the guard (`!scope.names.contains_key(name)`), the second registration is
/// skipped, x stays Length, and the constraint compiles cleanly.
///
/// Note: comparison operators (>, <, etc.) have no type checking in the compiler
/// (only Add/Sub do). The subtraction in `x - 1mm > 0mm` is what triggers the
/// observable type error when x is overwritten from Length to Real.
///
/// Cell shape: exactly ONE Param cell for `x` (from TraitA).  The two-pass
/// amendment (task 1907) means Pass 2 detects that the Param already claimed
/// the scope slot and records TraitB's `let x` in `pass2_skipped`, so the
/// injection loop skips the Let cell — preventing duplicate (entity, member)
/// pairs for `x`.
#[test]
fn cross_kind_pre_registration_preserves_first_type() {
    let source = r#"
trait TraitA {
    param x : Length = 10mm
    constraint x - 1mm > 0mm
}

trait TraitB {
    let x = 5.0
}

structure def S : TraitA + TraitB {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors: the constraint `x - 1mm > 0mm` should compile cleanly \
         because x is registered as Length (not overwritten to Real by the Let default). \
         Got errors: {:?}",
        errors
    );

    // At least 1 constraint injected (from TraitA).
    assert!(
        !template.constraints.is_empty(),
        "expected at least 1 constraint injected from TraitA, got {}",
        template.constraints.len()
    );

    // Exactly 1 'x' value cell: the Param from TraitA.  TraitB's `let x = 5.0`
    // is suppressed by the two-pass amendment: Pass 2 finds the scope slot
    // already occupied by TraitA's Param and records `x` in `pass2_skipped`,
    // so the injection loop skips Let-cell emission for `x`.
    let x_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "x")
        .collect();
    assert_eq!(
        x_cells.len(),
        1,
        "expected 1 'x' value cell (Param from TraitA only; Let from TraitB \
         suppressed by pass2_skipped), got {}",
        x_cells.len()
    );
    assert_eq!(
        x_cells[0].kind,
        ValueCellKind::Param,
        "the single `x` cell must be Param (from TraitA)"
    );
    assert_eq!(
        x_cells[0].cell_type,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "the `x` Param cell must carry type Length"
    );
}

/// Constraint default coexists with a param default for the same member name.
///
/// Trait A provides `param x : Real = 1.0`.
/// Trait B provides `constraint x > 0` (unlabeled — `name: None`).
/// Structure implements both with no override.
///
/// Unlabeled constraints have `name: None` and are pushed unconditionally in
/// `collect_all_requirements` (conformance.rs:469-471), bypassing the composite-key
/// dedup that applies to named Param/Let defaults. They never conflict with a
/// same-named param, so this test should pass immediately.
///
/// This test exercises the Constraint + Param coexistence path through the full
/// compilation pipeline: pre-registration → expression compilation → constraint
/// injection.
#[test]
fn constraint_default_coexists_with_param_default() {
    let source = r#"
trait HasParam {
    param x : Real = 1.0
}

trait HasConstraint {
    constraint x > 0
}

structure def S : HasParam + HasConstraint {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors when a constraint default coexists with a param default: {:?}",
        errors
    );

    // Exactly 1 'x' value cell (the Param from HasParam).
    let x_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "x")
        .collect();
    assert_eq!(
        x_cells.len(),
        1,
        "expected exactly 1 'x' value cell (Param from HasParam), got {}",
        x_cells.len()
    );
    assert_eq!(
        x_cells[0].kind,
        ValueCellKind::Param,
        "the 'x' cell should be a Param, got {:?}",
        x_cells[0].kind
    );
    assert_eq!(
        x_cells[0].cell_type,
        Type::Real,
        "the Param 'x' should have type Real, got {:?}",
        x_cells[0].cell_type
    );
    assert!(
        x_cells[0].default_expr.is_some(),
        "the Param 'x' should have a default expression (= 1.0)"
    );

    // At least 1 constraint injected (the `x > 0` from HasConstraint).
    assert!(
        !template.constraints.is_empty(),
        "expected at least 1 constraint injected from HasConstraint, got {}",
        template.constraints.len()
    );
}

/// Documents that the two-pass pre-register split resolves cross-kind
/// order-sensitivity when one trait provides an annotated Param and the other
/// provides an unannotated Let for the same name.
///
/// Scenario (bound-list order intentionally reversed to exercise the old failure
/// mode):
/// - TraitA provides `param x : Length = 10mm` + `constraint x - 1mm > 0mm`
/// - TraitB provides `let x = 5.0` (unannotated, type Real)
/// - Structure lists TraitB first: `S : TraitB + TraitA`
///
/// Under the old single-pass loop, TraitB's unannotated Let was compiled inline
/// (winning the scope registration for `x : Real`) before TraitA's annotated Param
/// could be visited — the constraint `x - 1mm > 0mm` then saw `x : Real` and
/// produced a `Real - Length` type error.
///
/// Under the two-pass split in conformance.rs:
///   Pass 1 — registers every annotated default (Param + Let-with-annotation)
///             regardless of bound-list order. TraitA's `param x : Length` wins.
///   Pass 2 — compiles each unannotated Let against the fully-annotated scope.
///             TraitB's `let x = 5.0` calls `scope.register_if_absent` and finds
///             `x` already occupied (debug-logged, no error). The constraint sees
///             `x : Length` → clean compile.
///
/// Bound-list order is preserved unchanged (`TraitB + TraitA`) so this test
/// continues to exercise the formerly-sensitive scenario as a regression guard.
#[test]
fn cross_kind_pre_registration_order_resolved_by_two_pass() {
    let source = r#"
trait TraitA {
    param x : Length = 10mm
    constraint x - 1mm > 0mm
}

trait TraitB {
    let x = 5.0
}

structure def S : TraitB + TraitA {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    // Pass 1 registers TraitA's annotated `param x : Length` before any
    // unannotated-let expression is compiled.  Pass 2 then sees `x` as occupied
    // and skips TraitB's `let x = 5.0`.  The constraint `x - 1mm > 0mm`
    // operates on `x : Length`, compiles cleanly, and no type error is emitted.
    assert!(
        errors.is_empty(),
        "with the two-pass pre-register split, `S : TraitB + TraitA` must compile \
         cleanly — Pass 1 registers the annotated `param x : Length` regardless of \
         bound-list order, so the constraint sees `x : Length`, not Real; got: {:?}",
        errors
    );

    // Pin the cell shape: exactly ONE cell for `x` (the Param from TraitA),
    // with type Length.  Two cells would reveal double-injection — the old
    // pre-amendment Pass 2 inserted TraitB's `let x` compiled expression into
    // `inferred_let_exprs` *before* calling `register_if_absent`, so the
    // injection loop found an entry and emitted a spurious Let cell alongside
    // the Param cell, producing duplicate (entity, member) pairs downstream.
    let x_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "x")
        .collect();
    assert_eq!(
        x_cells.len(),
        1,
        "exactly one cell expected for `x` (Param from TraitA); \
         two cells indicate double-injection (Let cell from TraitB must not \
         be emitted when Pass 1 already claimed the scope slot): {:?}",
        template.value_cells
    );
    assert_eq!(
        x_cells[0].kind,
        ValueCellKind::Param,
        "the single `x` cell must be Param (from TraitA), not Let"
    );
    assert_eq!(
        x_cells[0].cell_type,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "the `x` Param cell must carry type Length \
         (from TraitA's `param x : Length = 10mm`)"
    );
}

/// Verifies that a single trait providing BOTH `param x` and `constraint x > 0` compiles
/// cleanly when a structure uses that trait with no override.
///
/// Unlike `constraint_default_coexists_with_param_default` (which uses two separate traits),
/// this test exercises the intra-trait pre-registration path: both defaults come from the
/// same trait's defaults list, so the pre-registration loop must register `x` before the
/// constraint expression `x > 0` is compiled. If `x` were not pre-registered, the constraint
/// expression compilation would fail with an "unresolved name" error.
#[test]
fn constraint_and_param_coexist_same_trait_same_name() {
    let source = r#"
trait HasParamAndConstraint {
    param x : Real = 1.0
    constraint x > 0
}

structure def S : HasParamAndConstraint {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors when a single trait provides both param x and constraint x > 0: {:?}",
        errors
    );

    // Exactly 1 'x' value cell (the Param from HasParamAndConstraint).
    let x_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "x")
        .collect();
    assert_eq!(
        x_cells.len(),
        1,
        "expected exactly 1 'x' value cell (Param from HasParamAndConstraint), got {}",
        x_cells.len()
    );
    assert_eq!(
        x_cells[0].kind,
        ValueCellKind::Param,
        "the 'x' cell should be a Param, got {:?}",
        x_cells[0].kind
    );
    assert_eq!(
        x_cells[0].cell_type,
        Type::Real,
        "the Param 'x' should have type Real, got {:?}",
        x_cells[0].cell_type
    );
    assert!(
        x_cells[0].default_expr.is_some(),
        "the Param 'x' should have a default expression (= 1.0)"
    );

    // Exactly 1 constraint injected (the `x > 0` from HasParamAndConstraint).
    assert_eq!(
        template.constraints.len(),
        1,
        "expected exactly 1 constraint injected from HasParamAndConstraint, got {}",
        template.constraints.len()
    );

    // Structural assertion: the constraint expression must be `x > 0`.
    match &template.constraints[0].expr.kind {
        CompiledExprKind::BinOp { op, left, right } => {
            assert_eq!(*op, BinOp::Gt, "constraint operator should be Gt (>)");
            assert!(
                matches!(&left.kind, CompiledExprKind::ValueRef(id) if id.member == "x"),
                "left operand should be ValueRef with member 'x', got {:?}",
                left.kind
            );
            let right_is_zero = match &right.kind {
                CompiledExprKind::Literal(Value::Int(v)) => *v == 0,
                CompiledExprKind::Literal(Value::Real(v)) => v.abs() < 1e-9,
                _ => false,
            };
            assert!(
                right_is_zero,
                "right operand should be Literal(0), got {:?}",
                right.kind
            );
        }
        other => panic!("expected BinOp for constraint x > 0, got {:?}", other),
    }
}

/// Three traits each define `let x : Real = <different_expr>`.
/// Structure implements all three — three-way let conflict should produce
/// exactly ONE error diagnostic (not N-1 = 2).
///
/// Currently (before step-2 fix) each subsequent trait collision emits a
/// fresh diagnostic against the first-seen trait, producing 2 errors.
/// After adding `seen_let_conflict_names`, only the first collision is emitted.
#[test]
fn three_way_let_conflict_emits_single_diagnostic() {
    let source = r#"
trait TraitX {
    let x : Real = 1.0
}

trait TraitY {
    let x : Real = 2.0
}

trait TraitZ {
    let x : Real = 3.0
}

structure def S : TraitX + TraitY + TraitZ {
}
"#;

    let (_, diagnostics) = compile_first_template(source);

    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    assert!(
        !errors.is_empty(),
        "expected at least 1 conflict diagnostic for three-way let conflict"
    );
    assert_eq!(
        errors.len(),
        1,
        "expected exactly 1 conflict diagnostic (not N-1=2) for three-way let conflict, \
         got {}: {:?}",
        errors.len(),
        errors
    );
    assert!(
        errors[0].message.contains("conflicting"),
        "error should mention 'conflicting', got: {}",
        errors[0].message
    );
    assert!(
        errors[0].message.contains("TraitX") && errors[0].message.contains("TraitY"),
        "error should name both conflicting traits TraitX and TraitY, got: {}",
        errors[0].message
    );
    assert!(
        !errors[0].message.contains("TraitZ"),
        "error should report only the first collision pair (TraitX/TraitY) and suppress TraitZ; \
         got: {}",
        errors[0].message
    );
}

/// Verifies that a DEBUG-level tracing event is emitted from the `reify_compiler` target
/// when two traits supply the same-named default of **different kinds** (`let x` from one
/// trait and `param x` from another), causing the second `register_if_absent` call to
/// return `false` (the Occupied branch — first-seen type wins).
///
/// Same-kind duplicates (e.g., two `let x` from two traits) are deduplicated by
/// `collect_all_requirements` before the pre-registration loop runs and therefore never
/// exercise the `was_new = false` path. A cross-kind collision (let + param with the same
/// name) causes BOTH defaults to appear in `ctx.defaults` because they use separate
/// dedup maps (`seen_let_hashes` for Let, `seen_defaults` for Param). The pre-registration
/// loop then visits the let first (was_new = true) and the param second (was_new = false),
/// which is exactly the path guarded by the new `tracing::debug!` emission.
///
/// This is an observability regression guard: the `tracing::debug!` emission in
/// `conformance.rs` must fire when `register_if_absent` returns `false` (the Occupied
/// branch). Any future removal of that emission will be caught here.
#[test]
fn trait_merge_name_conflict_emits_debug_event() {
    // TraitA contributes `let x` (Let kind); TraitB contributes `param x` (Param kind).
    // Both are pushed into ctx.defaults by collect_all_requirements (different dedup maps).
    // The pre-registration loop registers x from TraitA (was_new=true) then tries to
    // register x from TraitB (was_new=false) → debug! fires.
    let source = r#"
trait TraitA {
    let x : Real = 1.0
}

trait TraitB {
    param x : Real = 2.0
}

structure def S : TraitA + TraitB {
}
"#;

    let (subscriber, counters) = CountingSubscriberBuilder::new()
        .target_prefix("reify_compiler::conformance")
        .count_level(tracing::Level::DEBUG)
        .build();

    let debug_count = std::sync::Arc::clone(
        counters
            .get(&tracing::Level::DEBUG)
            .expect("DEBUG counter registered"),
    );

    // NOTE: with_default installs a thread-local subscriber; DEBUG events emitted
    // on other threads spawned by the compile path would be missed. This test
    // relies on compile_first_template (defined in this same test module) running
    // the compile pipeline on the calling thread — if that ever dispatches to
    // rayon/spawned tasks, switch to set_global_default in a std::sync::Once.
    // Run the compilation under the scoped subscriber so we capture any DEBUG
    // events from reify_compiler::conformance targets.
    let _ = tracing::subscriber::with_default(subscriber, || compile_first_template(source));

    let debug = debug_count.load(Ordering::Relaxed);
    // Assert exactly 1 event: the fixture has one cross-kind collision (let x vs param x),
    // which fires the tracing::debug! path exactly once. Equality also guards against
    // accidental over-emission if additional debug sites are added to the same path.
    assert_eq!(
        debug, 1,
        "expected exactly 1 DEBUG event from reify_compiler::conformance target when two \
         traits supply the same-named default (second register_if_absent returns false), got {}",
        debug
    );
}

/// Forward-compatibility contract for task 1951: once `RequirementKind::Let` is
/// parser-reachable, this test will start catching regressions against the Option B
/// fix in `conformance.rs`.
///
/// ## Why this is a forward-compat placeholder (not a behavioral regression test)
///
/// The full 3-trait scenario from task 1951 requires trait X with `let x : Length`
/// (no `= expr`), which emits `RequirementKind::Let` for `x`.  That syntax is NOT
/// reachable from the reify parser today: `LetDecl.value` is `Expr`, not
/// `Option<Expr>`, so `lower_let` always requires a value expression.  The parser
/// pins this behavior in `let_type_disambiguation_tests.rs:470-497`, and the comment
/// at `trait_merge_tests.rs:280-284` acknowledges that `RequirementKind::Let` is
/// unreachable from source.  See also esc-1951-6.
///
/// Consequence: in the currently reachable universe, the phantom `(x, Let)` entry in
/// `available_defaults` is dormant — no `RequirementKind::Let` lookup ever fires
/// against it — so the phantom-signature guard (assertion 3) passes both before and
/// after the Option B fix.  This test therefore **cannot distinguish** the patched
/// binary from the unpatched one; all three assertions pass unconditionally today.
///
/// The behavioral unit test that **does** distinguish them (and fails on the unpatched
/// code) lives in `conformance.rs::tests::option_b_fix_blocks_phantom_let_entry_for_pass2_skipped_name`.
/// It hand-builds a `RequirementKind::Let` requirement and verifies the phantom
/// diagnostic is absent after the fix.
///
/// ## What this test exercises
///
/// The reachable Y+Z subset suffices to populate `pass2_skipped["x"]`: Pass 1 claims
/// the scope slot with Y's annotated Param; Pass 2 encounters Z's unannotated Let and
/// records the name in `pass2_skipped` instead of caching the expression.  The
/// assertions lock in the correct cell-shape (single Param cell, no spurious Let cell)
/// and the phantom-signature wording contract, so they will automatically detect a
/// regression if a parser extension later exposes `let x : Type` as a requirement.
///
/// ## Assertions
/// 1. Zero error diagnostics — structure compiles cleanly.
/// 2. Exactly 1 `x` value cell, kind `Param`, type `Length` — no spurious Let cell.
/// 3. No diagnostic combines the phrases "available default" and "Real" for member
///    `x` — the phantom-signature wording guard (dormant today; activates when
///    `RequirementKind::Let` becomes parser-reachable).
#[test]
fn phantom_let_advertisement_contract_for_future_parser_extension() {
    // trait Y provides an annotated Param: claims the scope slot in Pass 1.
    // trait Z provides an unannotated Let: Pass 2 sees the slot occupied,
    // records "x" in pass2_skipped, and skips caching the expression.
    let source = r#"
trait TraitY {
    param x : Length = 10mm
}

trait TraitZ {
    let x = 5.0
}

structure def S : TraitY + TraitZ {
}
"#;

    let (template, diagnostics) = compile_first_template(source);

    // --- Assertion 1: zero error diagnostics ---
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no errors: structure S : TraitY + TraitZ should compile cleanly \
         (TraitY's Param wins the slot, TraitZ's Let is suppressed via pass2_skipped). \
         Got errors: {:?}",
        errors
    );

    // --- Assertion 2: exactly 1 'x' cell, Param, Length ---
    let x_cells: Vec<_> = template
        .value_cells
        .iter()
        .filter(|vc| vc.id.member == "x")
        .collect();
    assert_eq!(
        x_cells.len(),
        1,
        "expected exactly 1 'x' value cell (Param from TraitY; Let from TraitZ \
         suppressed by pass2_skipped), got {}",
        x_cells.len()
    );
    assert_eq!(
        x_cells[0].kind,
        ValueCellKind::Param,
        "the single 'x' cell must be Param (from TraitY)"
    );
    assert_eq!(
        x_cells[0].cell_type,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
        "the 'x' Param cell must carry type Length"
    );

    // --- Assertion 3: phantom-signature wording guard ---
    // No diagnostic should combine "available default" + "Real" for member 'x'.
    // In the reachable universe this passes unconditionally (no RequirementKind::Let
    // lookup fires against the phantom entry). The assertion locks in the contract
    // for when parser support for bare `let x : Type` requirements is added later.
    let phantom_diags: Vec<_> = diagnostics
        .iter()
        .filter(|d| {
            d.message.contains("available default")
                && d.message.contains("Real")
                && d.message.contains('x')
        })
        .collect();
    assert!(
        phantom_diags.is_empty(),
        "phantom `(name, Let) -> Type::Real` advertisement must not emit a \
         spurious type-mismatch diagnostic referencing 'x'. \
         Got phantom-signature diagnostics: {:?}",
        phantom_diags
    );
}
