//! Tests for physical constants in `std/units` (task 4026).
//!
//! Initially pins two leaf signals for SPEED_OF_LIGHT (steps 1-2);
//! BOLTZMANN_CONSTANT tests are appended in step-3.
//!
//! SI references:
//!   - c = 299792458 m/s exactly — SI second/metre definition (BIPM, 1983).
//!   - k_B = 1.380649e-23 J/K exactly — 2019 SI redefinition
//!     (CGPM 26th meeting, Resolution 1).
//!
//! Pattern lifted from `standard_gravity_tests.rs`.

mod common;

use reify_compiler::stdlib_loader;
use reify_core::{DimensionVector, Severity, Type};
use reify_ir::{CompiledExpr, Value, ValueMap};

// ─── Shared helper: signature + eval in one call ──────────────────────────────

/// Verify that a zero-arg public constant function in `std/units`:
///
/// 1. **Signature** — exists, is `pub`, takes no params, and returns
///    `Type::Scalar { dimension: expected_dim }`.
/// 2. **Eval** — evaluates (via `reify_expr::eval_expr`) to a
///    `Value::Scalar` with `dimension == expected_dim` and
///    `si_value ≈ expected_si_value` within `rel_tol`.
///
/// Deriving `expected_dim` once here (rather than twice per old test pair)
/// makes copy-paste drift between the sig and eval assertions impossible.
fn check_constant(fn_name: &str, expected_dim: DimensionVector, expected_si_value: f64, rel_tol: f64) {
    let module = common::units_module();

    // ── 1. Signature ──────────────────────────────────────────────────────────
    let func = module
        .functions
        .iter()
        .find(|f| f.name == fn_name)
        .unwrap_or_else(|| {
            panic!(
                "{} not found in std/units; available functions: {:?}",
                fn_name,
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });
    assert!(func.is_pub, "{} should be pub", fn_name);
    assert!(
        func.params.is_empty(),
        "{} should take no params, got: {:?}",
        fn_name,
        func.params
    );
    assert_eq!(
        func.return_type,
        Type::Scalar { dimension: expected_dim },
        "{} return type wrong, got {:?}",
        fn_name,
        func.return_type
    );

    // ── 2. Eval ───────────────────────────────────────────────────────────────
    let call_expr = CompiledExpr::user_function_call(
        fn_name.to_string(),
        vec![],
        Type::Scalar { dimension: expected_dim },
    );
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    match reify_expr::eval_expr(&call_expr, &ctx) {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(
                dimension,
                expected_dim,
                "{}() body dimension wrong, got {:?}",
                fn_name,
                dimension
            );
            common::assert_eq_rel(
                si_value,
                expected_si_value,
                rel_tol,
                &format!("{}() si_value", fn_name),
            );
        }
        other => panic!("{}() should return Value::Scalar, got {:?}", fn_name, other),
    }
}

// ─── Test 1: SPEED_OF_LIGHT present and has correct signature ─────────────────

