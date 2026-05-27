//! Acceptance tests for the surface-syntax → IR lowering of parametric
//! `Field<D, C>` types in `param` positions.
//!
//! Task 3088: add a `Field<D, C>` arm to `resolve_parameterized_builtin_type`
//! (and its `_with_subst` mirror) so stdlib `param` declarations can be
//! statically typed as `Field<…>`, removing the need for `Real` placeholders on
//! field-typed slots like `ElasticResult.displacement`/`.stress`.
//!
//! PRD reference: `docs/prds/v0_3/structural-analysis-fea.md` task #4.
//!
//! Test structure mirrors `parametric_vector_point_resolution_tests.rs` exactly:
//! - `assert_param_type` helper for happy-path fixtures
//! - `structure def Body { param ... }` as the acceptance fixture form (avoids
//!   body-type-checking burden; see that file's header comment for rationale)
//! - positive tests: verify Field<D, C> resolves to Type::Field { domain, codomain }
//! - negative tests: arity mismatches emit Error-severity diagnostics via the
//!   existing fall-through path (no explicit arity arm needed; consistent with
//!   all other builtin parametrics)
//! - subst-path test (step 3): parametric alias `type F<D,C> = Field<D,C>`

mod common;

use common::compile_with_stdlib_helper;
use reify_core::{DimensionVector, Severity, Type};

// ---------------------------------------------------------------------------
// Helper: compile and assert resolved cell type
// ---------------------------------------------------------------------------

/// Compile `source`, assert no Error-severity diagnostics, then find `template`
/// and return the resolved type of cell `member`.
///
/// Collapses the repeated "compile → assert clean → find template →
/// find cell → assert type" scaffolding shared by the happy-path tests.
fn assert_param_type(source: &str, template_name: &str, member: &str, expected: &Type) {
    let module = compile_with_stdlib_helper(source);

    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "source must produce no Error-severity diagnostics; got: {:?}",
        errs
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == template_name)
        .unwrap_or_else(|| panic!("template `{}` not found in compiled module", template_name));

    let cell_type = template
        .value_cells
        .iter()
        .find(|c| c.id.member == member)
        .unwrap_or_else(|| panic!("cell `{}` not found on `{}`", member, template_name))
        .cell_type
        .clone();

    assert_eq!(
        cell_type, *expected,
        "{}::{} — expected {:?}",
        template_name, member, expected
    );
}

// ---------------------------------------------------------------------------
// Positive tests — main resolver path (step 1 / step 2)
// ---------------------------------------------------------------------------

/// Fixture: displacement field form from PRD task #4, line 59.
///
/// `Field<Point3<Length>, Vector3<Length>>` — a spatial field mapping 3D
/// length-domain points to 3D length-codomain vectors, the canonical type for
/// a displacement result from FEA.
const DISPLACEMENT_SOURCE: &str = r#"
structure def Body {
    param disp : Field<Point3<Length>, Vector3<Length>>
}
"#;

/// `Field<Point3<Length>, Vector3<Length>>` must resolve to
/// `Type::Field { domain: Point3(Length), codomain: Vector3(Length) }`.
///
/// Exercises: the new `"Field" if type_args.len() == 2` arm in
/// `resolve_parameterized_builtin_type` resolving both args via
/// `resolve_type_expr_with_aliases` (full-type resolver, not dimension-only).
#[test]
fn field_point3_to_vec3_resolves_to_typed_field() {
    assert_param_type(
        DISPLACEMENT_SOURCE,
        "Body",
        "disp",
        &Type::Field {
            domain: Box::new(Type::point3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            })),
            codomain: Box::new(Type::vec3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            })),
        },
    );
}

/// Fixture: stress field form from PRD task #4, line 60.
///
/// `Field<Point3<Length>, Tensor<2, 3, Pressure>>` — a spatial field mapping
/// 3D length-domain points to rank-2 pressure tensors, the canonical type for
/// a stress result from FEA.
const STRESS_SOURCE: &str = r#"
structure def Body {
    param stress : Field<Point3<Length>, Tensor<2, 3, Pressure>>
}
"#;

