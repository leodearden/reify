//! Integration tests for compound unit resolution in `compile_expr_guarded`'s
//! `ExprKind::QuantityLiteral` arm (task 3803, step-1 / step-2).
//!
//! Tests that `resolve_unit_expr` is correctly wired into the compilation path
//! so that compound unit expressions (Mul/Div/Pow) in value-expression position
//! fold into `Value::Scalar { si_value, dimension }` rather than emitting the
//! old placeholder "compound unit expressions are not yet supported" diagnostic.
//!
//! All positive and error assertions were RED against the placeholder code —
//! they turned GREEN only after the wiring in step-2.

mod common;

use reify_core::{DimensionVector, Severity};
use reify_test_support::{compile_source_with_stdlib, errors_only};

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Compile a structure whose single param uses `literal` as its default, using
/// the full stdlib (so kN/mm/GPa/… are in the unit registry).
/// Returns `(si_value, dimension)` from the param's default expression.
///
/// Panics if compilation produces any Error diagnostics.
fn stdlib_scalar_param(param_type: &str, literal: &str) -> (f64, DimensionVector) {
    common::stdlib_param_si_value(param_type, literal)
}

/// Assert two floats are approximately equal within a 1e-6 relative tolerance.
fn assert_approx(actual: f64, expected: f64, label: &str) {
    let tol = 1e-6 * expected.abs().max(1.0);
    assert!(
        (actual - expected).abs() <= tol,
        "{}: expected {} (tol {}), got {}",
        label,
        expected,
        tol,
        actual
    );
}

// ─── POSITIVE: compound unit params resolve to correct SI value + dimension ──

/// `5kN*m` → Energy, si_value = 5000.0 J
#[test]
fn compound_kn_mul_m_is_energy_5000_j() {
    let (si, dim) = stdlib_scalar_param("Energy", "5kN*m");
    assert_approx(si, 5000.0, "5kN*m SI value");
    assert_eq!(dim, DimensionVector::ENERGY, "5kN*m dimension should be ENERGY");
}

/// `25mm^2` → Area, si_value = 2.5e-5 m²
#[test]
fn compound_mm_pow2_is_area_25e_minus6_m2() {
    let (si, dim) = stdlib_scalar_param("Area", "25mm^2");
    assert_approx(si, 2.5e-5, "25mm^2 SI value");
    assert_eq!(dim, DimensionVector::AREA, "25mm^2 dimension should be AREA");
}

/// `9.81m/s^2` → Acceleration, si_value = 9.81 m/s²
#[test]
fn compound_m_div_s2_is_acceleration_9_81() {
    let (si, dim) = stdlib_scalar_param("Acceleration", "9.81m/s^2");
    assert_approx(si, 9.81, "9.81m/s^2 SI value");
    assert_eq!(
        dim,
        DimensionVector::ACCELERATION,
        "9.81m/s^2 dimension should be ACCELERATION"
    );
}

/// `7850kg/m^3` → Density, si_value = 7850.0 kg/m³
#[test]
fn compound_kg_div_m3_is_density_7850() {
    let (si, dim) = stdlib_scalar_param("Density", "7850kg/m^3");
    assert_approx(si, 7850.0, "7850kg/m^3 SI value");
    assert_eq!(
        dim,
        DimensionVector::MASS_DENSITY,
        "7850kg/m^3 dimension should be MASS_DENSITY (Density)"
    );
}

/// `0.001kg/m/s` → DynamicViscosity, si_value = 0.001 Pa·s
#[test]
fn compound_kg_div_m_div_s_is_dynamic_viscosity_0_001() {
    let (si, dim) = stdlib_scalar_param("DynamicViscosity", "0.001kg/m/s");
    assert_approx(si, 0.001, "0.001kg/m/s SI value");
    assert_eq!(
        dim,
        DimensionVector::DYNAMIC_VISCOSITY,
        "0.001kg/m/s dimension should be DYNAMIC_VISCOSITY"
    );
}

// ─── ERROR: unknown unit in compound → Severity::Error naming the offender ───

/// `5kgg/m` with unknown unit `kgg` → Error diagnostic naming "kgg".
#[test]
fn compound_unknown_unit_kgg_emits_error_naming_kgg() {
    let source = "structure def S { param x : Length = 5kgg/m }";
    let module = compile_source_with_stdlib(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected an Error diagnostic for unknown unit 'kgg', got none"
    );
    let names_kgg = errors
        .iter()
        .any(|d| d.message.contains("kgg"));
    assert!(
        names_kgg,
        "expected an Error naming 'kgg'; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// `5degC/m` with affine unit `degC` in compound → Error diagnostic naming "degC".
#[test]
fn compound_affine_unit_degc_emits_error_naming_degc() {
    let source = "structure def S { param x : Length = 5degC/m }";
    let module = compile_source_with_stdlib(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected an Error diagnostic for affine unit 'degC' in compound, got none"
    );
    let names_degc = errors
        .iter()
        .any(|d| d.message.contains("degC"));
    assert!(
        names_degc,
        "expected an Error naming 'degC'; got: {:?}",
        errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

// ─── REGRESSION: bare-unit path stays unchanged ───────────────────────────────

/// `20degC` (bare affine unit) → si ≈ 293.15 K via standalone offset path.
/// This guards that the bare `UnitExpr::Unit(name)` arm is NOT routed through
/// `resolve_unit_expr` (which would reject all offset units, even bare ones).
#[test]
fn regression_bare_degc_applies_offset_to_kelvin() {
    let source = "structure def S { param temp : Temperature = 20degC }";
    let module = compile_source_with_stdlib(source);
    let errors = errors_only(&module);
    assert!(
        errors.is_empty(),
        "bare 20degC should compile cleanly, got errors: {:?}",
        errors
    );
    let template = module
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template not found");
    let cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "temp")
        .expect("temp cell not found");
    let expr = cell.default_expr.as_ref().expect("temp has no default_expr");
    let (si, _dim) = common::expect_scalar(expr);
    assert_approx(si, 293.15, "bare 20degC SI (kelvin)");
}

/// `5mm` (bare non-affine unit) → si = 0.005 m.
/// Regression guard that `UnitExpr::Unit("mm")` still routes to the standalone
/// lookup path, not through `resolve_unit_expr`.
#[test]
fn regression_bare_mm_resolves_to_0_005_m() {
    let (si, dim) = stdlib_scalar_param("Length", "5mm");
    assert_approx(si, 0.005, "bare 5mm SI value");
    assert_eq!(dim, DimensionVector::LENGTH, "bare 5mm dimension should be LENGTH");
}
