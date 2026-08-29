//! Acceptance test for the surface-syntax → IR lowering of parametric
//! `Tensor<rank, n, q>` / `Matrix<m, n, q>` / `Scalar<Q>` types and the new
//! `MomentOfInertia` / `Density` named dimensions.
//!
//! The architect plan for task 2696 specified an acceptance fixture of
//! `fn f(b: Solid, density: Scalar<Density>) -> Tensor<2, 3, MomentOfInertia>`,
//! but `fn` declarations carry the additional burden of validating the body
//! against the declared return type — which would require an actual rank-2
//! tensor literal at the language level (out of scope: 2696 is type-system
//! only, not value-level builtins). A `structure def` with `param`s exercises
//! the same surface→IR resolution path with no body-type-checking distraction:
//! the parser builds the same `TypeExprKind::Named { name: "Tensor", type_args:
//! [IntegerLiteral, IntegerLiteral, Named] }` shape regardless of whether the
//! type is anchored to a fn signature or a param annotation.

use crate::common::compile_with_stdlib_helper;
use reify_core::{DimensionVector, Severity, Type};

/// End-to-end fixture: a structure with three params whose annotated types
/// exercise every new resolution arm shipped under task 2696.
///
/// - `density : Scalar<Density>` — the new `Scalar<Q>` parametric arm + the
///   new `Density` named dimension (kg·m⁻³).
/// - `inertia : Tensor<2, 3, MomentOfInertia>` — the new `Tensor` parametric
///   arm consuming two `IntegerLiteral` type-args + a quantity type, plus
///   the new `MomentOfInertia` named dimension (kg·m²).
/// - `transform : Matrix<3, 2, Length>` — the new `Matrix` parametric arm.
const ACCEPTANCE_SOURCE: &str = r#"
structure def Body {
    param density : Scalar<Density>
    param inertia : Tensor<2, 3, MomentOfInertia>
    param transform : Matrix<3, 2, Length>
}
"#;

/// Compile `ACCEPTANCE_SOURCE` and return the resolved cell types for
/// `density`, `inertia`, `transform` after asserting no Error-severity
/// diagnostics fired.
fn compile_acceptance() -> (Type, Type, Type) {
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

    (
        find_cell_type("density"),
        find_cell_type("inertia"),
        find_cell_type("transform"),
    )
}

#[test]
fn scalar_density_resolves_to_mass_density_singleton() {
    let (density, _, _) = compile_acceptance();
    assert_eq!(
        density,
        Type::Scalar {
            dimension: DimensionVector::MASS_DENSITY,
        },
        "Scalar<Density> must resolve to Type::Scalar with the MASS_DENSITY \
         (kg·m⁻³) dimension, not MAGNETIC_FLUX_DENSITY (kg·s⁻²·A⁻¹)."
    );
}

#[test]
fn tensor_2_3_moment_of_inertia_resolves_to_typed_tensor() {
    let (_, inertia, _) = compile_acceptance();
    let expected = Type::tensor(
        2,
        3,
        Type::Scalar {
            dimension: DimensionVector::MOMENT_OF_INERTIA,
        },
    );
    assert_eq!(
        inertia, expected,
        "Tensor<2, 3, MomentOfInertia> must resolve to Type::Tensor {{ rank: 2, \
         n: 3, quantity: Scalar<MomentOfInertia> }}"
    );
}

#[test]
fn matrix_3_2_length_resolves_to_typed_matrix() {
    let (_, _, transform) = compile_acceptance();
    let expected = Type::matrix(
        3,
        2,
        Type::Scalar {
            dimension: DimensionVector::LENGTH,
        },
    );
    assert_eq!(
        transform, expected,
        "Matrix<3, 2, Length> must resolve to Type::Matrix {{ m: 3, n: 2, \
         quantity: Scalar<Length> }}"
    );
}

/// Regression guard: parametric Display round-trip — the parsed type-expr
/// stringifies back to source-equivalent form, including integer literals.
#[test]
fn tensor_type_expr_displays_integer_args_round_trip() {
    let source = r#"
structure def Demo {
    param x : Tensor<2, 3, MomentOfInertia>
}
"#;
    let parsed = reify_syntax::parse(source, reify_core::ModulePath::single("test"));
    let demo = parsed
        .declarations
        .iter()
        .find_map(|d| match d {
            reify_ast::Declaration::Structure(s) if s.name == "Demo" => Some(s),
            _ => None,
        })
        .expect("structure `Demo` not found");
    let param = demo
        .members
        .iter()
        .find_map(|m| match m {
            reify_ast::MemberDecl::Param(p) if p.name == "x" => Some(p),
            _ => None,
        })
        .expect("param `x` not found");
    let type_expr = param.type_expr.as_ref().expect("missing type annotation");
    assert_eq!(
        format!("{}", type_expr),
        "Tensor<2, 3, MomentOfInertia>",
        "Display impl must round-trip integer-literal type-args verbatim"
    );
}