/// `Field<Point3<Length>, Tensor<2, 3, Pressure>>` must resolve to
/// `Type::Field { domain: Point3(Length), codomain: Tensor { rank:2, n:3, quantity:Scalar(PRESSURE) } }`.
#[test]
fn field_point3_to_tensor_resolves_to_typed_field() {
    assert_param_type(
        STRESS_SOURCE,
        "Body",
        "stress",
        &Type::Field {
            domain: Box::new(Type::point3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            })),
            codomain: Box::new(Type::tensor(
                2,
                3,
                Type::Scalar {
                    dimension: DimensionVector::PRESSURE,
                },
            )),
        },
    );
}

/// Smoke test for `Field<Real, Real>` — the simplest two-arg form.
///
/// Mirrors today's `Real` placeholder semantics used in `solver_elastic.ri`
/// before the stdlib is tightened in the follow-up task.  Verifying that
/// `Real` round-trips cleanly through the new arm eliminates resolver-shape
/// uncertainty when a future task swaps placeholders for typed field forms.
const REAL_REAL_SOURCE: &str = r#"
structure def Body {
    param scalar_field : Field<Real, Real>
}
"#;

/// `Field<Real, Real>` must resolve to `Type::Field { domain: Real, codomain: Real }`.
#[test]
fn field_real_real_resolves_to_typed_field() {
    assert_param_type(
        REAL_REAL_SOURCE,
        "Body",
        "scalar_field",
        &Type::Field {
            domain: Box::new(Type::Real),
            codomain: Box::new(Type::Real),
        },
    );
}

// ---------------------------------------------------------------------------
// Positive tests — user-defined structure/trait names (task 3101 backfill)
//
// Exercises the `structure_names` / `trait_names` threading added in task 3088
// through the `Field<D, C>` arm of `resolve_parameterized_builtin_type`.
// The existing positive tests above cover only builtin domain/codomain types;
// these two tests verify that user-declared struct and trait names resolve
// correctly in each slot via `resolve_type_with_aliases`.
// ---------------------------------------------------------------------------

/// Fixture: user-declared structure as the domain of a Field type.
///
/// `structure def Foo {}` declares a named struct; `Field<Foo, Real>` places it
/// in the domain slot to exercise `structure_names` threading.
const USER_STRUCT_DOMAIN_SOURCE: &str = r#"
structure def Foo {}
structure def Body {
    param f : Field<Foo, Real>
}
"#;

/// `Field<Foo, Real>` where `Foo` is a user-declared struct must resolve to
/// `Type::Field { domain: StructureRef("Foo"), codomain: Real }`.
///
/// Exercises `structure_names` threading: the domain `Foo` must resolve through
/// `resolve_type_expr_with_aliases` → `resolve_type_with_aliases` →
/// `Type::StructureRef("Foo")` (struct fallback branch in `resolve_type_with_aliases`).
#[test]
fn field_user_struct_domain_resolves_to_typed_field() {
    assert_param_type(
        USER_STRUCT_DOMAIN_SOURCE,
        "Body",
        "f",
        &Type::Field {
            domain: Box::new(Type::StructureRef("Foo".into())),
            codomain: Box::new(Type::Real),
        },
    );
}

/// Fixture: user-declared trait as the codomain of a Field type.
///
/// `trait Bar {}` declares a named trait; `Field<Real, Bar>` places it in the
/// codomain slot to exercise `trait_names` threading.
const USER_TRAIT_CODOMAIN_SOURCE: &str = r#"
trait Bar {}
structure def Body {
    param f : Field<Real, Bar>
}
"#;

