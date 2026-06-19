//! M8.4 stdlib integration tests.
//!
//! Exercises three stdlib modules — linalg, fields_analysis, io_export —
//! through the full parse → compile_with_stdlib → eval pipeline using
//! .ri fixture files in examples/.
//!
//! Follows the same `eval_ri_file` pattern as m8_3_stdlib_integration.rs:
//! parse → compile_with_stdlib → eval with `SimpleConstraintChecker`.
//! The three fixtures exercise:
//!   linalg.ri         — advanced matrix ops (outer, determinant, inverse, transpose,
//!                        eigenvalues) + complex number builtins
//!   fields_analysis.ri — analytical field defs (sample/gradient) + analysis
//!                        builtins (von_mises, safety_factor) on tensors
//!   io_export.ri       — Physical+Elastic+Strong trait conformance, tolerancing
//!                        subs, and geometry (box) for export-ready parts

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DimensionVector, Severity, ValueCellId};
use reify_ir::{ExportFormat, Value};
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── File paths (resolved at compile time from this crate's root) ─────────────

const PATH_LINALG: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/linalg.ri");

const PATH_FIELDS_ANALYSIS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/fields_analysis.ri"
);

const PATH_IO_EXPORT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/io_export.ri");

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Read a .ri fixture file, parse, compile with stdlib (asserting no
/// Severity::Error diagnostics at each stage), eval with
/// `SimpleConstraintChecker`, and assert no eval errors.
/// Returns the full `EvalResult` for per-test assertions.
fn eval_ri_file(path: &str, module_name: &str) -> reify_eval::EvalResult {
    let source =
        std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{} should exist: {}", path, e));

    let parsed = reify_syntax::parse(&source, reify_core::ModulePath::single(module_name));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in {}: {:?}",
        path,
        parsed.errors
    );

    let compiled = reify_compiler::compile_with_stdlib(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "compile errors in {}: {:?}",
        path,
        compile_errors
    );

    let mut engine = make_simple_engine();
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "eval errors in {}: {:?}",
        path,
        eval_errors
    );

    result
}

/// Extract a numeric `f64` from `Value::Real` or `Value::Int`.
/// Panics with a descriptive message including `label` for any other variant.
fn expect_real_or_int(val: &Value, label: &str) -> f64 {
    match val {
        Value::Real(v) => *v,
        Value::Int(i) => *i as f64,
        other => panic!("{label} should be Real or Int, got {other:?}"),
    }
}

/// Assert that the `Value::Scalar` stored at `entity.member` in `result` matches
/// `expected` within `tol` and has dimension `dim`.
fn assert_scalar(
    result: &reify_eval::EvalResult,
    entity: &str,
    member: &str,
    expected: f64,
    tol: f64,
    dim: DimensionVector,
) {
    let id = ValueCellId::new(entity, member);
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("{}.{} not found in eval result", entity, member));
    match val {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert!(
                (si_value - expected).abs() < tol,
                "{}.{} should be ≈{} (tol {}), got {}",
                entity,
                member,
                expected,
                tol,
                si_value
            );
            assert_eq!(*dimension, dim, "{}.{} dimension mismatch", entity, member);
        }
        other => panic!(
            "{}.{} should be Value::Scalar, got {:?}",
            entity, member, other
        ),
    }
}

/// Assert that the `Value::Complex` stored at `entity.member` in `result` matches
/// `expected_re`/`expected_im` within `tol` and has dimension `dim`.
fn assert_complex(
    result: &reify_eval::EvalResult,
    entity: &str,
    member: &str,
    expected_re: f64,
    expected_im: f64,
    tol: f64,
    dim: DimensionVector,
) {
    let id = ValueCellId::new(entity, member);
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("{}.{} not found in eval result", entity, member));
    match val {
        Value::Complex { re, im, dimension } => {
            assert!(
                (re - expected_re).abs() < tol,
                "{}.{}.re should be ≈{} (tol {}), got {}",
                entity,
                member,
                expected_re,
                tol,
                re
            );
            assert!(
                (im - expected_im).abs() < tol,
                "{}.{}.im should be ≈{} (tol {}), got {}",
                entity,
                member,
                expected_im,
                tol,
                im
            );
            assert_eq!(*dimension, dim, "{}.{} dimension mismatch", entity, member);
        }
        other => panic!(
            "{}.{} should be Value::Complex, got {:?}",
            entity, member, other
        ),
    }
}