/// Smoke pin: `analysis.ri` declares `pub type Stress = Pressure` and
/// `pub type Strain = Dimensionless`. The stdlib loader fails fast on any
/// Error-severity diagnostic in any prelude module
/// (`stdlib_loader.rs::load_stdlib` line ~129), so simply *triggering* the
/// stdlib load via `compile_with_stdlib_helper` is enough to certify that
/// the aliases parse, type-resolve, and produce no diagnostics inside their
/// own module.
///
/// Cross-module exposure of stdlib type aliases is now wired up (task 2750):
/// see `stdlib_stress_alias_resolves_to_pressure_dimension` and
/// `stdlib_strain_alias_resolves_to_dimensionless` for the acceptance tests
/// that verify a user module can write `param yield : Stress` and resolve it
/// to `Type:: Scalar<Pressure>` without an in-module alias decl.  This
/// smoke test is retained as a fast-fail load-time assertion: it exercises the
/// stdlib loader path and confirms no Error diagnostics appear in the prelude
/// modules themselves.
#[test]
fn stdlib_stress_strain_aliases_load_without_errors() {
    let module = compile_with_stdlib_helper("structure def Empty { }");
    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "stdlib (incl. analysis.ri Stress/Strain aliases) must compile clean; got: {:?}",
        errs
    );
}

/// Acceptance test (task 2750): `pub type Stress = Pressure` in `analysis.ri`
/// must be visible in user modules compiled with the stdlib prelude.
///
/// Prior to task 2750, `PreludeContext` carried modules and pre-flattened enums
/// but not alias data; this test ensures the `Stress` alias is now propagated
/// through the prelude-seeding mechanism introduced in step-4 and resolves to
/// `Type:: Scalar<Pressure>` in a user-module param annotation.
///
/// See also: `stdlib_strain_alias_resolves_to_dimensionless`.
#[test]
fn stdlib_stress_alias_resolves_to_pressure_dimension() {
    let source = "structure def Beam { param yield : Stress }";
    let module = compile_with_stdlib_helper(source);

    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "structure with `param yield : Stress` must compile clean with stdlib; got: {:?}",
        errs
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Beam")
        .expect("template `Beam` not found in compiled module");

    let yield_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "yield")
        .expect("value cell `yield` not found on `Beam`");

    assert_eq!(
        yield_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::PRESSURE,
        },
        "param `yield : Stress` must resolve to Type:: Scalar<Pressure> via stdlib prelude alias"
    );
}

/// Acceptance test (task 2750): `pub type Strain = Dimensionless` in
/// `analysis.ri` must be visible in user modules compiled with the stdlib
/// prelude.
///
/// See also: `stdlib_stress_alias_resolves_to_pressure_dimension`.
#[test]
fn stdlib_strain_alias_resolves_to_dimensionless() {
    let source = "structure def Specimen { param elongation : Strain }";
    let module = compile_with_stdlib_helper(source);

    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "structure with `param elongation : Strain` must compile clean with stdlib; got: {:?}",
        errs
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "Specimen")
        .expect("template `Specimen` not found in compiled module");

    let elong_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "elongation")
        .expect("value cell `elongation` not found on `Specimen`");

    assert_eq!(
        elong_cell.cell_type,
        Type::Scalar {
            dimension: DimensionVector::DIMENSIONLESS,
        },
        "param `elongation : Strain` must resolve to Type:: Scalar<Real> via stdlib prelude alias"
    );
}

