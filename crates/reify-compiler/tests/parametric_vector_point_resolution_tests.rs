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

/// End-to-end fixture: a structure with one param whose annotated type exercises
/// the Vector3<Q> resolution arm (step-1 fixture — extended in later steps).
///
/// - `force_vec : Vector3<Force>` — the new `Vector3<Q>` parametric arm.
const ACCEPTANCE_SOURCE: &str = r#"
structure def Body {
    param force_vec : Vector3<Force>
}
"#;

/// Compile `ACCEPTANCE_SOURCE` and return the resolved cell type for `force_vec`
/// after asserting no Error-severity diagnostics fired.
fn compile_acceptance() -> Type {
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

    template
        .value_cells
        .iter()
        .find(|c| c.id.member == "force_vec")
        .expect("cell `force_vec` not found on `Body`")
        .cell_type
        .clone()
}

#[test]
fn vector3_force_resolves_to_typed_vector() {
    let force_vec = compile_acceptance();
    assert_eq!(
        force_vec,
        Type::vec3(Type::Scalar {
            dimension: DimensionVector::FORCE,
        }),
        "Vector3<Force> must resolve to Type::Vector {{ n: 3, quantity: Scalar(FORCE) }}"
    );
}