// ── OCCT-build helpers (GHR-ζ mass-revival) ──────────────────────────────────

/// Compile `source` with stdlib (asserting no error-severity diagnostics), then —
/// if OCCT is available — build through a real-OCCT `Engine` and return the
/// `BuildResult`. Returns `None` when OCCT is unavailable (caller skips numeric
/// assertions). Mirrors `compile_and_build_occt` in geometry_query_kernel_dispatch.rs.
fn compile_and_build_occt(source: &str) -> Option<reify_eval::BuildResult> {
    let compiled = parse_and_compile_with_stdlib(source);
    {
        let errs: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errs.is_empty(),
            "fixture should compile with no error-severity diagnostics, got:\n{:#?}",
            errs
        );
    }
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return None;
    }
    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    Some(engine.build(&compiled, ExportFormat::Step))
}

/// Assert `value` is a `Value::Scalar` of dimension `dim` whose `si_value` is
/// within 1e-6 relative of `expected`.
fn assert_scalar_rel(value: Option<&Value>, dim: DimensionVector, expected: f64, what: &str) {
    match value {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert_eq!(
                *dimension, dim,
                "{what}: expected dimension {dim:?}, got {dimension:?}"
            );
            let rel = (si_value - expected).abs() / expected.abs().max(f64::MIN_POSITIVE);
            assert!(
                rel < 1e-6,
                "{what}: si_value {si_value:.12} not within 1e-6 relative of \
                 {expected:.12} (rel={rel:.3e})"
            );
        }
        other => panic!("{what}: expected Value::Scalar{{{dim:?}}}, got {other:?}"),
    }
}

/// Read the runtime `density` (SI kg·m⁻³) from a structure's evaluated
/// `material` StructureInstance cell. Lets expected mass track the actual
/// material constant rather than a hardcoded literal.
fn material_density_si(result: &reify_eval::BuildResult, structure: &str) -> f64 {
    match result.values.get(&ValueCellId::new(structure, "material")) {
        Some(Value::StructureInstance(data)) => match data.fields.get("density") {
            Some(Value::Scalar { si_value, .. }) => *si_value,
            other => panic!("{structure}.material.density should be Scalar, got {other:?}"),
        },
        other => panic!("{structure}.material should be StructureInstance, got {other:?}"),
    }
}

// ── Section 1: linalg.ri ─────────────────────────────────────────────────────

/// Smoke test: linalg.ri parses, compiles (stdlib), evals without errors,
/// and produces non-empty values.
#[test]
fn linalg_smoke() {
    let result = eval_ri_file(PATH_LINALG, "linalg");
    assert!(
        !result.values.is_empty(),
        "linalg.ri eval should produce non-empty values"
    );
}

// ── step-3: linalg_rotation_det_is_one ───────────────────────────────────────

/// Asserts LinalgDemo.det_rot ≈ 1.0 (Value::Real).
/// R_z(90°) = [[0,-1,0],[1,0,0],[0,0,1]] has exact determinant 1.
#[test]
fn linalg_rotation_det_is_one() {
    let result = eval_ri_file(PATH_LINALG, "linalg");

    let det_id = ValueCellId::new("LinalgDemo", "det_rot");
    let det_val = result
        .values
        .get(&det_id)
        .unwrap_or_else(|| panic!("LinalgDemo.det_rot not found in eval result"));

    match det_val {
        Value::Real(v) => {
            assert!(
                (v - 1.0).abs() < 1e-9,
                "LinalgDemo.det_rot should be ≈1.0 (det of rotation matrix), got {}",
                v
            );
        }
        other => panic!("LinalgDemo.det_rot should be Value::Real, got {:?}", other),
    }
}

// ── step-3: linalg_eigenvalues_of_diagonal ───────────────────────────────────

