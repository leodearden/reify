//! End-to-end integration gate for the full §11 std.fields surface.
//!
//! Consolidates the five per-symbol worked examples (fn_field.ri, from_samples.ri,
//! spatial_ops.ri, compose.ri, restrict.ri) into a single file-level smoke test
//! (task η #4225, PRD docs/prds/v0_6/std-fields-api.md §6/§8).
//!
//! Boundary tests covered:
//!   B1  — sample(fn_field(|p| 2.0*p), 3.0) == 6.0
//!   B2  — sample(from_samples([0,1,2],[0,10,20], InterpolationMethod.Linear), 0.5) == 5.0
//!   B3  — non-uniform spacing → DiagnosticCode::FieldSamplesNotGrid at Severity::Error
//!   B4  — InterpolationMethod.RBF → DiagnosticCode::InterpMethodUnsupported at Severity::Error
//!   B5  — restrict(base_field, region) inside→42.0, outside→Undef (OCCT-gated)
//!   B6  — sample(constant_field(42.0), 0.0) == 42.0
//!   B7  — sample(clamp_field(constant_field(250.0), 10.0, 200.0), 0.0) == 200.0
//!          sample(remap_field(constant_field(50.0), 0.0, 100.0, 0.0, 200.0), 0.0) == 100.0
//!   B8  — threshold(constant_field(250MPa), 200MPa) → true, threshold(150MPa,200MPa) → false
//!   B9  — sample(compose(f_double, g_plus1), 3.0) == 8.0
//!   B10 — sample(clamp_field(fn_field(|p| 2.0*p), 10.0, 200.0), 3.0) == 10.0 (clamp below lo)
//!
//! B3/B4 are tested via inline source strings (cannot live in a green .ri file —
//! they produce hard Error diagnostics). B5 is OCCT-gated (restrict containment
//! returns Indeterminate in OCCT-less lanes). B1/B2/B6/B7/B8/B9/B10 are constraint
//! gates in examples/fields/std_fields_surface.ri and asserted all-Satisfied here.
//!
//! Model: compose_example_smoke.rs (eval/check all-Satisfied) +
//!        from_samples_example_smoke.rs (inline-source B3/B4 diagnostics) +
//!        fields_restrict_e2e.rs (OCCT-gated cell lookup).

use reify_compiler::CompiledModule;
use reify_constraints::SimpleConstraintChecker;
use reify_core::{DiagnosticCode, Severity, ValueCellId};
use reify_ir::{ExportFormat, Satisfaction, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

/// Absolute path to the integration fixture, resolved at compile time.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/fields/std_fields_surface.ri"
);

