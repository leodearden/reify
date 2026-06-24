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
    let (si, dim) = common::stdlib_param_si_value("Energy", "5kN*m");
    assert_approx(si, 5000.0, "5kN*m SI value");
    assert_eq!(dim, DimensionVector::ENERGY, "5kN*m dimension should be ENERGY");
}

/// `25mm^2` → Area, si_value = 2.5e-5 m²
#[test]
fn compound_mm_pow2_is_area_25e_minus6_m2() {
    let (si, dim) = common::stdlib_param_si_value("Area", "25mm^2");
    assert_approx(si, 2.5e-5, "25mm^2 SI value");
    assert_eq!(dim, DimensionVector::AREA, "25mm^2 dimension should be AREA");
}

/// `9.81m/s^2` → Acceleration, si_value = 9.81 m/s²
#[test]
fn compound_m_div_s2_is_acceleration_9_81() {
    let (si, dim) = common::stdlib_param_si_value("Acceleration", "9.81m/s^2");
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
    let (si, dim) = common::stdlib_param_si_value("Density", "7850kg/m^3");
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
    let (si, dim) = common::stdlib_param_si_value("DynamicViscosity", "0.001kg/m/s");
    assert_approx(si, 0.001, "0.001kg/m/s SI value");
    assert_eq!(
        dim,
        DimensionVector::DYNAMIC_VISCOSITY,
        "0.001kg/m/s dimension should be DYNAMIC_VISCOSITY"
    );
}

// ─── MIGRATION-TARGET PINS (task ζ) ──────────────────────────────────────────
//
// These lock the exact SI value + dimension of the compound literals that
// stdlib/example workaround sites are migrated TO in this task. They run green
// against the current (pre-migration) stdlib — the migrations preserve values,
// so these pins assert the post-migration target before any source is touched
// and catch a mistyped unit during migration. Guards plan steps 2, 5, 14.

/// `0.0001ohm*m` → ElectricResistivity, si_value = 1e-4 Ω·m
/// (Conductive `resistivity <` bound, materials_electrical.ri — step 5).
#[test]
fn compound_ohm_mul_m_is_electric_resistivity_1e_minus4() {
    let (si, dim) = common::stdlib_param_si_value("ElectricResistivity", "0.0001ohm*m");
    assert_approx(si, 1e-4, "0.0001ohm*m SI value");
    assert_eq!(
        dim,
        DimensionVector::ELECTRIC_RESISTIVITY,
        "0.0001ohm*m dimension should be ELECTRIC_RESISTIVITY"
    );
}

/// `1000000ohm*m` → ElectricResistivity, si_value = 1e6 Ω·m
/// (Insulating `resistivity >` bound, materials_electrical.ri — step 5).
#[test]
fn compound_ohm_mul_m_is_electric_resistivity_1e6() {
    let (si, dim) = common::stdlib_param_si_value("ElectricResistivity", "1000000ohm*m");
    assert_approx(si, 1e6, "1000000ohm*m SI value");
    assert_eq!(
        dim,
        DimensionVector::ELECTRIC_RESISTIVITY,
        "1000000ohm*m dimension should be ELECTRIC_RESISTIVITY"
    );
}

/// `0W/(m*K)` → ThermalConductivity, si_value = 0.0 W/(m·K)
/// (ThermallyConductive `>` bound, structural_physical.ri — step 2).
/// Exercises a parenthesised unit group in the denominator.
#[test]
fn compound_w_div_m_times_k_is_thermal_conductivity_zero() {
    let (si, dim) = common::stdlib_param_si_value("ThermalConductivity", "0W/(m*K)");
    assert_approx(si, 0.0, "0W/(m*K) SI value");
    assert_eq!(
        dim,
        DimensionVector::THERMAL_CONDUCTIVITY,
        "0W/(m*K) dimension should be THERMAL_CONDUCTIVITY"
    );
}

/// `1m^2` → Area, si_value = 1.0 m² (topology area-range upper bound,
/// all_topology_selectors_wiring.ri — step 14).
#[test]
fn compound_m_pow2_is_area_1() {
    let (si, dim) = common::stdlib_param_si_value("Area", "1m^2");
    assert_approx(si, 1.0, "1m^2 SI value");
    assert_eq!(dim, DimensionVector::AREA, "1m^2 dimension should be AREA");
}

/// `0mm^2` → Area, si_value = 0.0 m² (topology area-range lower bound,
/// all_topology_selectors_wiring.ri — step 14).
#[test]
fn compound_mm_pow2_is_area_zero() {
    let (si, dim) = common::stdlib_param_si_value("Area", "0mm^2");
    assert_approx(si, 0.0, "0mm^2 SI value");
    assert_eq!(dim, DimensionVector::AREA, "0mm^2 dimension should be AREA");
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

// ─── OVERFLOW: compound literal that produces non-finite SI value ─────────────

/// Overflow guard — a 309-digit numeric value overflows f64 to +Inf.
/// Since task #4681, overflow is detected at the **parse layer** via
/// `check_number_range` wired into `lower_quantity_literal`, so the error
/// surfaces in `parsed.errors` (not in `module.diagnostics`).
/// `compile_source_with_stdlib` would panic on parse errors, so this test
/// calls `reify_compiler::parse_with_stdlib` directly.
#[test]
fn compound_overflow_emits_error_and_discards_value() {
    // 309 nines → f64::INFINITY when parsed (exceeds f64::MAX ≈ 1.8e308)
    let big_num = "9".repeat(309);
    let src = format!(
        "structure def S {{ param x : Energy = {}kN*m }}",
        big_num
    );
    let parsed =
        reify_compiler::parse_with_stdlib(&src, reify_core::ModulePath::single("test"));
    assert!(
        parsed
            .errors
            .iter()
            .any(|e| e.message.contains("overflow") || e.message.contains("finite")),
        "expected an overflow parse error for infinite compound literal; got: {:?}",
        parsed.errors.iter().map(|e| &e.message).collect::<Vec<_>>()
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
    let (si, dim) = common::stdlib_param_si_value("Length", "5mm");
    assert_approx(si, 0.005, "bare 5mm SI value");
    assert_eq!(dim, DimensionVector::LENGTH, "bare 5mm dimension should be LENGTH");
}
