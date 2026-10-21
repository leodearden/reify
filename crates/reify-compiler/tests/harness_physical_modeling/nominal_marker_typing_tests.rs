//! Compiler typing tests for the `nominal()` inert geometry-marker builtin
//! (η/4480, PRD docs/prds/v0_6/gdt-geometric-zones-and-containment.md task η,
//! contract C3).
//!
//! `nominal()` is a zero-arg builtin returning an inert `Geometry`-typed marker
//! (an INVALID-handle `Value::GeometryHandle`, see `reify-stdlib`). It is the
//! default for `Conforms.actual` — `param actual : Geometry = nominal()` — so
//! the compiler must recognise it as a zero-arg `Geometry`-returning function.
//! Param defaults compile in a *neutral scope* (functions.rs:106-130), so a
//! `= tolerance.feature` default cannot evaluate; the inert `nominal()` marker
//! is the only way to keep the param `Geometry`-typed while the constraint body
//! ignores it.
//!
//! RED today: `expr.rs`'s `NoUserFunctions` result-type cascade has no arm for
//! `nominal`, so a `nominal()` call falls to the zero-arg fallback — typed
//! `Real` (`Type::dimensionless_scalar()`) and emitting the
//! "cannot infer return type of zero-arg function" warning. After step-04
//! registers `nominal` it must type as `Type::Geometry` with no warning.

use reify_core::{Severity, Type};
use reify_test_support::compile_source;

/// The zero-arg-fallback warning string emitted by `expr.rs::infer_type` when a
/// zero-arg call reaches the final `else` arm without a registered result type.
const ZERO_ARG_WARNING: &str = "cannot infer return type of zero-arg function";

/// A bare `nominal()` call must type its cell as `Type::Geometry`, not the
/// zero-arg fallback `Real`, and must NOT emit the zero-arg fallback warning.
#[test]
fn nominal_call_types_as_geometry_with_no_zero_arg_warning() {
    let source = r#"
        structure NominalHost {
            let g = nominal()
        }
    "#;
    let compiled = compile_source(source);

    let host = compiled
        .templates
        .iter()
        .find(|t| t.name == "NominalHost")
        .expect("NominalHost template");

    let cell = host
        .value_cells
        .iter()
        .find(|vc| vc.id.member.as_str() == "g")
        .expect("value cell 'g' not found in NominalHost");
    let default_expr = cell
        .default_expr
        .as_ref()
        .expect("cell 'g' has no default_expr");
    assert_eq!(
        default_expr.result_type,
        Type::Geometry,
        "nominal() cell must type as Geometry, got {:?}",
        default_expr.result_type
    );

    // Registration means nominal() must NOT trip the zero-arg fallback warning.
    let zero_arg_warning = compiled
        .diagnostics
        .iter()
        .find(|d| d.message.contains(ZERO_ARG_WARNING));
    assert!(
        zero_arg_warning.is_none(),
        "nominal() must not emit the zero-arg fallback warning, got: {:?}",
        zero_arg_warning.map(|d| &d.message)
    );
}

/// The real use site: `param actual : Geometry = nominal()` on a constraint def
/// (mirrors the Conforms shape — an UNUSED geometry param whose default is the
/// inert marker; the predicate is purely scalar and never references `actual`).
/// This must compile with no error diagnostics and no zero-arg warning.
#[test]
fn constraint_def_param_actual_geometry_default_nominal_type_checks() {
    let source = r#"
        constraint def UsesNominal {
            param actual : Geometry = nominal()
            param a : Length
            a > 0mm
        }
    "#;
    let compiled = compile_source(source);

    let errors: Vec<&reify_core::Diagnostic> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "`param actual : Geometry = nominal()` must type-check with no errors \
         (no unresolved-function / type-mismatch), got: {:?}",
        errors
    );

    let zero_arg_warning = compiled
        .diagnostics
        .iter()
        .find(|d| d.message.contains(ZERO_ARG_WARNING));
    assert!(
        zero_arg_warning.is_none(),
        "nominal() default must not emit the zero-arg fallback warning, got: {:?}",
        zero_arg_warning.map(|d| &d.message)
    );

    // Positive shape assertion: the def is present with both params (the unused
    // `actual` survives compilation alongside the scalar `a`).
    let def = compiled
        .constraint_defs
        .iter()
        .find(|d| d.name == "UsesNominal")
        .expect("UsesNominal constraint def must be present in module.constraint_defs");
    assert_eq!(
        def.params.len(),
        2,
        "expected UsesNominal to have 2 params (actual, a), got {}",
        def.params.len()
    );
}