/// `SPEED_OF_LIGHT` must be present in `std/units`, be `pub`, take no
/// parameters, and return `Scalar<LENGTH / TIME>`.
///
/// Return type resolves to the m·s⁻¹ DimensionVector (Scalar{LENGTH/TIME}).
/// As of task 4580, `Velocity` IS now in NAMED_DIMENSIONS, so the units.ri
/// `pub type Velocity = Length / Time` alias is shadowed by the builtin
/// — but both resolve to the same DimensionVector, so this assertion is
/// unchanged.
#[test]
fn speed_of_light_function_present_in_std_units() {
    let module = common::units_module();

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "SPEED_OF_LIGHT")
        .unwrap_or_else(|| {
            panic!(
                "SPEED_OF_LIGHT not found in std/units; found functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    assert!(func.is_pub, "SPEED_OF_LIGHT should be pub");
    assert!(
        func.params.is_empty(),
        "SPEED_OF_LIGHT should take no params, got: {:?}",
        func.params
    );

    let expected_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME);
    assert_eq!(
        func.return_type,
        Type::Scalar {
            dimension: expected_dim
        },
        "SPEED_OF_LIGHT return type should be Scalar<LENGTH / TIME>, got {:?}",
        func.return_type
    );
}

// ─── Test 2: SPEED_OF_LIGHT evaluates to 299792458 m/s ───────────────────────

/// Evaluating `SPEED_OF_LIGHT()` via `eval_expr` must yield a
/// `Value::Scalar` with `si_value ≈ 299792458.0` and `dimension = LENGTH / TIME`.
///
/// c = 299792458 m/s exactly (SI definition, BIPM 1983).
#[test]
fn speed_of_light_evaluates_to_299792458_si_with_length_over_time_dimension() {
    let module = common::units_module();

    let expected_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME);
    let call_expr = CompiledExpr::user_function_call(
        "SPEED_OF_LIGHT".to_string(),
        vec![],
        Type::Scalar {
            dimension: expected_dim,
        },
    );
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    let result = reify_expr::eval_expr(&call_expr, &ctx);

    match result {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                dimension,
                DimensionVector::LENGTH.div(&DimensionVector::TIME),
                "SPEED_OF_LIGHT() should have LENGTH / TIME dimension, got {:?}",
                dimension
            );
            assert!(
                (si_value - 299792458.0).abs() < 1e-12,
                "SPEED_OF_LIGHT() si_value: expected 299792458.0, got {}",
                si_value
            );
        }
        other => panic!(
            "SPEED_OF_LIGHT() should return Value::Scalar, got {:?}",
            other
        ),
    }
}

// ─── Test 3: BOLTZMANN_CONSTANT present and has correct signature ─────────────

/// `BOLTZMANN_CONSTANT` must be present in `std/units`, be `pub`, take no
/// parameters, and return `Scalar<ENERGY / TEMPERATURE>`.
///
/// Return type resolves via the `HeatCapacity` type alias (`pub type HeatCapacity =
/// Energy / Temperature` in units.ri, introduced by esc-4026-121), which the
/// compiler expands to `Scalar<ENERGY/TEMPERATURE>`.
///
/// k_B = 1.380649e-23 J/K exactly — 2019 SI redefinition
/// (CGPM 26th meeting, Resolution 1).
#[test]
fn boltzmann_constant_function_present_in_std_units() {
    let module = common::units_module();

    let func = module
        .functions
        .iter()
        .find(|f| f.name == "BOLTZMANN_CONSTANT")
        .unwrap_or_else(|| {
            panic!(
                "BOLTZMANN_CONSTANT not found in std/units; found functions: {:?}",
                module.functions.iter().map(|f| &f.name).collect::<Vec<_>>()
            )
        });

    assert!(func.is_pub, "BOLTZMANN_CONSTANT should be pub");
    assert!(
        func.params.is_empty(),
        "BOLTZMANN_CONSTANT should take no params, got: {:?}",
        func.params
    );

    let expected_dim = DimensionVector::ENERGY.div(&DimensionVector::TEMPERATURE);
    assert_eq!(
        func.return_type,
        Type::Scalar {
            dimension: expected_dim
        },
        "BOLTZMANN_CONSTANT return type should be Scalar<ENERGY / TEMPERATURE>, got {:?}",
        func.return_type
    );
}

// ─── Test 4: BOLTZMANN_CONSTANT evaluates to 1.380649e-23 J/K ────────────────

