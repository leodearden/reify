//! Compiler typing tests for the std.fields α field-op free-functions
//! (task 4219, PRD docs/prds/v0_6/std-fields-api.md §5.1).
//!
//! KEY INVARIANT (§5.1): `sample(fn_field(|p| …), point)` must type as the
//! **codomain** of the field, NOT as `Field<…>` or as the function's first
//! argument.  This is the headline fix delivered by the field-op compiler arm.
//!
//! Model: `affine_constructor_typing_tests.rs` — same compile-source → find
//! template → find cell → assert result_type pattern.
//!
//! RED today: `expr.rs`'s NoUserFunctions cascade has no `is_field_op` arm,
//! so `fn_field(|p| 2.0 * p)` types as its first-arg fallback
//! (`Function{[Real]→Real}`) and `sample(fn_field(…), 3.0)` inherits the
//! same first-arg fallback instead of resolving to `Real` (the codomain).

use reify_core::{DimensionVector, Type};
use reify_test_support::{compile_source, compile_source_with_stdlib};

#[test]
fn field_op_sample_types_as_codomain_not_field() {
    // `fn_field(|p| 2.0 * p)` — one Real param, Real body → Field<Real, Real>
    // `sample(fn_field(|p| 2.0 * p), 3.0)` — sample of that field at 3.0 → Real
    let source = r#"
        structure FieldHost {
            let f = fn_field(|p| 2.0 * p)
            let s = sample(fn_field(|p| 2.0 * p), 3.0)
        }
    "#;
    let compiled = compile_source(source);

    let host = compiled
        .templates
        .iter()
        .find(|t| t.name == "FieldHost")
        .expect("FieldHost template must compile");

    // Cell `f` must type as Field<Real, Real>
    let f_cell = host
        .value_cells
        .iter()
        .find(|vc| vc.id.member.as_str() == "f")
        .expect("value cell 'f' must exist in FieldHost");
    let f_expr = f_cell
        .default_expr
        .as_ref()
        .expect("cell 'f' must have a default_expr");
    assert_eq!(
        f_expr.result_type,
        Type::Field {
            domain: Box::new(Type::dimensionless_scalar()),
            codomain: Box::new(Type::dimensionless_scalar()),
        },
        "fn_field(|p| 2.0 * p) must type as Field<Real,Real> (PRD §5.1), got {:?}",
        f_expr.result_type
    );

    // Cell `s` must type as Real — THE §5.1 INVARIANT
    let s_cell = host
        .value_cells
        .iter()
        .find(|vc| vc.id.member.as_str() == "s")
        .expect("value cell 's' must exist in FieldHost");
    let s_expr = s_cell
        .default_expr
        .as_ref()
        .expect("cell 's' must have a default_expr");
    assert_eq!(
        s_expr.result_type,
        Type::dimensionless_scalar(),
        "sample(fn_field(|p| 2.0 * p), 3.0) must type as Real (codomain), not Field or \
         Function (PRD §5.1 THE FIX), got {:?}",
        s_expr.result_type
    );

    // fn_field is a registered builtin: it must NOT emit the zero-arg-function
    // fallback warning (matches the affine_constructor_typing_tests.rs pattern).
    let zero_arg_warning = compiled.diagnostics.iter().find(|d| {
        d.message
            .contains("cannot infer return type of zero-arg function")
    });
    assert!(
        zero_arg_warning.is_none(),
        "fn_field/sample must not emit the zero-arg fallback warning; got: {:?}",
        zero_arg_warning.map(|d| &d.message)
    );
}

// ── W1 compile-level: max(Field<Real,Real>) must type as Real (task #4629) ────

/// `max(fn_field(|p| 2.0 * p))` — max of a scalar Real→Real field must type
/// as `Real` at compile time, NOT as `Field<Real,Real>`.
///
/// This exercises the NoUserFunctions ladder end-to-end: `fn_field` types the
/// field as `Field<Real,Real>`, then `max` of that field must recognise the
/// Field arg and return the reduced codomain (Real), not the Field type.
///
/// RED (step-1): the current `min | max | clamp | lerp` arm is kind-preserving
/// identity — `max(Field<Real,Real>)` types as `Field<Real,Real>`, not Real.
/// GREEN (step-2): W1 splits the arm — Field arg → reduce to codomain scalar.
#[test]
fn max_of_scalar_field_types_as_scalar_not_field() {
    let source = r#"
        structure MaxFieldTest {
            let f    = fn_field(|p| 2.0 * p)
            let peak = max(f)
        }
    "#;
    let compiled = compile_source(source);

    let host = compiled
        .templates
        .iter()
        .find(|t| t.name == "MaxFieldTest")
        .expect("MaxFieldTest template must compile");

    // Cell `peak` must type as Real (the reduced codomain), not as Field.
    let peak_cell = host
        .value_cells
        .iter()
        .find(|vc| vc.id.member.as_str() == "peak")
        .expect("value cell 'peak' must exist in MaxFieldTest");
    let peak_expr = peak_cell
        .default_expr
        .as_ref()
        .expect("cell 'peak' must have a default_expr");
    assert_eq!(
        peak_expr.result_type,
        Type::dimensionless_scalar(),
        "max(fn_field(|p| 2.0*p)) must type as Real (field reduction), got {:?}",
        peak_expr.result_type
    );
}