/// Asserts LinalgDemo.eig = eigenvalues(diag(3,5,7)) = sorted [3.0, 5.0, 7.0]
/// (Value::List of Value::Real).
#[test]
fn linalg_eigenvalues_of_diagonal() {
    let result = eval_ri_file(PATH_LINALG, "linalg");

    let eig_id = ValueCellId::new("LinalgDemo", "eig");
    let eig_val = result
        .values
        .get(&eig_id)
        .unwrap_or_else(|| panic!("LinalgDemo.eig not found in eval result"));

    match eig_val {
        Value::List(items) => {
            assert_eq!(
                items.len(),
                3,
                "eigenvalues should have 3 entries, got {}",
                items.len()
            );
            // `compute_eigenvalues_3x3` returns eigenvalues sorted ascending.
            // We independently sort the extracted floats before comparison so
            // the test stays robust if that sort-order contract ever changes.
            let expected = [3.0_f64, 5.0, 7.0];
            let mut actuals: Vec<f64> = items
                .iter()
                .enumerate()
                .map(|(i, item)| expect_real_or_int(item, &format!("eigenvalue[{i}]")))
                .collect();
            actuals.sort_by(|a, b| a.total_cmp(b));
            for (i, (&actual, &exp)) in actuals.iter().zip(expected.iter()).enumerate() {
                assert!(
                    (actual - exp).abs() < 1e-9,
                    "eigenvalue[{}]: expected {}, got {}",
                    i,
                    exp,
                    actual
                );
            }
        }
        other => panic!("LinalgDemo.eig should be Value::List, got {:?}", other),
    }
}

// ── step-3: linalg_inverse_equals_transpose ──────────────────────────────────