/// Evaluating `BOLTZMANN_CONSTANT()` via `eval_expr` must yield a
/// `Value::Scalar` with `si_value ≈ 1.380649e-23` and
/// `dimension = ENERGY / TEMPERATURE`.
///
/// Tolerance is 1e-35: `1.380649e-23` has 7 significant figures; f64
/// precision is ~15-17 digits, so the round-trip error is ≤ 1.5 × ulp ≈
/// 3e-39, comfortably under 1e-35.
#[test]
fn boltzmann_constant_evaluates_to_1p380649e_minus_23_si_with_energy_over_temperature_dimension() {
    let module = common::units_module();

    let expected_dim = DimensionVector::ENERGY.div(&DimensionVector::TEMPERATURE);
    let call_expr = CompiledExpr::user_function_call(
        "BOLTZMANN_CONSTANT".to_string(),
        vec![],
        Type::Scalar {
            dimension: expected_dim,
        },
    );
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);
    let result = reify_expr::eval_expr(&call_expr, &ctx);

    match result {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                dimension,
                DimensionVector::ENERGY.div(&DimensionVector::TEMPERATURE),
                "BOLTZMANN_CONSTANT() should have ENERGY / TEMPERATURE dimension, got {:?}",
                dimension
            );
            assert!(
                (si_value - 1.380649e-23).abs() < 1e-35,
                "BOLTZMANN_CONSTANT() si_value: expected 1.380649e-23, got {:.6e}",
                si_value
            );
        }
        other => panic!(
            "BOLTZMANN_CONSTANT() should return Value::Scalar, got {:?}",
            other
        ),
    }
}

// ─── Tests 5–11: 7 new constants — one test each via check_constant() ─────────
//
// Each test calls `check_constant(name, dim, si_value, rel_tol)` which runs
// both the presence/signature check and the eval check.  Adding a new constant
// in the future means adding one line here rather than ~80 lines of copied code.

/// N_A = 6.02214076×10²³ mol⁻¹ — exact by 2019 SI redefinition (CGPM 26th
/// meeting, Resolution 1).  Dimension: Dimensionless / AmountOfSubstance.
#[test]
fn avogadro_constant_signature_and_eval() {
    check_constant(
        "AVOGADRO_CONSTANT",
        DimensionVector::DIMENSIONLESS.div(&DimensionVector::AMOUNT_OF_SUBSTANCE),
        6.02214076e23,
        1e-12,
    );
}

/// h = 6.62607015×10⁻³⁴ J·s — exact by 2019 SI redefinition (CGPM 26th
/// meeting, Resolution 1).  Dimension: Energy × Time = kg·m²·s⁻¹.
#[test]
fn planck_constant_signature_and_eval() {
    check_constant(
        "PLANCK_CONSTANT",
        DimensionVector::ENERGY.mul(&DimensionVector::TIME),
        6.62607015e-34,
        1e-12,
    );
}

/// σ = 5.670374419×10⁻⁸ W·m⁻²·K⁻⁴ — CODATA 2018.
/// Dimension: Power / Area / T⁴ = kg·s⁻³·K⁻⁴ (T⁴ as four repeated factors).
#[test]
fn stefan_boltzmann_constant_signature_and_eval() {
    check_constant(
        "STEFAN_BOLTZMANN_CONSTANT",
        DimensionVector::POWER
            .div(&DimensionVector::AREA)
            .div(&DimensionVector::TEMPERATURE)
            .div(&DimensionVector::TEMPERATURE)
            .div(&DimensionVector::TEMPERATURE)
            .div(&DimensionVector::TEMPERATURE),
        5.670374419e-8,
        1e-12,
    );
}

/// ε₀ = 8.8541878128×10⁻¹² F/m — CODATA 2018.
/// Dimension: Capacitance / Length = kg⁻¹·m⁻³·s⁴·A².
#[test]
fn vacuum_permittivity_signature_and_eval() {
    check_constant(
        "VACUUM_PERMITTIVITY",
        DimensionVector::CAPACITANCE.div(&DimensionVector::LENGTH),
        8.8541878128e-12,
        1e-12,
    );
}

/// μ₀ = 1.25663706212×10⁻⁶ H/m — CODATA 2018.
/// Dimension: Inductance / Length = kg·m·s⁻²·A⁻².
#[test]
fn vacuum_permeability_signature_and_eval() {
    check_constant(
        "VACUUM_PERMEABILITY",
        DimensionVector::INDUCTANCE.div(&DimensionVector::LENGTH),
        1.25663706212e-6,
        1e-12,
    );
}

