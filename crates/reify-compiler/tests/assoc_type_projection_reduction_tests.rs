//! Integration tests for task 4604 δ: assoc-binding-references-type-param +
//! Type::Projection reduction + anti-cascade.
//!
//! PRD: docs/prds/type-args-and-assoc-type-projection.md §4.3, §4.4, §9.
//!
//! Tests in source order:
//!   - step-1 RED: build-side binding stores symbolic Projection
//!   - step-3 RED: read-side worked chain resolves concrete types
//!   - step-7 RED: anti-cascade — declared-but-unbound poisons to Error without cascade

use reify_core::{diagnostics::DiagnosticCode, Type};
use reify_test_support::{compile_source, errors_only};

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn any_diag_has_code(diags: &[&reify_core::Diagnostic], code: DiagnosticCode) -> bool {
    diags.iter().any(|d| d.code == Some(code))
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 1 RED: build-side binding stores a symbolic Projection
// ═══════════════════════════════════════════════════════════════════════════════

/// A generic structure whose assoc-type binding `type MotionValue = P::MotionValue`
/// references its own type parameter `P`.
///
/// After the fix (step-2), the build-side `collect_structure_assoc_type_bindings`
/// resolves `P` to `Type::TypeParam("P")` (since `P` is in scope as a type param)
/// and stores `Type::Projection { base: TypeParam("P"), member: "MotionValue" }` in
/// `template.assoc_types` — unreduced, because we don't have concrete args yet.
///
/// Fails today (step-1 RED): checker.rs hardcodes `empty_params` (line 912) and the
/// registry-less resolver returns None for the `QualifiedAssoc` RHS, yielding
/// `Type::Error` + "unresolved type in associated type binding" diagnostic.
#[test]
fn build_side_binding_stores_symbolic_projection() {
    let source = r#"
trait DrivingJoint {}
trait HasMotion { type MotionValue }
structure def Coupling<P: DrivingJoint + HasMotion> : HasMotion {
    type MotionValue = P::MotionValue
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        errors.is_empty(),
        "Coupling's assoc-type binding `P::MotionValue` must compile without errors; \
         got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Coupling")
        .expect("Coupling template should be compiled");

    let entry = template
        .assoc_types
        .iter()
        .find(|a| a.type_name == "MotionValue")
        .unwrap_or_else(|| {
            panic!(
                "Coupling should carry an assoc_types entry for MotionValue; \
                 assoc_types = {:?}",
                template.assoc_types
            )
        });

    assert_eq!(
        entry.resolved,
        Type::projection(Type::TypeParam("P".into()), "MotionValue"),
        "build-side binding must store Projection{{TypeParam(P), MotionValue}} \
         (unreduced / symbolic); got: {:?}",
        entry.resolved
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 3 RED: read-side worked chain — full projection reduction
// ═══════════════════════════════════════════════════════════════════════════════

/// Full end-to-end worked chain:
///
/// `Coupling<Prismatic>::MotionValue`
///   → Projection{Applied{"Coupling",[StructureRef("Prismatic")]},"MotionValue"}
///   → Coupling's binding Projection{TypeParam("P"),"MotionValue"} with P:=Prismatic
///   → Projection{StructureRef("Prismatic"),"MotionValue"}
///   → Prismatic's MotionValue binding = Type::length()
///
/// `Coupling<Revolute>::MotionValue` → Type::angle()
///
/// Fails today (step-3 RED): `resolve_qualified_assoc_type` rejects the applied base
/// ("must not have type arguments", type_resolution.rs line 826-835).
#[test]
fn applied_base_projection_reduces_to_concrete_type() {
    let source = r#"
trait DrivingJoint {}
trait HasMotion { type MotionValue }
structure def Prismatic : DrivingJoint + HasMotion {
    type MotionValue = Length
}
structure def Revolute : DrivingJoint + HasMotion {
    type MotionValue = Angle
}
structure def Coupling<P: DrivingJoint + HasMotion> : HasMotion {
    type MotionValue = P::MotionValue
}
structure def UseCoupling {
    param a : Coupling<Prismatic>::MotionValue
    param b : Coupling<Revolute>::MotionValue
}
"#;
    let module = compile_source(source);
    let errors = errors_only(&module);

    assert!(
        errors.is_empty(),
        "full worked-chain must compile without errors; got: {:?}",
        errors
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "UseCoupling")
        .expect("UseCoupling template should be compiled");

    let cell_type = |member: &str| {
        template
            .value_cells
            .iter()
            .find(|vc| vc.id.member == member)
            .unwrap_or_else(|| panic!("value cell `{member}` should exist"))
            .cell_type
            .clone()
    };

    assert_eq!(
        cell_type("a"),
        Type::length(),
        "Coupling<Prismatic>::MotionValue must reduce to Type::length(); got: {:?}",
        cell_type("a")
    );

    assert_eq!(
        cell_type("b"),
        Type::angle(),
        "Coupling<Revolute>::MotionValue must reduce to Type::angle(); got: {:?}",
        cell_type("b")
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// Step 7 RED: anti-cascade — declared-but-unbound poisons without second diagnostic
// ═══════════════════════════════════════════════════════════════════════════════

/// A generic structure that conforms to `HasMotion` but does NOT bind `MotionValue`.
/// The consumer (`UseCoupling`) references `Coupling<Prismatic>::MotionValue`.
///
/// Expected behaviour after the fix:
///   - `TraitAssocTypeNotBound` is emitted on `Coupling` (the root cause).
///   - The consumer emits NO second `UnresolvedType` or `AmbiguousAssocType`
///     (anti-cascade).
///   - `UseCoupling`'s `x`.cell_type == `Type::Error` (poison sentinel, not
///     `Type::dimensionless_scalar()`).
///
/// Fails today if `normalize_type` emits a duplicate diagnostic or poisons to a
/// concrete type when the member is declared-but-unbound.
#[test]
fn applied_base_projection_unbound_poisons_without_cascade() {
    let source = r#"
trait DrivingJoint {}
trait HasMotion { type MotionValue }
structure def Prismatic : DrivingJoint + HasMotion {
    type MotionValue = Length
}
structure def Coupling<P: DrivingJoint + HasMotion> : HasMotion {
    // deliberately does NOT bind MotionValue
}
structure def UseCoupling {
    param x : Coupling<Prismatic>::MotionValue
}
"#;
    let module = compile_source(source);
    let all_errors = errors_only(&module);

    // Root-cause: Coupling must not be unbound.
    assert!(
        any_diag_has_code(&all_errors, DiagnosticCode::TraitAssocTypeNotBound),
        "expected TraitAssocTypeNotBound on Coupling for MotionValue; got: {:?}",
        all_errors
    );

    // Anti-cascade: the consumer must NOT emit a second UnresolvedType or AmbiguousAssocType.
    assert!(
        !any_diag_has_code(&all_errors, DiagnosticCode::AmbiguousAssocType),
        "consumer must NOT emit AmbiguousAssocType (anti-cascade); got: {:?}",
        all_errors
    );

    // Consumer's cell_type must be poisoned to Type::Error (not dimensionless_scalar).
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "UseCoupling")
        .expect("UseCoupling template should be compiled");

    let x_cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "x")
        .expect("value cell 'x' should exist");

    assert_eq!(
        x_cell.cell_type,
        Type::Error,
        "x's cell_type must be poisoned to Type::Error (not dimensionless_scalar); \
         got: {:?}",
        x_cell.cell_type
    );
}
