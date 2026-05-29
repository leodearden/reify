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
        Type::Real,
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