/// R = 8.314462618 J·mol⁻¹·K⁻¹ — exact by 2019 SI (R = N_A × k_B).
/// Dimension: Energy / AmountOfSubstance / Temperature = kg·m²·s⁻²·mol⁻¹·K⁻¹.
#[test]
fn molar_gas_constant_signature_and_eval() {
    check_constant(
        "MOLAR_GAS_CONSTANT",
        DimensionVector::ENERGY
            .div(&DimensionVector::AMOUNT_OF_SUBSTANCE)
            .div(&DimensionVector::TEMPERATURE),
        8.314462618,
        1e-12,
    );
}

/// e = 1.602176634×10⁻¹⁹ C — exact by 2019 SI redefinition (CGPM 26th meeting).
/// Dimension: Charge = s·A (pre-existing named dimension; no new alias).
#[test]
fn elementary_charge_signature_and_eval() {
    check_constant(
        "ELEMENTARY_CHARGE",
        DimensionVector::CHARGE,
        1.602176634e-19,
        1e-12,
    );
}

// ─── Test 12: consumer-facing probe — all 7 new constants callable from user code

/// A consumer-facing probe: a `.ri` structure referencing all 7 new physical
/// constant functions compiled via the prelude-seeded path must produce ZERO
/// `Severity::Error` diagnostics.
///
/// This exercises the prelude-seeded user-compilation path (distinct from the
/// internal `units_module()` path used by Tests 5–11), confirming that all 7
/// fns and their composite-dimension aliases are callable from user code and
/// coexist without resolution errors.
#[test]
fn all_seven_new_constants_probe_compiles_with_zero_errors() {
    let source = r#"
structure def ProbeAllConstants {
    param na : InverseAmount = AVOGADRO_CONSTANT()
    param h  : Action = PLANCK_CONSTANT()
    param sigma : StefanBoltzmannDim = STEFAN_BOLTZMANN_CONSTANT()
    param eps0 : Permittivity = VACUUM_PERMITTIVITY()
    param mu0  : Permeability = VACUUM_PERMEABILITY()
    param R    : MolarGasConstant = MOLAR_GAS_CONSTANT()
    param e    : Charge = ELEMENTARY_CHARGE()
}
"#;
    let module = common::compile_with_stdlib_helper(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "probe .ri referencing all 7 new constants should compile with zero Error diagnostics; got:\n{:#?}",
        errors
    );
}

// ─── Tests 13–14: physical-identity cross-checks ──────────────────────────────
//
// These tests pin documented source-of-truth relationships so a future typo
// in one constant but not another is caught immediately.

/// `ELEMENTARY_CHARGE()` si_value must equal the `eV` unit factor in the stdlib.
///
/// The 2019-SI value 1.602176634×10⁻¹⁹ appears in two places:
///   - `si_units.rs` (Rust kernel — the `eV` unit factor, registered in `std/si_units`), and
///   - `units.ri` (the `ELEMENTARY_CHARGE` fn body, in `std/units`).
///
/// This test cross-checks them so an edit to either layer is caught here.
///
/// Note: `eV` lives in `std/si_units`, not `std/units`, so this test searches
/// the full stdlib rather than only the `std/units` compiled module.
#[test]
fn elementary_charge_equals_ev_unit_factor() {
    // Search all stdlib modules for the eV unit (it lives in std/si_units).
    let stdlib = stdlib_loader::load_stdlib();
    let ev_unit = stdlib
        .iter()
        .flat_map(|m| m.units.iter())
        .find(|u| u.name == "eV")
        .expect("eV unit not found in any stdlib module");
    let ev_factor = ev_unit.factor;

    // ELEMENTARY_CHARGE() si_value from the .ri stdlib fn body.
    let std_units = common::units_module(); // needed for the EvalContext function list
    let call_expr = CompiledExpr::user_function_call(
        "ELEMENTARY_CHARGE".to_string(),
        vec![],
        Type::Scalar { dimension: DimensionVector::CHARGE },
    );
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &std_units.functions);
    let e_si = match reify_expr::eval_expr(&call_expr, &ctx) {
        Value::Scalar { si_value, .. } => si_value,
        other => panic!("ELEMENTARY_CHARGE() should return Value::Scalar, got {:?}", other),
    };

    common::assert_eq_rel(
        e_si,
        ev_factor,
        1e-12,
        "ELEMENTARY_CHARGE() si_value must equal the 'eV' unit factor in std/units (same CODATA/SI value; both must stay in sync)",
    );
}