/// Spot-checks that LinalgDemo.inv_rot and LinalgDemo.trans_rot have matching
/// [0][0] elements. For R_z(90°) the [0][0] element of both R^-1 and R^T is 0.
/// This verifies the orthogonal matrix property: R^{-1} = R^T.
#[test]
fn linalg_inverse_equals_transpose() {
    let result = eval_ri_file(PATH_LINALG, "linalg");

    let inv_id = ValueCellId::new("LinalgDemo", "inv_rot");
    let inv_val = result
        .values
        .get(&inv_id)
        .unwrap_or_else(|| panic!("LinalgDemo.inv_rot not found"));

    let trans_id = ValueCellId::new("LinalgDemo", "trans_rot");
    let trans_val = result
        .values
        .get(&trans_id)
        .unwrap_or_else(|| panic!("LinalgDemo.trans_rot not found"));

    // Helper: extract [row][col] element from a nested Tensor
    fn tensor_elem(v: &Value, row: usize, col: usize) -> f64 {
        match v {
            Value::Tensor(rows) => match &rows[row] {
                Value::Tensor(cols) => match &cols[col] {
                    Value::Real(x) => *x,
                    Value::Int(i) => *i as f64,
                    other => panic!("tensor element should be Real or Int, got {:?}", other),
                },
                other => panic!("tensor row should be Tensor, got {:?}", other),
            },
            other => panic!("expected Tensor, got {:?}", other),
        }
    }

    // For R_z(90°): R = [[0,-1,0],[1,0,0],[0,0,1]]
    // R^T = R^-1 = [[0,1,0],[-1,0,0],[0,0,1]]  (orthogonal matrix property)
    //
    // Check all 9 elements: (a) inv_rot == trans_rot at every position, and
    // (b) trans_rot matches the known analytical values of R^T.
    // A 2-element spot-check could pass by coincidence (e.g. both zero at [0][0]).
    let expected_rt: [[f64; 3]; 3] = [[0.0, 1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    for (row, expected_row) in expected_rt.iter().enumerate() {
        for (col, &exp) in expected_row.iter().enumerate() {
            let inv_elem = tensor_elem(inv_val, row, col);
            let trans_elem = tensor_elem(trans_val, row, col);
            assert!(
                (inv_elem - trans_elem).abs() < 1e-9,
                "inv_rot[{}][{}] ({}) != trans_rot[{}][{}] ({}): R^-1 should equal R^T",
                row,
                col,
                inv_elem,
                row,
                col,
                trans_elem
            );
            assert!(
                (trans_elem - exp).abs() < 1e-9,
                "trans_rot[{}][{}] should be ≈{}, got {}",
                row,
                col,
                exp,
                trans_elem
            );
        }
    }
}

// ── step-3: linalg_complex_re_im ─────────────────────────────────────────────

/// Asserts LinalgDemo.z_re ≈ 3.0 and LinalgDemo.z_im ≈ 4.0 (both Value::Real,
/// since z = complex(3.0, 4.0) is dimensionless).
#[test]
fn linalg_complex_re_im() {
    let result = eval_ri_file(PATH_LINALG, "linalg");

    // re(z) → Value::Real(3.0) for dimensionless complex
    let re_id = ValueCellId::new("LinalgDemo", "z_re");
    let re_val = result
        .values
        .get(&re_id)
        .unwrap_or_else(|| panic!("LinalgDemo.z_re not found"));
    match re_val {
        Value::Real(v) => assert!((v - 3.0).abs() < 1e-9, "z_re should be ≈3.0, got {}", v),
        other => panic!("LinalgDemo.z_re should be Value::Real, got {:?}", other),
    }

    // im(z) → Value::Real(4.0) for dimensionless complex
    let im_id = ValueCellId::new("LinalgDemo", "z_im");
    let im_val = result
        .values
        .get(&im_id)
        .unwrap_or_else(|| panic!("LinalgDemo.z_im not found"));
    match im_val {
        Value::Real(v) => assert!((v - 4.0).abs() < 1e-9, "z_im should be ≈4.0, got {}", v),
        other => panic!("LinalgDemo.z_im should be Value::Real, got {:?}", other),
    }
}

// ── step-3: linalg_complex_magnitude ─────────────────────────────────────────

/// Asserts LinalgDemo.z_mag = complex_magnitude(z) ≈ 5.0 (Value::Real).
/// z = complex(3.0, 4.0) → |z| = sqrt(9+16) = 5.0.
#[test]
fn linalg_complex_magnitude() {
    let result = eval_ri_file(PATH_LINALG, "linalg");

    let mag_id = ValueCellId::new("LinalgDemo", "z_mag");
    let mag_val = result
        .values
        .get(&mag_id)
        .unwrap_or_else(|| panic!("LinalgDemo.z_mag not found"));

    match mag_val {
        Value::Real(v) => {
            assert!(
                (v - 5.0).abs() < 1e-9,
                "z_mag should be ≈5.0 (|3+4i| = 5), got {}",
                v
            );
        }
        other => panic!("LinalgDemo.z_mag should be Value::Real, got {:?}", other),
    }
}

// ── step-1: linalg_complex_conjugate ─────────────────────────────────────────

/// Asserts LinalgDemo.z_conj = conjugate(z) = Complex{re:3.0, im:-4.0, DIMENSIONLESS}.
/// z = complex(3.0, 4.0) → conjugate flips the sign of the imaginary part.
#[test]
fn linalg_complex_conjugate() {
    let result = eval_ri_file(PATH_LINALG, "linalg");
    assert_complex(
        &result,
        "LinalgDemo",
        "z_conj",
        3.0,
        -4.0,
        1e-9,
        DimensionVector::DIMENSIONLESS,
    );
}

// ── step-2: linalg_complex_phase ─────────────────────────────────────────────

/// Asserts LinalgDemo.z_phase = phase(z) ≈ atan2(4,3) ≈ 0.9273 rad (Value::Scalar, ANGLE).
/// z = complex(3.0, 4.0) → phase = atan2(im, re) = atan2(4, 3).
#[test]
fn linalg_complex_phase() {
    let result = eval_ri_file(PATH_LINALG, "linalg");
    let expected = (4.0_f64).atan2(3.0);
    assert_scalar(
        &result,
        "LinalgDemo",
        "z_phase",
        expected,
        1e-9,
        DimensionVector::ANGLE,
    );
}

// ── Section 2: fields_analysis.ri ────────────────────────────────────────────

/// Smoke test: fields_analysis.ri parses, compiles (stdlib), evals without
/// errors, and produces non-empty values.
#[test]
fn fields_analysis_smoke() {
    let result = eval_ri_file(PATH_FIELDS_ANALYSIS, "fields_analysis");
    assert!(
        !result.values.is_empty(),
        "fields_analysis.ri eval should produce non-empty values"
    );
}

// ── step-7: fields_temperature_sample ────────────────────────────────────────

/// Asserts FieldsAnalysisDemo.temp_at_5 = sample(temperature, 5.0) ≈ 45.0.
/// temperature(x) = x*x + 20.0, so temperature(5.0) = 25 + 20 = 45.0.
#[test]
fn fields_temperature_sample() {
    let result = eval_ri_file(PATH_FIELDS_ANALYSIS, "fields_analysis");

    let id = ValueCellId::new("FieldsAnalysisDemo", "temp_at_5");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("FieldsAnalysisDemo.temp_at_5 not found"));

    let v = expect_real_or_int(val, "FieldsAnalysisDemo.temp_at_5");
    assert!(
        (v - 45.0).abs() < 1e-9,
        "temp_at_5 should be ≈45.0 (= 5²+20), got {}",
        v
    );
}

// ── step-7: fields_temperature_gradient ──────────────────────────────────────

/// Asserts FieldsAnalysisDemo.dtemp_at_5 = sample(gradient(temperature), 5.0) ≈ 10.0.
/// gradient(x*x+20) = 2*x, so at x=5: derivative ≈ 10.0 (central differences).
#[test]
fn fields_temperature_gradient() {
    let result = eval_ri_file(PATH_FIELDS_ANALYSIS, "fields_analysis");

    let id = ValueCellId::new("FieldsAnalysisDemo", "dtemp_at_5");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("FieldsAnalysisDemo.dtemp_at_5 not found"));

    let v = expect_real_or_int(val, "FieldsAnalysisDemo.dtemp_at_5");
    assert!(
        (v - 10.0).abs() < 1e-4,
        "dtemp_at_5 should be ≈10.0 (= 2*5, central differences), got {}",
        v
    );
}