/// Read the fixture and compile it with the stdlib, asserting no error
/// diagnostics. Returns the compiled program for further use.
///
/// Panics if the example file is absent (RED signal when step-2 is not yet done).
fn compile_surface_fixture() -> CompiledModule {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/fields/std_fields_surface.ri should exist (task η #4225 step-2)");

    let compiled = parse_and_compile_with_stdlib(&source);

    assert!(
        errors_only(&compiled).is_empty(),
        "expected no compile errors in std_fields_surface.ri, got: {:?}",
        errors_only(&compiled)
    );

    compiled
}

// ── (a) Compile-clean signal ─────────────────────────────────────────────────

/// Compile `examples/fields/std_fields_surface.ri` and verify it has no
/// error-severity diagnostics (compile-clean signal for all §11 symbols).
///
/// **RED before step-2**: panics on `read_to_string` — example file is absent.
/// **GREEN after step-2**: example file exists and compiles clean.
#[test]
fn std_fields_surface_compiles_clean() {
    compile_surface_fixture();
}

// ── (b) All constraint gates satisfied (B1/B2/B6/B7/B8/B9/B10) ─────────────

/// Eval and check `examples/fields/std_fields_surface.ri` and verify every
/// structure constraint is `Satisfaction::Satisfied`.
///
/// The `StdFieldsSurface` structure declares at least 9 constraint gates:
///   - B1: `doubled_at_3 > 5.999` and `doubled_at_3 < 6.001`
///   - B2: `v_linear > 4.999` and `v_linear < 5.001`
///   - B6: `c0 > 41.999` and `c0 < 42.001`
///   - B7: `clamped_real > 199.999` and `clamped_real < 200.001`
///     `remapped > 99.999` and `remapped < 100.001`
///   - B7 Pressure: `clamped_pressure > 199.99MPa` and `clamped_pressure < 200.01MPa`
///   - B8: `above` (true) and `!below` (true)
///   - B9: `via_compose > 7.999` and `via_compose < 8.001`
///   - B10: `chained > 9.999` and `chained < 10.001`
///
/// The exact count is asserted `>= 9` so that adding illustrative constraints
/// later doesn't break this test — the per-entry Satisfied loop is the real signal.
///
/// **RED before step-2**: panics reading the missing file.
/// **GREEN after step-2**: all constraints Satisfied.
#[test]
fn std_fields_surface_constraints_all_satisfied() {
    let compiled = compile_surface_fixture();

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // No eval-level errors.
    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {:?}", eval_errors);

    // At least 9 constraint results, all Satisfied.
    let check = engine.check(&compiled);
    assert!(
        check.constraint_results.len() >= 9,
        "expected at least 9 constraint results (B1/B2/B6/B7/B8/B9/B10), got {}",
        check.constraint_results.len()
    );

    for entry in &check.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied (§11 surface gate), got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}

// ── (c) B3 — non-grid spacing produces FieldSamplesNotGrid error ─────────────

/// B3 via inline source: `from_samples` with non-uniform spacing must produce
/// `DiagnosticCode::FieldSamplesNotGrid` at `Severity::Error`.
///
/// This is the same assertion as `from_samples_example_smoke.rs::
/// from_samples_non_grid_surfaces_field_samples_not_grid_in_check_b3`, reproduced
/// here as part of the consolidation gate so the full surface is covered in a
/// single test crate.
///
/// **GREEN immediately**: `DiagnosticCode::FieldSamplesNotGrid` landed in γ (#4221).
#[test]
fn std_fields_surface_b3_non_grid_errors() {
    let source = r#"
structure def B3NonGridDemo {
    let v = sample(from_samples([0.0, 1.0, 5.0], [0.0, 10.0, 20.0], InterpolationMethod.Linear), 0.5)
    constraint v > 4.999
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "inline source should compile clean; got: {:?}",
        errors_only(&compiled)
    );

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    assert!(
        result.diagnostics.iter().any(|d| {
            d.code == Some(DiagnosticCode::FieldSamplesNotGrid) && d.severity == Severity::Error
        }),
        "result.diagnostics must contain FieldSamplesNotGrid Error (B3); got: {:?}",
        result.diagnostics
    );
}

// ── (d) B4 — RBF method produces InterpMethodUnsupported error ───────────────

/// B4 via inline source: `InterpolationMethod.RBF` passed to `from_samples` must
/// produce `DiagnosticCode::InterpMethodUnsupported` at `Severity::Error`.
///
/// This mirrors `from_samples_example_smoke.rs::
/// from_samples_rbf_method_surfaces_interp_method_unsupported_in_check_b4`.
///
/// **GREEN immediately**: `DiagnosticCode::InterpMethodUnsupported` landed in γ (#4221).
#[test]
fn std_fields_surface_b4_rbf_unsupported() {
    let source = r#"
structure def B4RbfDemo {
    let v = sample(from_samples([0.0, 1.0, 2.0], [0.0, 10.0, 20.0], InterpolationMethod.RBF), 0.5)
    constraint v > 4.999
}
"#;
    let compiled = parse_and_compile_with_stdlib(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "inline source should compile clean; got: {:?}",
        errors_only(&compiled)
    );

    let checker = SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    assert!(
        result.diagnostics.iter().any(|d| {
            d.code == Some(DiagnosticCode::InterpMethodUnsupported) && d.severity == Severity::Error
        }),
        "result.diagnostics must contain InterpMethodUnsupported Error (B4); got: {:?}",
        result.diagnostics
    );
}

// ── B5: restrict (OCCT-gated) ────────────────────────────────────────────────

/// B5 via cell-lookup: load `examples/fields/std_fields_surface.ri`, build with
/// a real OCCT kernel, and assert:
///   - `RestrictSurface.v_in`  == `Value::Real(42.0)` (inside the box → field value)
///   - `RestrictSurface.v_out` == `Value::Undef`       (outside the box → strict-Undef)
///
/// Compile-clean is asserted unconditionally (part of step-3 RED: the
/// `RestrictSurface` structure is absent before step-4, so the cell lookups are
/// `None` → assert fails under OCCT).
///
/// Skips OCCT-dependent cell assertions when OCCT is unavailable.
/// Reuses the exact harness of `fields_restrict_e2e.rs::restrict_field_b5_integration`.
#[test]
fn std_fields_surface_b5_restrict_occt_gated() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/fields/std_fields_surface.ri should exist");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/fields/std_fields_surface.ri should compile with no errors, got:\n{:#?}",
        errors_only(&compiled)
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return;
    }

    // Build with real OCCT kernel.
    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // v_in: inside the box (0mm,0mm,0mm) → inner field value 42.0.
    let v_in_cell = ValueCellId::new("RestrictSurface", "v_in");
    let v_in = result.values.get(&v_in_cell).cloned();
    assert_eq!(
        v_in,
        Some(Value::Real(42.0)),
        "RestrictSurface.v_in (inside the box) should be Value::Real(42.0), got: {:?}",
        v_in
    );

    // v_out: outside the box (20mm,0mm,0mm) → Value::Undef.
    let v_out_cell = ValueCellId::new("RestrictSurface", "v_out");
    let v_out = result.values.get(&v_out_cell).cloned();
    assert_eq!(
        v_out,
        Some(Value::Undef),
        "RestrictSurface.v_out (outside the box) should be Value::Undef, got: {:?}",
        v_out
    );
}