/// Physical identity R = N_A × k_B, verified across independently-evaluated
/// si_values within relative tolerance 1e-10.
///
/// `AVOGADRO_CONSTANT() × BOLTZMANN_CONSTANT()` computed from two separate
/// eval calls must agree with `MOLAR_GAS_CONSTANT()`.  Pins the documented
/// physical/source-of-truth relationship so a future coefficient typo in one
/// constant but not the others is detected here.
///
/// **Tolerance note**: The exact product 6.02214076e23 × 1.380649e-23 =
/// 8.31446261815324…, while `MOLAR_GAS_CONSTANT` encodes 8.314462618 (10
/// significant figures — the CODATA/BIPM published rounding of R).  The
/// discrepancy is ~1.84e-11 relative, inherent in the finite significant-figure
/// encoding of R.  A tolerance of 1e-10 catches any real coefficient typo
/// (which would shift by at least ~1e-7 relative) while accommodating the
/// known encoding rounding.
#[test]
fn avogadro_times_boltzmann_equals_molar_gas_constant() {
    let module = common::units_module();
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &module.functions);

    let na_dim = DimensionVector::DIMENSIONLESS.div(&DimensionVector::AMOUNT_OF_SUBSTANCE);
    let kb_dim = DimensionVector::ENERGY.div(&DimensionVector::TEMPERATURE);
    let r_dim = DimensionVector::ENERGY
        .div(&DimensionVector::AMOUNT_OF_SUBSTANCE)
        .div(&DimensionVector::TEMPERATURE);

    let na_expr = CompiledExpr::user_function_call(
        "AVOGADRO_CONSTANT".to_string(), vec![], Type::Scalar { dimension: na_dim },
    );
    let na = match reify_expr::eval_expr(&na_expr, &ctx) {
        Value::Scalar { si_value, .. } => si_value,
        other => panic!("AVOGADRO_CONSTANT() returned {:?}", other),
    };

    let kb_expr = CompiledExpr::user_function_call(
        "BOLTZMANN_CONSTANT".to_string(), vec![], Type::Scalar { dimension: kb_dim },
    );
    let kb = match reify_expr::eval_expr(&kb_expr, &ctx) {
        Value::Scalar { si_value, .. } => si_value,
        other => panic!("BOLTZMANN_CONSTANT() returned {:?}", other),
    };

    let r_expr = CompiledExpr::user_function_call(
        "MOLAR_GAS_CONSTANT".to_string(), vec![], Type::Scalar { dimension: r_dim },
    );
    let r = match reify_expr::eval_expr(&r_expr, &ctx) {
        Value::Scalar { si_value, .. } => si_value,
        other => panic!("MOLAR_GAS_CONSTANT() returned {:?}", other),
    };

    common::assert_eq_rel(
        na * kb,
        r,
        1e-10,
        "AVOGADRO_CONSTANT() * BOLTZMANN_CONSTANT() should equal MOLAR_GAS_CONSTANT() (R = N_A * k_B)",
    );
}

// ─── Test 15: end-to-end dimensional-algebra — user-compilation → eval ────────

