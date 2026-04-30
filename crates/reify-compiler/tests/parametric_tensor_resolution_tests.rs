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

mod common;

use common::compile_with_stdlib_helper;
use reify_types::{DimensionVector, Severity, Type};

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
         n: 3, quantity: Scalar(MOMENT_OF_INERTIA) }}"
    );
}

#[test]
fn matrix_3_2_length_resolves_to_typed_matrix() {
    let (_, _, transform) = compile_acceptance();
    let expected = Type::matrix(3, 2, Type::Scalar {
        dimension: DimensionVector::LENGTH,
    });
    assert_eq!(
        transform, expected,
        "Matrix<3, 2, Length> must resolve to Type::Matrix {{ m: 3, n: 2, \
         quantity: Scalar(LENGTH) }}"
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
    let parsed = reify_syntax::parse(source, reify_types::ModulePath::single("test"));
    let demo = parsed
        .declarations
        .iter()
        .find_map(|d| match d {
            reify_syntax::Declaration::Structure(s) if s.name == "Demo" => Some(s),
            _ => None,
        })
        .expect("structure `Demo` not found");
    let param = demo
        .members
        .iter()
        .find_map(|m| match m {
            reify_syntax::MemberDecl::Param(p) if p.name == "x" => Some(p),
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
/// Cross-module exposure of stdlib type aliases (so a user module can write
/// `param yield : Stress` without `import std.analysis`) is not wired up
/// today — `PreludeContext` carries modules and pre-flattened enums but not
/// the alias registry. That gap is filed as a follow-up; for task 2696 we
/// only commit to the alias *declaration* shipping in stdlib.
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
