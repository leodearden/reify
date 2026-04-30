//! Acceptance test for the surface-syntax → IR lowering of parametric
//! `Vector3<Q>` / `Point3<Q>` types.
//!
//! The architect plan for task 2746 specifies acceptance fixtures wrapped in
//! `structure def Body { param ... }` rather than `fn f() -> Vector3<Force> { undef }`.
//! `fn` declarations carry the additional burden of validating the body against
//! the declared return type — which would require an actual vector literal at the
//! language level (out of scope: 2746 is type-system only). A `structure def`
//! with `param`s exercises the same surface→IR resolution path with no
//! body-type-checking distraction.

mod common;

use common::compile_with_stdlib_helper;
use reify_types::{DimensionVector, Severity, Type};

/// Compile `source`, assert no Error-severity diagnostics, then find `template`
/// and return the resolved type of cell `member`.
///
/// This helper collapses the repeated "compile → assert clean → find template →
/// find cell → assert type" scaffolding that the four happy-path tests share.
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

/// End-to-end fixture: a structure with two params whose annotated types exercise
/// the `Vector3<Q>` and `Point3<Q>` resolution arms.
///
/// - `force_vec : Vector3<Force>` — the `Vector3<Q>` parametric arm.
/// - `origin : Point3<Length>`   — the `Point3<Q>` parametric arm.
const ACCEPTANCE_SOURCE: &str = r#"
structure def Body {
    param force_vec : Vector3<Force>
    param origin : Point3<Length>
}
"#;

#[test]
fn vector3_force_resolves_to_typed_vector() {
    assert_param_type(
        ACCEPTANCE_SOURCE,
        "Body",
        "force_vec",
        &Type::vec3(Type::Scalar {
            dimension: DimensionVector::FORCE,
        }),
    );
}

#[test]
fn point3_length_resolves_to_typed_point() {
    assert_param_type(
        ACCEPTANCE_SOURCE,
        "Body",
        "origin",
        &Type::point3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }),
    );
}

/// Verify the `_with_subst` codepath for `Vector3<Q>`: define a parametric alias
/// `type V<Q> = Vector3<Q>` and annotate a structure param as `V<Dimensionless>`.
/// This exercises `resolve_parameterized_alias` → `resolve_type_alias_expr_with_subst` →
/// `resolve_parameterized_builtin_type_with_subst("Vector3", ..., {Q: Scalar(DIMENSIONLESS)})`.
///
/// The PRD's exact form `Vector3<Dimensionless>` is verified inside this fixture.
const ALIAS_SUBST_SOURCE: &str = r#"
type V<Q> = Vector3<Q>

structure def Alias {
    param dir : V<Dimensionless>
}
"#;

#[test]
fn vector3_via_parametric_alias_resolves_through_subst_path() {
    assert_param_type(
        ALIAS_SUBST_SOURCE,
        "Alias",
        "dir",
        &Type::vec3(Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS,
        }),
    );
}

/// Verify the `_with_subst` codepath for `Point3<Q>`: define a parametric alias
/// `type P<Q> = Point3<Q>` and annotate a structure param as `P<Length>`.
/// This exercises `resolve_parameterized_alias` → `resolve_type_alias_expr_with_subst` →
/// `resolve_parameterized_builtin_type_with_subst("Point3", ..., {Q: Scalar(LENGTH)})`.
const POINT_ALIAS_SUBST_SOURCE: &str = r#"
type P<Q> = Point3<Q>

structure def AliasPoint {
    param origin : P<Length>
}
"#;

#[test]
fn point3_via_parametric_alias_resolves_through_subst_path() {
    assert_param_type(
        POINT_ALIAS_SUBST_SOURCE,
        "AliasPoint",
        "origin",
        &Type::point3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }),
    );
}

// ---------------------------------------------------------------------------
// Negative tests — bad type arguments must produce Error-severity diagnostics
// ---------------------------------------------------------------------------

/// Compile `source` and assert that at least one Error-severity diagnostic is
/// emitted. Used by the negative test battery below.
fn assert_produces_error(source: &str) {
    let module = compile_with_stdlib_helper(source);
    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errs.is_empty(),
        "source must produce at least one Error-severity diagnostic, but got none.\
         \nSource:\n{source}"
    );
}

/// `Vector3<NotADim>` — the quantity type arg is an unknown name; the
/// `resolve_type_alias_expr_to_dimension` helper must emit a "cannot resolve to a
/// dimension type" Error before the Vector3 arm returns `None`.
#[test]
fn vector3_unknown_dimension_produces_error() {
    assert_produces_error("structure def Bad { param v : Vector3<NotADim> }");
}

/// `Point3<NotADim>` — parallel negative fixture for the Point3 arm.
#[test]
fn point3_unknown_dimension_produces_error() {
    assert_produces_error("structure def Bad { param v : Point3<NotADim> }");
}

/// `Vector3<3>` — an integer literal is not a valid quantity; the
/// `resolve_type_alias_expr_to_dimension` helper must emit an
/// "integer literal cannot appear as a dimension type" Error.
#[test]
fn vector3_integer_literal_arg_produces_error() {
    assert_produces_error("structure def Bad { param v : Vector3<3> }");
}

/// `Point3<3>` — parallel negative fixture for the Point3 arm; mirrors
/// `vector3_integer_literal_arg_produces_error`. `resolve_type_alias_expr_to_dimension`
/// rejects integer literals as dimensions in the `Point3<Q>` arm just as it does
/// for `Vector3<Q>`.
#[test]
fn point3_integer_literal_arg_produces_error() {
    assert_produces_error("structure def Bad { param v : Point3<3> }");
}
