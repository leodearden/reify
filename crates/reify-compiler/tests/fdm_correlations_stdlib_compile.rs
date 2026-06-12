//! Tests for `crates/reify-compiler/stdlib/fdm_correlations.ri` —
//! `std.fdm.correlations` module: the `FDMCorrelationDefaults` and
//! `FDMCouponOverride` structures — the human-facing citation + override
//! surface for the v0.5 FDM-as-printed-FEA effective-property correlation
//! library (task β / slice 1; docs/prds/v0_5/fdm-as-printed-fea.md
//! §"Built-in property correlations").
//!
//! These tests pin the *human-facing* surface: the default numeric
//! correlation constants, their `MaterialPropertyProvenance` citations, and
//! the parallel `..._low_confidence : Bool` flags. The Rust compute
//! source-of-truth (`crates/reify-fdm/src/correlation.rs`) carries the same
//! constants under its own tests; the two surfaces must move together (see
//! Plan §"Design Decisions").
//!
//! All tests use the production-path `load_stdlib_module()` helper that
//! exercises the same embedded + sequential-prelude compilation path as
//! production, mirroring the helper trio in `fdm_stdlib_compile.rs`.

use reify_compiler::*;
use reify_core::*;
use reify_ir::*;
use reify_test_support::compile_source_with_stdlib;

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Return the `std/fdm/correlations` CompiledModule from the production stdlib
/// loader. Exercises the exact same code path as production: embedded source,
/// sequential compilation with growing prelude, OnceLock caching.
fn load_stdlib_module() -> &'static CompiledModule {
    reify_compiler::stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/fdm/correlations")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/fdm/correlations module; available paths: {:?}",
                reify_compiler::stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

/// Look up a structure template by name within the `std/fdm/correlations` module.
fn find_structure(name: &str) -> &'static TopologyTemplate {
    let module = load_stdlib_module();
    module
        .templates
        .iter()
        .find(|t| t.name == name && t.entity_kind == EntityKind::Structure)
        .unwrap_or_else(|| {
            panic!(
                "expected `structure def {}` template in std/fdm/correlations, got templates: {:?}",
                name,
                module
                    .templates
                    .iter()
                    .map(|t| (&t.name, &t.entity_kind))
                    .collect::<Vec<_>>()
            )
        })
}

/// Extract the SI scalar value from a compiled expression. Handles bare
/// numeric literals, dimensioned quantity literals, and compositional BinOp /
/// `some(...)` forms (kept for parity with `fdm_stdlib_compile.rs`).
fn compute_si_value(expr: &CompiledExpr) -> f64 {
    match &expr.kind {
        CompiledExprKind::Literal(Value::Scalar { si_value, .. }) => *si_value,
        CompiledExprKind::Literal(Value::Real(v)) => *v,
        CompiledExprKind::Literal(Value::Int(v)) => *v as f64,
        CompiledExprKind::BinOp { op, left, right } => {
            let l = compute_si_value(left);
            let r = compute_si_value(right);
            match op {
                BinOp::Mul => l * r,
                BinOp::Div => l / r,
                BinOp::Add => l + r,
                BinOp::Sub => l - r,
                other => panic!("compute_si_value: unsupported BinOp {:?}", other),
            }
        }
        CompiledExprKind::OptionSome(inner) => compute_si_value(inner),
        other => panic!("compute_si_value: unsupported expression kind: {:?}", other),
    }
}

/// Assert that the named param cell on `template` carries a Real default with
/// the expected value (1e-9 absolute tolerance for dimensionless).
fn assert_real_default(template: &TopologyTemplate, member: &str, expected: f64) {
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("{} missing param '{}'", template.name, member));
    let expr = cell
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("{}.{} missing default_expr", template.name, member));

    assert_eq!(
        expr.result_type,
        Type::dimensionless_scalar(),
        "{}.{} default result_type should be Real, got: {:?}",
        template.name,
        member,
        expr.result_type
    );

    let actual = compute_si_value(expr);
    let tol = 1e-9_f64;
    assert!(
        (actual - expected).abs() <= tol,
        "{}.{} default value should be {} (within {}), got {}",
        template.name,
        member,
        expected,
        tol,
        actual
    );
}