// ── step-7: fields_von_mises_uniaxial ────────────────────────────────────────

/// Asserts FieldsAnalysisDemo.vm_stress = von_mises(sigma) ≈ 100.0.
/// sigma = 100.0 * outer(e1, e1) is a uniaxial stress tensor; von Mises = σ.
#[test]
fn fields_von_mises_uniaxial() {
    let result = eval_ri_file(PATH_FIELDS_ANALYSIS, "fields_analysis");

    let id = ValueCellId::new("FieldsAnalysisDemo", "vm_stress");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("FieldsAnalysisDemo.vm_stress not found"));

    let v = expect_real_or_int(val, "FieldsAnalysisDemo.vm_stress");
    assert!(
        (v - 100.0).abs() < 1e-9,
        "vm_stress should be ≈100.0 (uniaxial von Mises = σ), got {}",
        v
    );
}

// ── step-7: fields_safety_factor ─────────────────────────────────────────────

/// Asserts FieldsAnalysisDemo.sf = safety_factor(sigma, 250.0) ≈ 2.5.
/// yield_strength=250, von_mises(sigma)=100 → SF = 250/100 = 2.5 (dimensionless Real).
#[test]
fn fields_safety_factor() {
    let result = eval_ri_file(PATH_FIELDS_ANALYSIS, "fields_analysis");

    let id = ValueCellId::new("FieldsAnalysisDemo", "sf");
    let val = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("FieldsAnalysisDemo.sf not found"));

    let v = expect_real_or_int(val, "FieldsAnalysisDemo.sf");
    assert!(
        (v - 2.5).abs() < 1e-9,
        "sf should be ≈2.5 (= 250/100), got {}",
        v
    );
}

// ── Section 3: io_export.ri ───────────────────────────────────────────────────

/// Smoke test: io_export.ri parses, compiles (stdlib), evals without errors,
/// and produces non-empty values.
#[test]
fn io_export_smoke() {
    let result = eval_ri_file(PATH_IO_EXPORT, "io_export");
    assert!(
        !result.values.is_empty(),
        "io_export.ri eval should produce non-empty values"
    );
}

// ── step-11: io_export_mass_computed ─────────────────────────────────────────

