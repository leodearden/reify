//! Investigative probe: does `Field<X, Y>` work in `param` positions?
//!
//! Filed by task 3117 (task-G in `docs/notes/stdlib-real-placeholder-audit.md`)
//! to determine whether the TODO at `solver_elastic.ri:243-260` was stale.
//!
//! **Finding:** both tests below pass without any resolver changes. The `Field<D, C>`
//! arm at `crates/reify-compiler/src/type_resolution.rs:1313` (added by task 3088)
//! already accepts `Field<X, Y>` in `param` positions. The TODO was stale.
//!
//! The probe uses the fixture name `FieldProbe` (not `Body` from
//! `parametric_field_resolution_tests.rs`) so future readers asking "does
//! `Field<X,Y>` work in `param` positions for the two `ElasticResult` forms
//! specifically?" have a self-documenting answer here, independently of the
//! broader resolver-coverage tests in `parametric_field_resolution_tests.rs`.
//!
//! Both test cases pass on first run — this is the investigative finding, not a
//! TDD violation. The classic red→green pair driving the actual stdlib tightening
//! lives in `solver_elastic_tests.rs::elastic_result_struct_has_correct_param_shape`
//! (step-2 → step-3 of task 3117).

mod common;

use common::compile_with_stdlib_helper;
use reify_types::{DimensionVector, Severity, Type};

// ---------------------------------------------------------------------------
// Helper: compile and assert resolved cell type for FieldProbe
// ---------------------------------------------------------------------------

fn assert_param_type(source: &str, member: &str, expected: &Type) {
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
        .find(|t| t.name == "FieldProbe")
        .unwrap_or_else(|| panic!("template `FieldProbe` not found in compiled module"));

    let cell_type = template
        .value_cells
        .iter()
        .find(|c| c.id.member == member)
        .unwrap_or_else(|| panic!("cell `{}` not found on `FieldProbe`", member))
        .cell_type
        .clone();

    assert_eq!(
        cell_type, *expected,
        "FieldProbe::{} — expected {:?}, got {:?}",
        member, expected, cell_type
    );
}

// ---------------------------------------------------------------------------
// Probe test 1: displacement form (Field<Point3<Length>, Vector3<Length>>)
// ---------------------------------------------------------------------------

/// `Field<Point3<Length>, Vector3<Length>>` in a `param` slot must resolve to
/// `Type::Field { domain: Point3(Length), codomain: Vector3(Length) }`.
///
/// This is the canonical type for `ElasticResult.displacement`. The test
/// confirms the resolver arm at `type_resolution.rs:1313` works in a
/// `structure def FieldProbe { param … }` fixture — the same shape as the
/// stdlib's `ElasticResult` declaration.
#[test]
fn displacement_form_resolves_in_param_position() {
    let source = r#"
structure def FieldProbe {
    param disp : Field<Point3<Length>, Vector3<Length>>
}
"#;
    assert_param_type(
        source,
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

// ---------------------------------------------------------------------------
// Probe test 2: stress form (Field<Point3<Length>, Tensor<2, 3, Pressure>>)
// ---------------------------------------------------------------------------

/// `Field<Point3<Length>, Tensor<2, 3, Pressure>>` in a `param` slot must
/// resolve to `Type::Field { domain: Point3(Length), codomain: Tensor { rank:2,
/// n:3, quantity: Scalar(PRESSURE) } }`.
///
/// This is the canonical type for `ElasticResult.stress`. Passing confirms the
/// resolver arm at `type_resolution.rs:1313` resolves both domain and codomain
/// via the full-type resolver (not dimension-only), correctly handling the
/// nested `Tensor<2, 3, Pressure>` codomain.
#[test]
fn stress_form_resolves_in_param_position() {
    let source = r#"
structure def FieldProbe {
    param stress : Field<Point3<Length>, Tensor<2, 3, Pressure>>
}
"#;
    assert_param_type(
        source,
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