/// Assert that the named param cell on `template` carries a Bool default with
/// the expected boolean value.
fn assert_bool_default(template: &TopologyTemplate, member: &str, expected: bool) {
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("{} missing param '{}'", template.name, member));

    assert_eq!(
        cell.cell_type,
        Type::Bool,
        "{}.{} cell_type should be Bool, got: {:?}",
        template.name,
        member,
        cell.cell_type
    );

    let expr = cell
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("{}.{} missing default_expr", template.name, member));

    match &expr.kind {
        CompiledExprKind::Literal(Value::Bool(b)) => assert_eq!(
            *b, expected,
            "{}.{} default value should be {}, got {}",
            template.name, member, expected, b
        ),
        other => panic!(
            "{}.{} default should be Literal(Value::Bool({})), got: {:?}",
            template.name, member, expected, other
        ),
    }
}

/// Assert that the named param cell on `template` has the expected declared
/// `cell_type`.
fn assert_param_type(template: &TopologyTemplate, member: &str, expected: &Type) {
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("{} missing param '{}'", template.name, member));
    assert_eq!(
        &cell.cell_type, expected,
        "{}.{} cell_type should be {:?}, got {:?}",
        template.name, member, expected, cell.cell_type
    );
}

/// Assert that the named param cell on `template` is a
/// `MaterialPropertyProvenance` slot carrying a default
/// `MaterialPropertyProvenance(..)` constructor. The exact citation string
/// content is intentionally not pinned (fragile to rewording) — only that the
/// slot exists with the right type and a ctor default.
fn assert_provenance_ctor(template: &TopologyTemplate, member: &str) {
    let cell = template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == member)
        .unwrap_or_else(|| panic!("{} missing param '{}'", template.name, member));

    assert_eq!(
        cell.cell_type,
        Type::StructureRef("MaterialPropertyProvenance".to_string()),
        "{}.{} cell_type should be StructureRef(MaterialPropertyProvenance), got: {:?}",
        template.name,
        member,
        cell.cell_type
    );

    let expr = cell
        .default_expr
        .as_ref()
        .unwrap_or_else(|| panic!("{}.{} missing default_expr", template.name, member));

    match &expr.kind {
        CompiledExprKind::StructureInstanceCtor { type_name, .. } => assert_eq!(
            type_name, "MaterialPropertyProvenance",
            "{}.{} default should be MaterialPropertyProvenance(..), got type_name: {}",
            template.name, member, type_name
        ),
        other => panic!(
            "{}.{} default should be StructureInstanceCtor {{ type_name: \"MaterialPropertyProvenance\", .. }}, got: {:?}",
            template.name, member, other
        ),
    }
}

// ─── step-1: module loads + build-Z / Gibson-Ashby default values ────────────

/// The std/fdm/correlations module must load through the production stdlib
/// path with zero error-severity diagnostics. The loader-level `assert!`
/// already fails fast on Error diagnostics during init, but this test
/// independently asserts the post-init invariant.
#[test]
fn std_fdm_correlations_module_loads_with_no_errors() {
    let module = load_stdlib_module();

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "unexpected error diagnostics in fdm_correlations.ri: {:?}",
        errors
    );
}

/// `FDMCorrelationDefaults` pins the build-Z knockdown ratios and the
/// Gibson-Ashby infill-law coefficients to the PRD §"Built-in property
/// correlations" values:
///   - build_z_modulus_ratio = 0.67  (PLA-calibrated, PMC9828590)
///   - build_z_strength_ratio = 0.52 (PLA-calibrated, PMC9828590)
///   - gibson_ashby_c = 1.0          (Gibson & Ashby 1997 open-cell foam)
///   - gibson_ashby_n = 2.0          (FDM-specific exponent)
///
/// These are closed-form definitions of the correlation, not guessed
/// thresholds; the magnitudes are specified by the task with citations.
#[test]
fn fdm_correlation_defaults_build_z_and_gibson_ashby_values() {
    let template = find_structure("FDMCorrelationDefaults");

    assert_real_default(template, "build_z_modulus_ratio", 0.67);
    assert_real_default(template, "build_z_strength_ratio", 0.52);
    assert_real_default(template, "gibson_ashby_c", 1.0);
    assert_real_default(template, "gibson_ashby_n", 2.0);
}

// ─── step-3: pattern-factor defaults + low-confidence flags ──────────────────