/// `ExportPart.mass` folds via the landed GHR-ζ dispatch (task 3608):
/// `mass = volume(geometry) * material.density`, where
///   - `geometry = box(100mm, 50mm, 10mm)` → analytic volume 5e-5 m³
///   - `material.density ≈ 7850 kg/m³` (runtime-read from the `material` StructureInstance slot)
///     → `mass ≈ 0.3925 kg` (`Value::Scalar<MASS>`, rel < 1e-6).
///
/// Requires the real-OCCT `Engine::build()` path — `post_process_geometry_queries` runs
/// only on the build path with a registered kernel. Skips with zero numeric coverage when
/// OCCT is unavailable; CI must have `/opt/reify-deps` configured for this test to verify.
#[test]
fn io_export_mass_computed() {
    let source = std::fs::read_to_string(PATH_IO_EXPORT)
        .unwrap_or_else(|e| panic!("{} should exist: {}", PATH_IO_EXPORT, e));
    let Some(result) = compile_and_build_occt(&source) else {
        return;
    };
    let box_v = 0.100 * 0.050 * 0.010; // 5e-5 m³: box(100mm, 50mm, 10mm)
    let density = material_density_si(&result, "ExportPart");
    assert_scalar_rel(
        result.values.get(&ValueCellId::new("ExportPart", "mass")),
        DimensionVector::MASS,
        box_v * density,
        "ExportPart.mass",
    );
}

// ── step-11: io_export_tolerance_upper_limit ─────────────────────────────────

/// Asserts ExportPart.tol.upper_limit ≈ 0.10005m (100mm + 0.05mm).
/// DimensionalTolerance: nominal=100mm, upper_deviation=+0.05mm
/// upper_limit = nominal + upper_deviation = 0.100 + 0.00005 = 0.10005m.
#[test]
fn io_export_tolerance_upper_limit() {
    let result = eval_ri_file(PATH_IO_EXPORT, "io_export");
    assert_scalar(
        &result,
        "ExportPart.tol",
        "upper_limit",
        0.10005,
        1e-9,
        DimensionVector::LENGTH,
    );
}

// ── step-11: io_export_tolerance_band ────────────────────────────────────────

/// Asserts ExportPart.tol.tolerance_band ≈ 0.0001m (0.1mm = 2×0.05mm).
/// DimensionalTolerance: upper_deviation=+0.05mm, lower_deviation=-0.05mm
/// tolerance_band = upper_deviation - lower_deviation = 0.05mm - (-0.05mm) = 0.1mm.
#[test]
fn io_export_tolerance_band() {
    let result = eval_ri_file(PATH_IO_EXPORT, "io_export");
    assert_scalar(
        &result,
        "ExportPart.tol",
        "tolerance_band",
        0.0001,
        1e-12,
        DimensionVector::LENGTH,
    );
}

// ── step-3: io_export_flatness_tolerance ─────────────────────────────────────

/// Asserts ExportPart.flat.tolerance_value ≈ 0.00002m (0.02mm in SI).
/// Flatness sub: tolerance_value: 0.02mm → 2e-5 m.
#[test]
fn io_export_flatness_tolerance() {
    let result = eval_ri_file(PATH_IO_EXPORT, "io_export");
    assert_scalar(
        &result,
        "ExportPart.flat",
        "tolerance_value",
        0.00002,
        1e-12,
        DimensionVector::LENGTH,
    );
}

// ── step-4: io_export_surface_finish ─────────────────────────────────────────

/// Asserts ExportPart.finish.value ≈ 8e-7m (Ra 0.8 μm per ISO 1302).
/// SurfaceFinish sub: value: 0.8um → 0.8e-6 m = 8e-7 m.
#[test]
fn io_export_surface_finish() {
    let result = eval_ri_file(PATH_IO_EXPORT, "io_export");
    assert_scalar(
        &result,
        "ExportPart.finish",
        "value",
        8e-7,
        1e-12,
        DimensionVector::LENGTH,
    );
}

// ── Helper unit tests ────────────────────────────────────────────────────────

#[test]
#[allow(clippy::approx_constant)]
fn expect_real_or_int_extracts_real() {
    let val = Value::Real(3.14);
    let result = expect_real_or_int(&val, "test_label");
    assert!(
        (result - 3.14).abs() < 1e-15,
        "expected 3.14, got {}",
        result
    );
}

#[test]
fn expect_real_or_int_extracts_int() {
    let val = Value::Int(42);
    let result = expect_real_or_int(&val, "test_label");
    assert!(
        (result - 42.0).abs() < 1e-15,
        "expected 42.0, got {}",
        result
    );
}

#[test]
#[should_panic(expected = "should be Real or Int")]
fn expect_real_or_int_panics_on_non_numeric() {
    let val = Value::Bool(true);
    expect_real_or_int(&val, "test_label");
}