// ── W2 compile-level: envelope_von_mises etc. type as Field (task #4629) ────
//
// envelope_von_mises / envelope_max_principal / envelope_displacement_magnitude
// are eval-only, name-dispatched builtins (eval_fea, no .ri fn body). Without a
// name-only resolver in the NoUserFunctions ladder, the first-arg fallback types
// these as Type::StructureRef("MultiCaseResult") — the input arg type.
//
// After W2 wires the fea-envelope resolver (step-4), these must type as:
//   envelope_von_mises / envelope_max_principal →
//       Field<Point3<Length>, Scalar<PRESSURE>>
//   envelope_displacement_magnitude →
//       Field<Point3<Length>, Scalar<LENGTH>>
//
// RED (step-3): NoUserFunctions ladder has no fea-envelope arm → first-arg
//   fallback returns StructureRef("MultiCaseResult"), not Field.
// GREEN (step-4): fea-envelope resolver added and wired.

/// Helper: find a value cell's `result_type` within the first compiled template.
fn cell_result_type(
    compiled: &reify_compiler::CompiledModule,
    template_name: &str,
    cell_name: &str,
) -> Type {
    let tmpl = compiled
        .templates
        .iter()
        .find(|t| t.name == template_name)
        .unwrap_or_else(|| {
            panic!(
                "template {template_name:?} not found; found: {:?}",
                compiled.templates.iter().map(|t| &t.name).collect::<Vec<_>>()
            )
        });
    let cell = tmpl
        .value_cells
        .iter()
        .find(|vc| vc.id.member.as_str() == cell_name)
        .unwrap_or_else(|| {
            panic!(
                "cell {cell_name:?} not found in {template_name:?}; found: {:?}",
                tmpl.value_cells
                    .iter()
                    .map(|vc| vc.id.member.as_str())
                    .collect::<Vec<_>>()
            )
        });
    cell.default_expr
        .as_ref()
        .expect("cell must have a default_expr")
        .result_type
        .clone()
}

/// The expected Field<Point3<Length>, Scalar<PRESSURE>> return type for
/// envelope_von_mises and envelope_max_principal.
fn field_point3_length_scalar_pressure() -> Type {
    Type::Field {
        domain: Box::new(Type::Point {
            n: 3,
            quantity: Box::new(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
        }),
        codomain: Box::new(Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        }),
    }
}

/// The expected Field<Point3<Length>, Scalar<LENGTH>> return type for
/// envelope_displacement_magnitude.
fn field_point3_length_scalar_length() -> Type {
    Type::Field {
        domain: Box::new(Type::Point {
            n: 3,
            quantity: Box::new(Type::Scalar {
                dimension: DimensionVector::LENGTH,
            }),
        }),
        codomain: Box::new(Type::Scalar {
            dimension: DimensionVector::LENGTH,
        }),
    }
}

/// `envelope_von_mises(results)` must type as
/// `Field<Point3<Length>, Scalar<PRESSURE>>` at compile time.
///
/// The resolver supplies this return type independent of the arg type;
/// the call stays a name-dispatched FunctionCall (eval unchanged).
///
/// RED (step-3): NoUserFunctions ladder has no fea-envelope arm → first-arg
///   fallback types this as StructureRef("MultiCaseResult"), not Field.
/// GREEN (step-4): fea-envelope resolver added; result type is Field.
#[test]
fn envelope_von_mises_types_as_field_point3_pressure() {
    let source = r#"
        structure EnvVonMisesTypingTest {
            param results : MultiCaseResult
            let envelope  = envelope_von_mises(results)
        }
    "#;
    let compiled = compile_source_with_stdlib(source);
    let got = cell_result_type(&compiled, "EnvVonMisesTypingTest", "envelope");
    assert_eq!(
        got,
        field_point3_length_scalar_pressure(),
        "envelope_von_mises(results) must type as Field<Point3<Length>,Scalar<PRESSURE>>; \
         got {:?} (W2 step-3 RED / step-4 GREEN)",
        got
    );
}

/// `envelope_max_principal(results)` must type as
/// `Field<Point3<Length>, Scalar<PRESSURE>>` at compile time (same return
/// type as `envelope_von_mises` — both are per-point pressure envelopes).
///
/// RED (step-3): first-arg fallback → StructureRef("MultiCaseResult").
/// GREEN (step-4): fea-envelope resolver added.
#[test]
fn envelope_max_principal_types_as_field_point3_pressure() {
    let source = r#"
        structure EnvMaxPrincipalTypingTest {
            param results   : MultiCaseResult
            let envelope_mp = envelope_max_principal(results)
        }
    "#;
    let compiled = compile_source_with_stdlib(source);
    let got = cell_result_type(&compiled, "EnvMaxPrincipalTypingTest", "envelope_mp");
    assert_eq!(
        got,
        field_point3_length_scalar_pressure(),
        "envelope_max_principal(results) must type as Field<Point3<Length>,Scalar<PRESSURE>>; \
         got {:?} (W2 step-3 RED / step-4 GREEN)",
        got
    );
}

/// `envelope_displacement_magnitude(results)` must type as
/// `Field<Point3<Length>, Scalar<LENGTH>>` — displacement is a length field.
///
/// RED (step-3): first-arg fallback → StructureRef("MultiCaseResult").
/// GREEN (step-4): fea-envelope resolver added.
#[test]
fn envelope_displacement_magnitude_types_as_field_point3_length() {
    let source = r#"
        structure EnvDispMagTypingTest {
            param results  : MultiCaseResult
            let envelope_d = envelope_displacement_magnitude(results)
        }
    "#;
    let compiled = compile_source_with_stdlib(source);
    let got = cell_result_type(&compiled, "EnvDispMagTypingTest", "envelope_d");
    assert_eq!(
        got,
        field_point3_length_scalar_length(),
        "envelope_displacement_magnitude(results) must type as Field<Point3<Length>,Scalar<LENGTH>>; \
         got {:?} (W2 step-3 RED / step-4 GREEN)",
        got
    );
}