/// `Field<Real, Bar>` where `Bar` is a user-declared trait must resolve to
/// `Type::Field { domain: Real, codomain: TraitObject("Bar") }`.
///
/// Exercises `trait_names` threading: the codomain `Bar` must resolve through
/// `resolve_type_expr_with_aliases` → `resolve_type_with_aliases` →
/// `Type::TraitObject("Bar")` (trait fallback branch at type_resolution.rs:663-664).
#[test]
fn field_user_trait_codomain_resolves_to_typed_field() {
    assert_param_type(
        USER_TRAIT_CODOMAIN_SOURCE,
        "Body",
        "f",
        &Type::Field {
            domain: Box::new(Type::Real),
            codomain: Box::new(Type::TraitObject("Bar".into())),
        },
    );
}

// ---------------------------------------------------------------------------
// Negative tests — arity mismatch, regression via fall-through (step 1)
//
// These tests exercise the EXISTING fall-through behaviour (`_ => return None`
// in `resolve_parameterized_builtin_type`) that all sibling builtins inherit.
// No explicit arity-diagnostic arm is added for `Field`; the caller
// (`entity.rs:472`) emits "unresolved type: Field<X>" with span label
// "unknown type name". Both tests pass BEFORE step 2's impl.
// ---------------------------------------------------------------------------

/// `Field<Real>` (one type arg) fails the `type_args.len() == 2` guard, falls
/// through to the catch-all `_ => return None`, and the caller emits an
/// "unresolved type" diagnostic naming "Field".
#[test]
fn field_arity_one_emits_unresolved_type_diagnostic() {
    let source = r#"structure def Body { param x : Field<Real> }"#;
    let module = compile_with_stdlib_helper(source);

    let matching: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.message.contains("unresolved type")
                && d.message.contains("Field")
        })
        .collect();

    assert!(
        !matching.is_empty(),
        "Field<Real> (arity 1) must produce at least one Error-severity diagnostic \
         whose message contains both \"unresolved type\" and \"Field\"; \
         got diagnostics: {:?}",
        module.diagnostics
    );
}

/// `Field<Real, Real, Real>` (three type args) fails the `type_args.len() == 2`
/// guard, falls through to the catch-all `_ => return None`, and the caller
/// emits an "unresolved type" diagnostic naming "Field".
#[test]
fn field_arity_three_emits_unresolved_type_diagnostic() {
    let source = r#"structure def Body { param x : Field<Real, Real, Real> }"#;
    let module = compile_with_stdlib_helper(source);

    let matching: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| {
            d.severity == Severity::Error
                && d.message.contains("unresolved type")
                && d.message.contains("Field")
        })
        .collect();

    assert!(
        !matching.is_empty(),
        "Field<Real, Real, Real> (arity 3) must produce at least one Error-severity \
         diagnostic whose message contains both \"unresolved type\" and \"Field\"; \
         got diagnostics: {:?}",
        module.diagnostics
    );
}

// ---------------------------------------------------------------------------
// Subst-path test — parametric alias (step 3 / step 4)
//
// Routes through `resolve_parameterized_alias` →
// `resolve_type_alias_expr_with_subst` →
// `resolve_parameterized_builtin_type_with_subst("Field", …)`
// — the path step 2 did NOT touch. Must FAIL before step 4's impl.
// ---------------------------------------------------------------------------

/// Fixture: parametric alias `type F<D, C> = Field<D, C>` instantiated as
/// `F<Point3<Length>, Vector3<Length>>`.
const ALIAS_SUBST_SOURCE: &str = r#"
type F<D, C> = Field<D, C>

structure def Alias {
    param disp : F<Point3<Length>, Vector3<Length>>
}
"#;

/// `F<Point3<Length>, Vector3<Length>>` must expand through the parametric
/// alias and resolve to `Type::Field { domain: Point3(Length), codomain:
/// Vector3(Length) }` via the `_with_subst` codepath.
#[test]
fn field_via_parametric_alias_resolves_through_subst_path() {
    assert_param_type(
        ALIAS_SUBST_SOURCE,
        "Alias",
        "disp",
        &Type::Field {
            domain: Box::new(Type::point3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            })),
            codomain: Box::new(Type::vec3(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            })),
        },
    );
}
