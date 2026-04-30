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

/// Compile `ACCEPTANCE_SOURCE` and return the resolved cell types for
/// `force_vec` and `origin` after asserting no Error-severity diagnostics.
fn compile_acceptance() -> (Type, Type) {
    let module = compile_with_stdlib_helper(ACCEPTANCE_SOURCE);

    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "ACCEPTANCE_SOURCE must produce no Error-severity diagnostics; got: {:?}",
        errs
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Body")
        .expect("template `Body` not found in compiled module");

    let find_cell_type = |member: &str| {
        template
            .value_cells
            .iter()
            .find(|c| c.id.member == member)
            .unwrap_or_else(|| panic!("cell `{}` not found on `Body`", member))
            .cell_type
            .clone()
    };

    (find_cell_type("force_vec"), find_cell_type("origin"))
}

#[test]
fn vector3_force_resolves_to_typed_vector() {
    let (force_vec, _) = compile_acceptance();
    assert_eq!(
        force_vec,
        Type::vec3(Type::Scalar {
            dimension: DimensionVector::FORCE,
        }),
        "Vector3<Force> must resolve to Type::Vector {{ n: 3, quantity: Scalar(FORCE) }}"
    );
}

#[test]
fn point3_length_resolves_to_typed_point() {
    let (_, origin) = compile_acceptance();
    assert_eq!(
        origin,
        Type::point3(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }),
        "Point3<Length> must resolve to Type::Point {{ n: 3, quantity: Scalar(LENGTH) }}"
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
    let module = compile_with_stdlib_helper(ALIAS_SUBST_SOURCE);

    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "ALIAS_SUBST_SOURCE must produce no Error-severity diagnostics; got: {:?}",
        errs
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Alias")
        .expect("template `Alias` not found in compiled module");

    let dir = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "dir")
        .expect("cell `dir` not found on `Alias`")
        .cell_type
        .clone();

    assert_eq!(
        dir,
        Type::vec3(Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS,
        }),
        "V<Dimensionless> (via alias subst) must resolve to Type::Vector {{ n: 3, \
         quantity: Scalar(DIMENSIONLESS) }}"
    );
}