/// A user `.ri` structure that references `ELEMENTARY_CHARGE()` as a param
/// default is compiled via the prelude-seeded path, then the compiled default
/// expression is evaluated through `reify_expr::eval_expr` to confirm that the
/// resulting `si_value` and `dimension` are correct end-to-end.
///
/// This exercises: parse → typecheck with stdlib aliases → compile default
/// expression → `eval_expr` over stdlib functions — the full dimensional-value
/// pipeline, not merely resolvability (Test 12) or direct eval (Tests 5–11).
#[test]
fn probe_default_expression_evaluates_end_to_end_through_user_compilation_path() {
    let source = r#"
structure def EvalProbe {
    param e : Charge = ELEMENTARY_CHARGE()
}
"#;
    let user_module = common::compile_with_stdlib_helper(source);

    // Confirm clean compile first.
    let errors: Vec<_> = user_module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "EvalProbe produced compile errors: {:#?}",
        errors
    );

    // Access the compiled default expression for param `e`.
    let template = user_module
        .templates
        .iter()
        .find(|t| t.name == "EvalProbe")
        .expect("EvalProbe template not found");
    let e_cell = template
        .value_cells
        .iter()
        .find(|c| c.id.member == "e")
        .expect("param 'e' cell not found in EvalProbe");
    let e_expr = e_cell
        .default_expr
        .as_ref()
        .expect("param 'e' has no default_expr (compiler did not emit one for this function-call default)");

    // Evaluate through the stdlib EvalContext — resolves ELEMENTARY_CHARGE() if
    // the expr is a UserFunctionCall, or trivially returns the value if it was
    // constant-folded to a Literal during compilation.
    let std_units = common::units_module();
    let values = ValueMap::new();
    let ctx = reify_expr::EvalContext::new(&values, &std_units.functions);
    match reify_expr::eval_expr(e_expr, &ctx) {
        Value::Scalar { si_value, dimension } => {
            assert_eq!(
                dimension,
                DimensionVector::CHARGE,
                "EvalProbe 'e' dimension should be CHARGE, got {:?}",
                dimension
            );
            common::assert_eq_rel(
                si_value,
                1.602176634e-19,
                1e-12,
                "EvalProbe 'e' si_value through user-compilation + eval path",
            );
        }
        other => panic!(
            "EvalProbe 'e' should eval to Value::Scalar, got {:?}",
            other
        ),
    }
}

// ─── Task-4580 regression guards ─────────────────────────────────────────────

/// Regression guard (task 4580, suggestion 2): `pub type Velocity = Length / Time`
/// in units.ri is now a dead alias shadowed by the NAMED_DIMENSIONS builtin.
/// Confirm units.ri still compiles with zero errors and that SPEED_OF_LIGHT's
/// return type equals `DimensionVector::VELOCITY` (the builtin), not a stale
/// LENGTH/TIME expression whose identity could drift independently.
#[test]
fn units_ri_velocity_alias_shadowed_by_builtin_zero_errors() {
    let module = common::units_module();
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "units.ri should compile with zero errors even with the dead Velocity alias present; got:\n{:#?}",
        errors
    );
    let func = module
        .functions
        .iter()
        .find(|f| f.name == "SPEED_OF_LIGHT")
        .expect("SPEED_OF_LIGHT must be present in std/units");
    assert_eq!(
        func.return_type,
        Type::Scalar { dimension: DimensionVector::VELOCITY },
        "SPEED_OF_LIGHT return type must equal Scalar{{VELOCITY}} — builtin shadows \
         alias, both are m·s⁻¹; got {:?}",
        func.return_type
    );
}

/// Regression guard (task 4580, suggestion 1): adding 'Velocity' to
/// NAMED_DIMENSIONS means LENGTH/TIME scalars now have a canonical name.
/// A dimension mismatch between a velocity and force scalar must produce the
/// secondary label "Velocity and Force are different dimensions…" — pinning
/// that the rendering change is not silently reverted.
#[test]
fn velocity_force_mismatch_secondary_label_includes_velocity() {
    let source = r#"
structure def S {
    let _x = 1m / 1s + 1N
}
"#;
    let module = common::compile_with_stdlib_helper(source);
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "expected a dimension mismatch error for `1m / 1s + 1N`"
    );
    let has_velocity_label = errors
        .iter()
        .flat_map(|d| &d.labels)
        .any(|l| l.message.contains("Velocity") && l.message.contains("Force"));
    assert!(
        has_velocity_label,
        "expected secondary label mentioning 'Velocity and Force'; got labels:\n{:#?}",
        errors
            .iter()
            .flat_map(|d| &d.labels)
            .map(|l| &l.message)
            .collect::<Vec<_>>()
    );
}