/// Extract element [row][col] from a nested `Value::Tensor` (matrix).
/// Panics with a descriptive message on any non-Real/Int element (incl. Undef).
fn matrix_elem(v: &Value, row: usize, col: usize) -> f64 {
    match v {
        Value::Tensor(rows) => match &rows[row] {
            Value::Tensor(cols) => match &cols[col] {
                Value::Real(x) => *x,
                Value::Int(i) => *i as f64,
                other => panic!("matrix_elem[{row}][{col}] should be Real or Int, got {other:?}"),
            },
            other => panic!("matrix row[{row}] should be Tensor, got {other:?}"),
        },
        other => panic!("expected Tensor for matrix, got {other:?}"),
    }
}

// ── step-1: linalg_4x4_determinant ───────────────────────────────────────────

/// Asserts LinalgDemo.det4 ≈ 209.0 (Value::Real).
/// m4 = tridiagonal SPD [[4,1,0,0],[1,4,1,0],[0,1,4,1],[0,0,1,4]];
/// det(m4) = 209 by recurrence D_n = 4·D_{n-1} − D_{n-2}, D_0=1,D_1=4.
#[test]
fn linalg_4x4_determinant() {
    let result = eval_ri_file(PATH_LINALG, "linalg");

    let det_id = ValueCellId::new("LinalgDemo", "det4");
    let det_val = result
        .values
        .get(&det_id)
        .unwrap_or_else(|| panic!("LinalgDemo.det4 not found in eval result"));

    match det_val {
        Value::Real(v) => {
            assert!(
                (v - 209.0).abs() < 1e-9,
                "LinalgDemo.det4 should be ≈209.0 (tridiagonal 4×4 det), got {}",
                v
            );
        }
        other => panic!("LinalgDemo.det4 should be Value::Real, got {:?}", other),
    }
}

// ── step-7: linalg_complex_eigenvalues_rotation ──────────────────────────────

/// Asserts LinalgDemo.ceig = complex_eigenvalues(rot2) is a Value::List of 2
/// Value::Complex items, containing {0+1i, 0−1i} by set-membership (tol 1e-9).
/// rot2 = [[0,-1],[1,0]] (90° rotation); char poly λ²+1=0 → eigenvalues ±i.
/// Set-membership is used rather than positional check to be robust to
/// builtin's (re,im) sort order.
#[test]
fn linalg_complex_eigenvalues_rotation() {
    let result = eval_ri_file(PATH_LINALG, "linalg");

    let ceig_id = ValueCellId::new("LinalgDemo", "ceig");
    let ceig_val = result
        .values
        .get(&ceig_id)
        .unwrap_or_else(|| panic!("LinalgDemo.ceig not found in eval result"));

    match ceig_val {
        Value::List(items) => {
            assert_eq!(
                items.len(),
                2,
                "ceig should have 2 entries, got {}",
                items.len()
            );

            // Extract (re, im) pairs from each Complex item.
            let pairs: Vec<(f64, f64)> = items
                .iter()
                .enumerate()
                .map(|(i, item)| match item {
                    Value::Complex { re, im, dimension } => {
                        assert_eq!(
                            *dimension,
                            DimensionVector::DIMENSIONLESS,
                            "ceig[{i}].dimension should be DIMENSIONLESS, got {dimension:?}"
                        );
                        (*re, *im)
                    }
                    other => panic!("ceig[{i}] should be Value::Complex, got {other:?}"),
                })
                .collect();

            // Check by set-membership: {0+1i} and {0-1i} must both appear.
            let has_plus_i = pairs
                .iter()
                .any(|(re, im)| re.abs() < 1e-9 && (im - 1.0).abs() < 1e-9);
            let has_minus_i = pairs
                .iter()
                .any(|(re, im)| re.abs() < 1e-9 && (im + 1.0).abs() < 1e-9);

            assert!(has_plus_i, "ceig should contain 0+1i, got {pairs:?}");
            assert!(has_minus_i, "ceig should contain 0-1i, got {pairs:?}");
        }
        other => panic!("LinalgDemo.ceig should be Value::List, got {other:?}"),
    }
}

// ── step-5: linalg_4x4_symmetric_eigenvalues ─────────────────────────────────