/// Acceptance test (task 6092): `pub type Pressure2 = Pressure * Pressure` and
/// `pub type Pressure3 = Pressure2 * Pressure` in `units.ri` must be visible in
/// user modules compiled with the stdlib prelude, and must resolve to EXACTLY
/// PRESSURE² / PRESSURE³.
///
/// These aliases exist so `std.fea`'s `StressInvariants` fields can carry the
/// dimensions the `stress_invariants` builtin already emits. The expected
/// dimensions are derived here with `DimensionVector::mul` the same way the
/// producer derives the real ones (`crates/reify-stdlib/src/analysis.rs`:
/// `let dim2 = dim.mul(&dim); let dim3 = dim2.mul(&dim);`) rather than being
/// hardcoded exponent literals, so the test and the runtime cannot silently
/// disagree about what "PRESSURE squared" means.
///
/// `Pressure3` chains through `Pressure2` — this also pins that the transitive
/// alias path (alias RHS naming another alias) resolves without precision loss.
#[test]
fn stdlib_pressure_power_aliases_resolve_to_squared_and_cubed_pressure() {
    let source = r#"
structure def PressurePowers {
    param a : Pressure
    param b : Pressure2
    param c : Pressure3
}
"#;
    let module = compile_with_stdlib_helper(source);

    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "structure with `Pressure`/`Pressure2`/`Pressure3` params must compile clean \
         with stdlib; got: {:?}",
        errs
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "PressurePowers")
        .expect("template `PressurePowers` not found in compiled module");

    let find_cell_type = |member: &str| {
        template
            .value_cells
            .iter()
            .find(|c| c.id.member == member)
            .unwrap_or_else(|| panic!("value cell `{}` not found on `PressurePowers`", member))
            .cell_type
            .clone()
    };

    let p1 = DimensionVector::PRESSURE;
    let p2 = p1.mul(&DimensionVector::PRESSURE);
    let p3 = p2.mul(&DimensionVector::PRESSURE);

    assert_eq!(
        find_cell_type("a"),
        Type::Scalar { dimension: p1 },
        "param `a : Pressure` must resolve to Type::Scalar<PRESSURE>"
    );
    assert_eq!(
        find_cell_type("b"),
        Type::Scalar { dimension: p2 },
        "param `b : Pressure2` must resolve to Type::Scalar<PRESSURE²> \
         (= DimensionVector::PRESSURE.mul(&PRESSURE))"
    );
    assert_eq!(
        find_cell_type("c"),
        Type::Scalar { dimension: p3 },
        "param `c : Pressure3` must resolve to Type::Scalar<PRESSURE³> via the \
         transitive alias chain Pressure3 -> Pressure2 * Pressure"
    );
}

/// Acceptance test (task 6092), compile side: the three `StressInvariants`
/// fields must be ACCEPTED where a `Pressure` / `Pressure2` / `Pressure3` is
/// demanded.
///
/// The `fn` params are the load-bearing part. Overload resolution is what makes
/// the declared field type observable: while the fields were typed `Real`, this
/// fixture failed with `no matching overload for takes_p3(Real), candidates:
/// takes_p3(Scalar[kg^3·m^-3·s^-6])`. So the test cannot pass by accident —
/// it can only pass once `fea.ri` declares the dimensions the runtime already
/// produces.
///
/// Expected dimensions are `.mul()`-derived, matching the producer, exactly as
/// in `stdlib_pressure_power_aliases_resolve_to_squared_and_cubed_pressure`.
#[test]
fn stress_invariants_fields_accepted_at_pressure_power_fn_params() {
    let source = r#"
fn takes_p(p : Pressure) -> Pressure { p }
fn takes_p2(p : Pressure2) -> Pressure2 { p }
fn takes_p3(p : Pressure3) -> Pressure3 { p }

structure def InvariantTyping {
    param sigma : Tensor<2, 3, Pressure>

    let inv = stress_invariants(sigma)
    let a   = takes_p(inv.i1)
    let b   = takes_p2(inv.i2)
    let c   = takes_p3(inv.i3)
}
"#;
    let module = compile_with_stdlib_helper(source);

    let errs: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errs.is_empty(),
        "StressInvariants fields must be accepted at Pressure/Pressure2/Pressure3 \
         fn params; got: {:?}",
        errs
    );

    let template = module
        .templates
        .iter()
        .find(|t| t.name == "InvariantTyping")
        .expect("template `InvariantTyping` not found in compiled module");

    let find_cell_type = |member: &str| {
        template
            .value_cells
            .iter()
            .find(|c| c.id.member == member)
            .unwrap_or_else(|| panic!("value cell `{}` not found on `InvariantTyping`", member))
            .cell_type
            .clone()
    };

    let p1 = DimensionVector::PRESSURE;
    let p2 = p1.mul(&DimensionVector::PRESSURE);
    let p3 = p2.mul(&DimensionVector::PRESSURE);

    assert_eq!(
        find_cell_type("a"),
        Type::Scalar { dimension: p1 },
        "takes_p(inv.i1) must resolve to Type::Scalar<PRESSURE>"
    );
    assert_eq!(
        find_cell_type("b"),
        Type::Scalar { dimension: p2 },
        "takes_p2(inv.i2) must resolve to Type::Scalar<PRESSURE²>"
    );
    assert_eq!(
        find_cell_type("c"),
        Type::Scalar { dimension: p3 },
        "takes_p3(inv.i3) must resolve to Type::Scalar<PRESSURE³>"
    );
}