/// `FDMCorrelationDefaults` pins the infill-pattern directional factors and
/// the parallel `..._low_confidence : Bool` flags (PRD §"Built-in property
/// correlations": gyroid/cubic near-isotropic; grid/triangular/honeycomb
/// directional; FDM-specific Gibson-Ashby exponent + directional pattern
/// factors flagged low-confidence).
///
/// Pattern factors:
///   - pattern_near_isotropic_factor = 1.0 (gyroid/cubic ≈ in-plane isotropic)
///   - pattern_directional_strong_factor = 1.0 (along the raster lines)
///   - pattern_directional_weak_factor = 0.6 (transverse to the raster lines)
///
/// The strong > weak ordering is what makes the orthotropic E1 > E2 split; the
/// magnitudes are flagged low-confidence (no PRD-pinned value).
///
/// Low-confidence contract (machine-checkable, not buried prose):
///   false for the PLA-calibrated build-Z ratio and the standard Gibson-Ashby
///   C / near-isotropic factor; true for the FDM-specific Gibson-Ashby
///   exponent n and the directional pattern factors.
#[test]
fn fdm_correlation_defaults_pattern_factors_and_low_confidence_flags() {
    let template = find_structure("FDMCorrelationDefaults");

    // Pattern-factor Real defaults.
    assert_real_default(template, "pattern_near_isotropic_factor", 1.0);
    assert_real_default(template, "pattern_directional_strong_factor", 1.0);
    assert_real_default(template, "pattern_directional_weak_factor", 0.6);

    // Each pattern factor carries a MaterialPropertyProvenance citation slot.
    assert_provenance_ctor(template, "pattern_near_isotropic_factor_provenance");
    assert_provenance_ctor(template, "pattern_directional_strong_factor_provenance");
    assert_provenance_ctor(template, "pattern_directional_weak_factor_provenance");

    // Low-confidence flags: false for well-calibrated defaults …
    assert_bool_default(template, "build_z_modulus_ratio_low_confidence", false);
    assert_bool_default(template, "build_z_strength_ratio_low_confidence", false);
    assert_bool_default(template, "gibson_ashby_c_low_confidence", false);
    assert_bool_default(template, "pattern_near_isotropic_factor_low_confidence", false);

    // … true for the FDM-specific exponent and directional pattern factors.
    assert_bool_default(template, "gibson_ashby_n_low_confidence", true);
    assert_bool_default(template, "pattern_directional_strong_factor_low_confidence", true);
    assert_bool_default(template, "pattern_directional_weak_factor_low_confidence", true);
}

// ─── step-5: FDMCouponOverride shape + subset-override compile probe ──────────

/// `FDMCouponOverride` is the user-facing coupon-override entry point. It
/// carries optional measured elastic constants (ex/ey/ez/gxy : Option<Pressure>)
/// and optional Gibson-Ashby infill-curve overrides (infill_gibson_ashby_c /
/// infill_gibson_ashby_n : Option<Real>). Any set field beats the corresponding
/// FDMCorrelationDefaults default (PRD §"Built-in property correlations":
/// "a user supplies measured coupon data … to override any constant").
///
/// Test pins the (name, type) of each override field. Defaults (= none) are
/// not pinned here — the subset-ctor probe below exercises the none-default path.
#[test]
fn fdm_coupon_override_has_optional_override_fields() {
    let template = find_structure("FDMCouponOverride");

    let pressure_opt = Type::Option(Box::new(Type::Scalar {
        dimension: DimensionVector::PRESSURE,
    }));
    let real_opt = Type::Option(Box::new(Type::dimensionless_scalar()));

    assert_param_type(template, "ex", &pressure_opt);
    assert_param_type(template, "ey", &pressure_opt);
    assert_param_type(template, "ez", &pressure_opt);
    assert_param_type(template, "gxy", &pressure_opt);
    assert_param_type(template, "infill_gibson_ashby_c", &real_opt);
    assert_param_type(template, "infill_gibson_ashby_n", &real_opt);
}

/// The coupon-override surface must support partial construction: a user
/// overrides a subset of constants (here ex + the infill-curve exponent) and
/// leaves the rest to default to `none`. This is the user-observable signal
/// for the override entry point — it must compile cleanly through the stdlib
/// prelude path. Mirrors `fdm_stdlib_compile.rs::prd_motivating_example_…`.
#[test]
fn fdm_coupon_override_subset_ctor_compiles_cleanly() {
    let source = r#"
structure def TestCoupon {
    let coupon = FDMCouponOverride(
        ex: some(4GPa),
        infill_gibson_ashby_n: some(1.8)
    )
}
"#;
    let compiled = compile_source_with_stdlib(source);

    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "FDMCouponOverride subset ctor should compile without errors; got: {:?}",
        errors
    );
}