/// Asserts LinalgDemo.eig4 = eigenvalues(sym4) has exactly 4 entries,
/// sorted ≈ [1.0, 2.0, 3.0, 8.0] within 1e-9.
/// sym4 = block-diagonal [[2,1,0,0],[1,2,0,0],[0,0,5,3],[0,0,3,5]];
/// block spectra: top {2-1,2+1}={1,3}, bottom {5-3,5+3}={2,8}.
#[test]
fn linalg_4x4_symmetric_eigenvalues() {
    let result = eval_ri_file(PATH_LINALG, "linalg");

    let eig4_id = ValueCellId::new("LinalgDemo", "eig4");
    let eig4_val = result
        .values
        .get(&eig4_id)
        .unwrap_or_else(|| panic!("LinalgDemo.eig4 not found in eval result"));

    match eig4_val {
        Value::List(items) => {
            assert_eq!(
                items.len(),
                4,
                "eig4 should have 4 entries, got {}",
                items.len()
            );
            let mut actuals: Vec<f64> = items
                .iter()
                .enumerate()
                .map(|(i, item)| expect_real_or_int(item, &format!("eig4[{i}]")))
                .collect();
            // Intentional defensive sort: eigenvalues() already returns sorted-ascending
            // for the symmetric path, but we don't rely on that contract here so the
            // test stays valid if the ordering guarantee is ever relaxed.
            actuals.sort_by(|a, b| a.total_cmp(b));
            let expected = [1.0_f64, 2.0, 3.0, 8.0];
            for (i, (&actual, &exp)) in actuals.iter().zip(expected.iter()).enumerate() {
                assert!(
                    (actual - exp).abs() < 1e-9,
                    "eig4[{i}] (sorted): expected {exp}, got {actual}"
                );
            }
        }
        other => panic!("LinalgDemo.eig4 should be Value::List, got {other:?}"),
    }
}

// ── step-3: linalg_4x4_inverse_roundtrip ─────────────────────────────────────

/// Asserts m4 · inv4 ≈ I₄ (all 16 entries, tolerance 1e-9).
/// m4 = tridiagonal SPD; SPD with κ≈2.36 gives try_inverse residual ~1e-15.
/// Both m4 and inv4 are extracted as 4×4 f64 arrays via matrix_elem;
/// the product is computed in Rust (no matmul builtin exists in .ri).
#[test]
fn linalg_4x4_inverse_roundtrip() {
    let result = eval_ri_file(PATH_LINALG, "linalg");

    let m4_id = ValueCellId::new("LinalgDemo", "m4");
    let m4_val = result
        .values
        .get(&m4_id)
        .unwrap_or_else(|| panic!("LinalgDemo.m4 not found in eval result"));

    let inv4_id = ValueCellId::new("LinalgDemo", "inv4");
    let inv4_val = result
        .values
        .get(&inv4_id)
        .unwrap_or_else(|| panic!("LinalgDemo.inv4 not found in eval result"));

    // Extract both matrices as 4×4 f64 arrays.
    let m4: [[f64; 4]; 4] =
        std::array::from_fn(|r| std::array::from_fn(|c| matrix_elem(m4_val, r, c)));
    let inv4: [[f64; 4]; 4] =
        std::array::from_fn(|r| std::array::from_fn(|c| matrix_elem(inv4_val, r, c)));

    // Compute product P = m4 · inv4 and assert P ≈ I₄.
    // Cross-dimensional indexing (m4[i][k] * inv4[k][j]) requires both i and j.
    #[allow(clippy::needless_range_loop)]
    for i in 0..4 {
        for j in 0..4 {
            let p_ij: f64 = (0..4).map(|k| m4[i][k] * inv4[k][j]).sum();
            let expected = if i == j { 1.0 } else { 0.0 };
            assert!(
                (p_ij - expected).abs() < 1e-9,
                "m4·inv4[{i}][{j}] should be ≈{expected}, got {p_ij}"
            );
        }
    }
}

// NOTE: each test independently calls eval_ri_file, re-parsing and re-compiling
// the same .ri fixture. This matches the established m8_3 pattern; future
// iterations may share results across same-fixture tests via
// `std::sync::LazyLock` to avoid redundant parse→compile→eval cycles.
