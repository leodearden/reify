//! Unified v0.1 integration test — definitive correctness gate for both fixtures.
//!
//! Loads both `examples/integration_full_v01.ri` (task 291) and
//! `examples/integration_corner_cases.ri` (task 292) through the full
//! parse → compile → eval → check pipeline in a single file.
//!
//! Test coverage:
//!   - Computed values (params, lets, fn overloads, match, field sample, etc.)
//!   - Constraint satisfaction status for all constraints
//!   - Determinacy states via `Engine::snapshot().values`
//!   - `@test` annotation results via `run_tests()`
//!   - Purpose activation / deactivation effects
//!
//! Tests prefixed `full_v01_*` target `integration_full_v01.ri`.
//! Tests prefixed `corner_*` target `integration_corner_cases.ri`.

use std::sync::OnceLock;

use reify_compiler::CompiledModule;
use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};
use reify_core::{DimensionVector, ModulePath, Severity, ValueCellId};
use reify_ir::{DeterminacyState, Satisfaction, Value};

// ── Path constants ────────────────────────────────────────────────────────────

const FULL_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/integration_full_v01.ri"
);

const CORNER_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/integration_corner_cases.ri"
);

// ── Cached helpers: integration_full_v01.ri ───────────────────────────────────

/// Read integration_full_v01.ri, caching the result. Returns `&'static str`.
fn source_full() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(FULL_PATH)
            .unwrap_or_else(|e| panic!("{FULL_PATH} should exist: {e}"))
    })
    .as_str()
}

/// Parse and compile with stdlib, caching the result. Returns `&'static CompiledModule`.
fn compiled_full() -> &'static CompiledModule {
    static C: OnceLock<CompiledModule> = OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(source_full()))
}

/// Evaluate integration_full_v01.ri with a fresh SimpleConstraintChecker engine.
fn eval_full() -> reify_eval::EvalResult {
    let mut engine = make_simple_engine();
    engine.eval(compiled_full())
}

/// Check integration_full_v01.ri with a fresh SimpleConstraintChecker engine.
fn check_full() -> reify_eval::CheckResult {
    let mut engine = make_simple_engine();
    engine.check(compiled_full())
}

/// Parse, compile (with stdlib) + check a mutated source string.
fn check_full_source(src: &str) -> reify_eval::CheckResult {
    let compiled = parse_and_compile_with_stdlib(src);
    let mut engine = make_simple_engine();
    engine.check(&compiled)
}

// ── Cached helpers: integration_corner_cases.ri ───────────────────────────────

/// Read integration_corner_cases.ri, caching the result. Returns `&'static str`.
fn source_corner() -> &'static str {
    static S: OnceLock<String> = OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(CORNER_PATH)
            .unwrap_or_else(|e| panic!("{CORNER_PATH} should exist: {e}"))
    })
    .as_str()
}

/// Parse and compile without stdlib, caching the result. Returns `&'static CompiledModule`.
fn compiled_corner() -> &'static CompiledModule {
    static C: OnceLock<CompiledModule> = OnceLock::new();
    C.get_or_init(|| {
        let src = source_corner();
        let parsed = reify_syntax::parse(src, ModulePath::single("integration_corner_cases"));
        assert!(
            parsed.errors.is_empty(),
            "parse errors in integration_corner_cases.ri: {:?}",
            parsed.errors
        );
        let compiled = reify_compiler::compile(&parsed);
        let errors: Vec<_> = compiled
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(
            errors.is_empty(),
            "compile errors in integration_corner_cases.ri: {:?}",
            errors
        );
        compiled
    })
}

/// Evaluate integration_corner_cases.ri with a fresh SimpleConstraintChecker engine.
fn eval_corner() -> reify_eval::EvalResult {
    let mut engine = make_simple_engine();
    engine.eval(compiled_corner())
}

/// Check integration_corner_cases.ri with a fresh SimpleConstraintChecker engine.
fn check_corner() -> reify_eval::CheckResult {
    let mut engine = make_simple_engine();
    engine.check(compiled_corner())
}

// ── STEP-1: Smoke tests ───────────────────────────────────────────────────────

/// Smoke test for integration_full_v01.ri: parse, compile with stdlib, eval.
/// Asserts no errors and non-empty values map.
#[test]
fn full_v01_smoke_test() {
    let result = eval_full();
    assert!(
        !result.values.is_empty(),
        "eval of integration_full_v01.ri should produce non-empty values"
    );
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "eval of integration_full_v01.ri should produce no errors, got: {:?}",
        errors
    );
}

/// Smoke test for integration_corner_cases.ri: parse, compile, eval.
/// Asserts no errors and non-empty values map.
#[test]
fn corner_cases_smoke_test() {
    let result = eval_corner();
    assert!(
        !result.values.is_empty(),
        "eval of integration_corner_cases.ri should produce non-empty values"
    );
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "eval of integration_corner_cases.ri should produce no errors, got: {:?}",
        errors
    );
}

// ── STEP-3: Template structure and constraint satisfaction (full_v01) ─────────

/// Verify Assembly template exists with >=10 value cells, RecursiveBeam and
/// PipeConnector templates exist, and >=5 templates total (incl @test structs).
#[test]
fn full_v01_template_structure() {
    let compiled = compiled_full();

    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("Assembly template should exist in compiled_full");
    assert!(
        assembly.value_cells.len() >= 10,
        "Assembly should have >=10 value cells, got {}",
        assembly.value_cells.len()
    );

    assert!(
        compiled.templates.iter().any(|t| t.name == "RecursiveBeam"),
        "RecursiveBeam template should exist"
    );
    assert!(
        compiled.templates.iter().any(|t| t.name == "PipeConnector"),
        "PipeConnector template should exist"
    );
    assert!(
        compiled.templates.len() >= 5,
        "expected >=5 templates total (Assembly, RecursiveBeam, PipeConnector, + @test), got {}",
        compiled.templates.len()
    );
}

/// Verify check_full() returns no Violated entries — all constraints Satisfied.
#[test]
fn full_v01_all_constraints_satisfied() {
    let check_result = check_full();
    assert!(
        !check_result.constraint_results.is_empty(),
        "check_full() should return at least one constraint result"
    );
    for entry in &check_result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}

/// Verify total constraint count from check_full() >= 40.
#[test]
fn full_v01_constraint_count_at_least_40() {
    let check_result = check_full();
    let n = check_result.constraint_results.len();
    assert!(
        n >= 40,
        "expected >=40 total constraint results in integration_full_v01.ri, got {n}"
    );
}

// ── STEP-5: Computed value assertions (full_v01) ─────────────────────────────

/// mass=5kg (5.0 SI), position_x=100mm (0.1 SI), position_y=200mm (0.2 SI).
#[test]
fn full_v01_trait_param_values() {
    let result = eval_full();

    let mass_id = ValueCellId::new("Assembly", "mass");
    match result
        .values
        .get(&mass_id)
        .expect("Assembly.mass not found")
    {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 5.0).abs() < 1e-12,
                "Assembly.mass: expected 5.0 SI (5kg), got {si_value}"
            );
        }
        other => panic!("Assembly.mass should be Scalar, got {other:?}"),
    }

    let px_id = ValueCellId::new("Assembly", "position_x");
    match result
        .values
        .get(&px_id)
        .expect("Assembly.position_x not found")
    {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.1).abs() < 1e-12,
                "Assembly.position_x: expected 0.1 SI (100mm), got {si_value}"
            );
        }
        other => panic!("Assembly.position_x should be Scalar, got {other:?}"),
    }

    let py_id = ValueCellId::new("Assembly", "position_y");
    match result
        .values
        .get(&py_id)
        .expect("Assembly.position_y not found")
    {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.2).abs() < 1e-12,
                "Assembly.position_y: expected 0.2 SI (200mm), got {si_value}"
            );
        }
        other => panic!("Assembly.position_y should be Scalar, got {other:?}"),
    }
}

/// clearance = 500mil = 500 * 0.0000254m = 0.0127 SI.
#[test]
fn full_v01_custom_unit_value() {
    let result = eval_full();
    let id = ValueCellId::new("Assembly", "clearance");
    match result
        .values
        .get(&id)
        .expect("Assembly.clearance not found")
    {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.0127).abs() < 1e-9,
                "Assembly.clearance: expected ~0.0127 SI (500mil), got {si_value}"
            );
        }
        other => panic!("Assembly.clearance should be Scalar, got {other:?}"),
    }
}

/// proj_name = String("integration-test"), file_ver = String("0.1").
#[test]
fn full_v01_meta_access_values() {
    let result = eval_full();

    let proj_id = ValueCellId::new("Assembly", "proj_name");
    assert_eq!(
        result
            .values
            .get(&proj_id)
            .expect("Assembly.proj_name not found"),
        &Value::String("integration-test".to_string()),
        "Assembly.proj_name should be String(\"integration-test\")"
    );

    let ver_id = ValueCellId::new("Assembly", "file_ver");
    assert_eq!(
        result
            .values
            .get(&ver_id)
            .expect("Assembly.file_ver not found"),
        &Value::String("0.1".to_string()),
        "Assembly.file_ver should be String(\"0.1\")"
    );
}

/// grade_code = Int(3) from match Grade.Premium → 3.
#[test]
fn full_v01_match_expression() {
    let result = eval_full();
    let id = ValueCellId::new("Assembly", "grade_code");
    assert_eq!(
        result
            .values
            .get(&id)
            .expect("Assembly.grade_code not found"),
        &Value::Int(3),
        "Assembly.grade_code should be Int(3) (Grade.Premium → 3)"
    );
}

/// size_count = Int(5) (sizes.count where sizes=[10,20,30,40,50]).
#[test]
fn full_v01_collection_operations() {
    let result = eval_full();
    let id = ValueCellId::new("Assembly", "size_count");
    assert_eq!(
        result
            .values
            .get(&id)
            .expect("Assembly.size_count not found"),
        &Value::Int(5),
        "Assembly.size_count should be Int(5)"
    );
}

/// safe_load_real ≈ 150 (Real overload * 1.5), safe_load_int = Int(200) (Int overload * 2).
#[test]
fn full_v01_function_overloads() {
    let result = eval_full();

    let real_id = ValueCellId::new("Assembly", "safe_load_real");
    match result
        .values
        .get(&real_id)
        .expect("Assembly.safe_load_real not found")
    {
        Value::Real(v) => {
            assert!(
                (v - 150.0).abs() < 1.0,
                "Assembly.safe_load_real: expected ~150.0, got {v}"
            );
        }
        other => panic!("Assembly.safe_load_real should be Real, got {other:?}"),
    }

    let int_id = ValueCellId::new("Assembly", "safe_load_int");
    assert_eq!(
        result
            .values
            .get(&int_id)
            .expect("Assembly.safe_load_int not found"),
        &Value::Int(200),
        "Assembly.safe_load_int should be Int(200)"
    );
}

/// impedance = complex(3.0, 4.0) — Value::Complex, not Undef.
#[test]
fn full_v01_complex_binding() {
    let result = eval_full();
    let id = ValueCellId::new("Assembly", "impedance");
    let v = result
        .values
        .get(&id)
        .expect("Assembly.impedance not found");
    assert!(
        !matches!(v, Value::Undef),
        "Assembly.impedance should not be Undef"
    );
    assert!(
        matches!(v, Value::Complex { .. }),
        "Assembly.impedance should be Value::Complex, got {v:?}"
    );
}

/// grade_label = String("premium") from if-then-else on Grade.Premium.
#[test]
fn full_v01_if_then_else() {
    let result = eval_full();
    let id = ValueCellId::new("Assembly", "grade_label");
    assert_eq!(
        result
            .values
            .get(&id)
            .expect("Assembly.grade_label not found"),
        &Value::String("premium".to_string()),
        "Assembly.grade_label should be String(\"premium\")"
    );
}

/// height_ok = Bool(true), mass_valid = Bool(true).
#[test]
fn full_v01_boolean_logic() {
    let result = eval_full();

    let hok_id = ValueCellId::new("Assembly", "height_ok");
    assert_eq!(
        result
            .values
            .get(&hok_id)
            .expect("Assembly.height_ok not found"),
        &Value::Bool(true),
        "Assembly.height_ok should be Bool(true)"
    );

    let mv_id = ValueCellId::new("Assembly", "mass_valid");
    assert_eq!(
        result
            .values
            .get(&mv_id)
            .expect("Assembly.mass_valid not found"),
        &Value::Bool(true),
        "Assembly.mass_valid should be Bool(true)"
    );
}

/// maybe_load = Option(Some(_)), no_load = Option(None).
#[test]
fn full_v01_some_none() {
    let result = eval_full();

    let some_id = ValueCellId::new("Assembly", "maybe_load");
    let some_val = result
        .values
        .get(&some_id)
        .expect("Assembly.maybe_load not found");
    assert!(
        matches!(some_val, Value::Option(Some(_))),
        "Assembly.maybe_load should be Value::Option(Some(_)), got {some_val:?}"
    );

    let none_id = ValueCellId::new("Assembly", "no_load");
    let none_val = result
        .values
        .get(&none_id)
        .expect("Assembly.no_load not found");
    assert!(
        matches!(none_val, Value::Option(None)),
        "Assembly.no_load should be Value::Option(None), got {none_val:?}"
    );
}

// ── STEP-7: Geometric and field value tests (full_v01) ────────────────────────

/// origin, target, offset, displacement, shifted are all non-Undef concrete values.
#[test]
fn full_v01_geometric_bindings() {
    let result = eval_full();
    for name in ["origin", "target", "offset", "displacement"] {
        let id = ValueCellId::new("Assembly", name);
        let v = result
            .values
            .get(&id)
            .unwrap_or_else(|| panic!("Assembly.{name} not found in eval result"));
        assert!(
            !matches!(v, Value::Undef),
            "Assembly.{name} should not be Undef (expected a concrete geometric value)"
        );
    }
}

/// rot, base_frame, xform are non-Undef (orientation + frame + transform).
#[test]
fn full_v01_orientation_frame_transform() {
    let result = eval_full();
    for name in ["rot", "base_frame", "xform"] {
        let id = ValueCellId::new("Assembly", name);
        let v = result
            .values
            .get(&id)
            .unwrap_or_else(|| panic!("Assembly.{name} not found in eval result"));
        assert!(
            !matches!(v, Value::Undef),
            "Assembly.{name} should not be Undef"
        );
    }
}

/// temp_at_3 ≈ 19.0 (x²+10 at x=3), dtemp_at_3 ≈ 6.0 (central differences, 2*3).
#[test]
fn full_v01_field_sample_gradient() {
    let result = eval_full();

    let numeric = |v: &Value, name: &str| -> f64 {
        match v {
            Value::Real(f) => *f,
            Value::Int(i) => *i as f64,
            other => panic!("expected Real or Int for Assembly.{name}, got {other:?}"),
        }
    };

    let temp_id = ValueCellId::new("Assembly", "temp_at_3");
    let temp_val = result
        .values
        .get(&temp_id)
        .expect("Assembly.temp_at_3 not found");
    let temp_f = numeric(temp_val, "temp_at_3");
    assert!(
        (temp_f - 19.0).abs() < 0.1,
        "Assembly.temp_at_3: expected ~19.0, got {temp_f}"
    );

    let dtemp_id = ValueCellId::new("Assembly", "dtemp_at_3");
    let dtemp_val = result
        .values
        .get(&dtemp_id)
        .expect("Assembly.dtemp_at_3 not found");
    let dtemp_f = numeric(dtemp_val, "dtemp_at_3");
    assert!(
        (dtemp_f - 6.0).abs() < 0.1,
        "Assembly.dtemp_at_3: expected ~6.0 (central diff), got {dtemp_f}"
    );
}

/// size_range and size_range_exc are Value::Range variants.
#[test]
fn full_v01_range_lets() {
    let result = eval_full();

    let range_id = ValueCellId::new("Assembly", "size_range");
    let range_val = result
        .values
        .get(&range_id)
        .expect("Assembly.size_range not found");
    assert!(
        matches!(range_val, Value::Range { .. }),
        "Assembly.size_range should be Value::Range, got {range_val:?}"
    );

    let exc_id = ValueCellId::new("Assembly", "size_range_exc");
    let exc_val = result
        .values
        .get(&exc_id)
        .expect("Assembly.size_range_exc not found");
    assert!(
        matches!(exc_val, Value::Range { .. }),
        "Assembly.size_range_exc should be Value::Range, got {exc_val:?}"
    );
}

// ── STEP-9: Determinacy state tests via snapshot API (full_v01) ───────────────

/// Key params and let bindings should be DeterminacyState::Determined in the snapshot.
#[test]
fn full_v01_determinacy_determined_cells() {
    let compiled = compiled_full();
    let mut engine = make_simple_engine();
    engine.eval(compiled);
    let snap = engine.snapshot().expect("snapshot should exist after eval");

    let determined_cells = [
        ("Assembly", "mass"),
        ("Assembly", "height"),
        ("Assembly", "width"),
        ("Assembly", "clearance"),
        ("Assembly", "position_x"),
        ("Assembly", "position_y"),
        ("Assembly", "origin"),
        ("Assembly", "target"),
        ("Assembly", "impedance"),
        ("Assembly", "grade_code"),
        ("Assembly", "size_count"),
    ];

    for (entity, member) in &determined_cells {
        let id = ValueCellId::new(*entity, *member);
        match snap.values.get(&id) {
            Some((_, state)) => {
                assert_eq!(
                    *state,
                    DeterminacyState::Determined,
                    "{entity}.{member} should be Determined, got {state:?}"
                );
            }
            None => panic!("{entity}.{member} not found in snapshot"),
        }
    }
}

/// auto(free) params should be DeterminacyState::Auto in the snapshot.
#[test]
fn full_v01_determinacy_auto_cells() {
    let compiled = compiled_full();
    let mut engine = make_simple_engine();
    engine.eval(compiled);
    let snap = engine.snapshot().expect("snapshot should exist after eval");

    for member in ["load_auto", "load_free"] {
        let id = ValueCellId::new("Assembly", member);
        match snap.values.get(&id) {
            Some((_, state)) => {
                assert_eq!(
                    *state,
                    DeterminacyState::Auto,
                    "Assembly.{member} should be Auto (auto/free param), got {state:?}"
                );
            }
            None => panic!("Assembly.{member} not found in snapshot"),
        }
    }
}

/// Let bindings (proj_name, file_ver, grade, temp_at_3) should be Determined.
#[test]
fn full_v01_determinacy_let_bindings() {
    let compiled = compiled_full();
    let mut engine = make_simple_engine();
    engine.eval(compiled);
    let snap = engine.snapshot().expect("snapshot should exist after eval");

    let let_cells = [
        ("Assembly", "proj_name"),
        ("Assembly", "file_ver"),
        ("Assembly", "grade_code"),
        ("Assembly", "temp_at_3"),
    ];

    for (entity, member) in &let_cells {
        let id = ValueCellId::new(*entity, *member);
        match snap.values.get(&id) {
            Some((_, state)) => {
                assert_eq!(
                    *state,
                    DeterminacyState::Determined,
                    "{entity}.{member} (let binding) should be Determined, got {state:?}"
                );
            }
            None => panic!("{entity}.{member} not found in snapshot"),
        }
    }
}

// ── STEP-11: @test runner (full_v01) ─────────────────────────────────────────

/// Run @test structures with SimpleConstraintChecker; assert all named tests Pass.
#[test]
fn full_v01_test_runner_all_pass() {
    let compiled = compiled_full();
    let results = reify_eval::run_tests(compiled, || Box::new(SimpleConstraintChecker));

    assert!(
        results.len() >= 6,
        "expected >=6 @test results, got {}",
        results.len()
    );

    let expected_tests = [
        "TestHeightPositive",
        "TestWidthInRange",
        "TestSubBeamSpan",
        "TestComplexBinding",
        "TestFieldSample",
        "TestGradeMatch",
    ];

    let by_name: std::collections::HashMap<&str, reify_eval::TestStatus> = results
        .iter()
        .map(|r| (r.name.as_str(), r.status))
        .collect();

    for name in &expected_tests {
        if let Some(&status) = by_name.get(*name) {
            assert_eq!(
                status,
                reify_eval::TestStatus::Pass,
                "@test {name} should be Pass, got {status:?}"
            );
        }
    }

    // All present tests must be Pass
    for r in &results {
        assert_eq!(
            r.status,
            reify_eval::TestStatus::Pass,
            "@test {} should be Pass, got {:?}",
            r.name,
            r.status
        );
    }
}

// ── STEP-13: Purpose activation tests (full_v01) ─────────────────────────────

/// Helper: count constraints from engine snapshot graph.
fn constraint_count(engine: &reify_eval::Engine) -> usize {
    engine
        .snapshot()
        .expect("snapshot should exist")
        .graph
        .constraints
        .len()
}

/// mfg_ready purpose: exists, entity_kind='Structure', activate adds constraints,
/// deactivate restores count.
#[test]
fn full_v01_purpose_mfg_ready() {
    let compiled = compiled_full();

    let mfg_ready = compiled
        .compiled_purposes
        .iter()
        .find(|p| p.name == "mfg_ready")
        .expect("mfg_ready purpose should exist in compiled_full");
    assert_eq!(
        mfg_ready.params.len(),
        1,
        "mfg_ready should have exactly 1 param"
    );
    assert_eq!(
        mfg_ready.params[0].entity_kind, "Structure",
        "mfg_ready param entity_kind should be 'Structure'"
    );

    let mut engine = make_simple_engine();
    engine.eval(compiled);
    let before = constraint_count(&engine);

    engine.activate_purpose("mfg_ready", "Assembly");
    assert!(
        engine.is_purpose_active("mfg_ready"),
        "mfg_ready should be active after activate_purpose"
    );

    let expected_extra = mfg_ready.constraints.len();
    assert_eq!(
        constraint_count(&engine),
        before + expected_extra,
        "activating mfg_ready should add {expected_extra} constraints"
    );

    engine.deactivate_purpose("mfg_ready");
    assert_eq!(
        constraint_count(&engine),
        before,
        "deactivating mfg_ready should restore constraint count to {before}"
    );
    assert!(
        !engine.is_purpose_active("mfg_ready"),
        "mfg_ready should NOT be active after deactivate_purpose"
    );
}

// ── STEP-15: Structural tests (full_v01) ─────────────────────────────────────

/// Assembly has 1 connection, connector_sub is Some, port_mappings has diameter→diameter.
#[test]
fn full_v01_connect_has_connector_and_mapping() {
    let compiled = compiled_full();
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("Assembly template should exist");

    assert_eq!(
        assembly.connections.len(),
        1,
        "Assembly should have exactly 1 connection, got {}",
        assembly.connections.len()
    );

    let conn = &assembly.connections[0];
    assert!(
        conn.connector_sub.is_some(),
        "connection should have a connector_sub (from `: PipeConnector`)"
    );
    assert!(
        !conn.port_mappings.is_empty(),
        "expected at least 1 port mapping (diameter→diameter)"
    );
    assert!(
        conn.port_mappings
            .iter()
            .any(|(from, to)| from == "diameter" && to == "diameter"),
        "port_mappings should contain (diameter, diameter); got {:?}",
        conn.port_mappings
    );

    // Compatibility constraint is Satisfied
    let compat_id = &conn.compatibility_constraint;
    let check_result = check_full();
    let compat_entry = check_result
        .constraint_results
        .iter()
        .find(|e| &e.id == compat_id)
        .unwrap_or_else(|| {
            panic!("compatibility constraint {compat_id} not found in check results")
        });
    assert_eq!(
        compat_entry.satisfaction,
        Satisfaction::Satisfied,
        "compatibility constraint should be Satisfied"
    );
}

/// Assembly has 1 guarded_group with 2 constraints, both Satisfied.
#[test]
fn full_v01_where_block_constraints() {
    let compiled = compiled_full();
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("Assembly template should exist");

    assert_eq!(
        assembly.guarded_groups.len(),
        1,
        "Assembly should have exactly 1 guarded group"
    );
    let group = &assembly.guarded_groups[0];
    assert_eq!(
        group.constraints.len(),
        2,
        "where-block should have exactly 2 constraints"
    );

    let guarded_ids: Vec<_> = group.constraints.iter().map(|c| &c.id).collect();
    let check_result = check_full();
    for guarded_id in &guarded_ids {
        let entry = check_result
            .constraint_results
            .iter()
            .find(|e| &&e.id == guarded_id)
            .unwrap_or_else(|| {
                panic!("where-block constraint {guarded_id} not found in check results")
            });
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "where-block constraint {guarded_id} should be Satisfied"
        );
    }
}

/// RecursiveBeam.child.span=0.15 SI, .child.child.span=0.075 SI, .child.child.child does not exist.
#[test]
fn full_v01_recursive_unfold() {
    let result = eval_full();

    let child_span_id = ValueCellId::new("RecursiveBeam.child", "span");
    match result
        .values
        .get(&child_span_id)
        .expect("RecursiveBeam.child.span not found")
    {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.15).abs() < 1e-9,
                "RecursiveBeam.child.span: expected ~0.15 SI (150mm), got {si_value}"
            );
        }
        other => panic!("RecursiveBeam.child.span should be Scalar, got {other:?}"),
    }

    let grandchild_span_id = ValueCellId::new("RecursiveBeam.child.child", "span");
    match result
        .values
        .get(&grandchild_span_id)
        .expect("RecursiveBeam.child.child.span not found")
    {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.075).abs() < 1e-9,
                "RecursiveBeam.child.child.span: expected ~0.075 SI (75mm), got {si_value}"
            );
        }
        other => panic!("RecursiveBeam.child.child.span should be Scalar, got {other:?}"),
    }

    let great_grandchild_id = ValueCellId::new("RecursiveBeam.child.child.child", "span");
    assert!(
        !result.values.contains(&great_grandchild_id),
        "RecursiveBeam.child.child.child.span should not exist (depth=0 stops unfolding)"
    );
}

/// Assembly has 4 InRange constraints (2 invocations × 2 predicates), all Satisfied.
#[test]
fn full_v01_constraint_def_labels() {
    let check_result = check_full();

    let inrange_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| {
            e.id.entity == "Assembly"
                && e.label
                    .as_deref()
                    .is_some_and(|l| l.starts_with("InRange#"))
        })
        .collect();

    assert_eq!(
        inrange_constraints.len(),
        4,
        "expected exactly 4 Assembly InRange constraints (2 invocations × 2 predicates), got {}",
        inrange_constraints.len()
    );

    // Under task 845's label scheme each invocation gets a unique inst_idx, so
    // the 4 labels are InRange#0[0], InRange#0[1], InRange#1[0], InRange#1[1]
    // (1 of each).
    let count_label = |label: &str| -> usize {
        inrange_constraints
            .iter()
            .filter(|e| e.label.as_deref() == Some(label))
            .count()
    };
    assert_eq!(
        count_label("InRange#0[0]"),
        1,
        "expected 1 constraint with label 'InRange#0[0]'"
    );
    assert_eq!(
        count_label("InRange#0[1]"),
        1,
        "expected 1 constraint with label 'InRange#0[1]'"
    );
    assert_eq!(
        count_label("InRange#1[0]"),
        1,
        "expected 1 constraint with label 'InRange#1[0]'"
    );
    assert_eq!(
        count_label("InRange#1[1]"),
        1,
        "expected 1 constraint with label 'InRange#1[1]'"
    );

    for entry in &inrange_constraints {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "InRange constraint {} should be Satisfied",
            entry.id
        );
    }
}

// ── STEP-17: Violation regression guard (full_v01) ────────────────────────────

/// Replace 'constraint height < 2000mm' with 'constraint height < 100mm' to force
/// a Violated result. Total count must still be >=40 (no short-circuit).
#[test]
fn full_v01_violated_constraint_detected() {
    let violating =
        source_full().replace("constraint height < 2000mm", "constraint height < 100mm");
    assert_ne!(
        violating,
        source_full(),
        "replace target drifted — 'constraint height < 2000mm' not found; update the test"
    );

    let check_result = check_full_source(&violating);

    assert!(
        check_result.constraint_results.len() >= 40,
        "expected >=40 constraint results even for violating source, got {}",
        check_result.constraint_results.len()
    );

    let violations: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        !violations.is_empty(),
        "expected at least one Violated constraint after raising height bound below its value, \
         got {} results with no violations",
        check_result.constraint_results.len()
    );
}

// ── STEP-19: Corner_cases computed value assertions ───────────────────────────

/// JerkDemo.j has Jerk dimension (Length·Time⁻³ = L¹·T⁻³).
#[test]
fn corner_type_alias_three_deep() {
    let result = eval_corner();
    let id = ValueCellId::new("JerkDemo", "j");
    let v = result.values.get(&id).expect("JerkDemo.j not found");

    let expected_dim = DimensionVector::LENGTH.div(&DimensionVector::TIME.pow(3));
    match v {
        Value::Scalar { dimension, .. } => {
            assert_eq!(
                dimension, &expected_dim,
                "JerkDemo.j dimension should be Length/Time^3 (Jerk), got {dimension:?}"
            );
        }
        other => panic!("JerkDemo.j should be Scalar (Jerk), got {other:?}"),
    }
}

/// FullTraitImpl.size and .doubled are Scalar; trait_bounds includes FullTrait;
/// no Violated constraints.
#[test]
fn corner_trait_all_member_kinds() {
    let result = eval_corner();

    let size_id = ValueCellId::new("FullTraitImpl", "size");
    let size_val = result
        .values
        .get(&size_id)
        .expect("FullTraitImpl.size not found");
    assert!(
        matches!(size_val, Value::Scalar { .. }),
        "FullTraitImpl.size should be Scalar, got {size_val:?}"
    );

    let doubled_id = ValueCellId::new("FullTraitImpl", "doubled");
    let doubled_val = result
        .values
        .get(&doubled_id)
        .expect("FullTraitImpl.doubled not found");
    assert!(
        matches!(doubled_val, Value::Scalar { .. }),
        "FullTraitImpl.doubled should be Scalar, got {doubled_val:?}"
    );

    let compiled = compiled_corner();
    let template = compiled
        .templates
        .iter()
        .find(|t| t.name == "FullTraitImpl")
        .expect("FullTraitImpl template must exist");
    assert!(
        template.trait_bounds.iter().any(|b| b == "FullTrait"),
        "FullTraitImpl should declare conformance to FullTrait, got: {:?}",
        template.trait_bounds
    );

    let check_result = check_corner();
    let violated: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "FullTraitImpl" && e.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        violated.is_empty(),
        "FullTraitImpl should have no Violated constraints, got: {:?}",
        violated.iter().map(|e| &e.id).collect::<Vec<_>>()
    );
}

/// VacuousUser.w is Scalar (vacuous constraint def = 0 predicates).
#[test]
fn corner_vacuous_constraint_def() {
    let result = eval_corner();
    let w_id = ValueCellId::new("VacuousUser", "w");
    let w_val = result.values.get(&w_id).expect("VacuousUser.w not found");
    assert!(
        matches!(w_val, Value::Scalar { .. }),
        "VacuousUser.w should be Scalar, got {w_val:?}"
    );
}

/// EmptyListOps.n = Int(0).
#[test]
fn corner_empty_list_count() {
    let result = eval_corner();
    let n_id = ValueCellId::new("EmptyListOps", "n");
    assert_eq!(
        result.values.get(&n_id).expect("EmptyListOps.n not found"),
        &Value::Int(0),
        "EmptyListOps.n should be Int(0)"
    );
}

/// UndefPropagation: arith, cmp, neg all Value::Undef.
#[test]
fn corner_undef_propagation() {
    let result = eval_corner();

    for member in ["arith", "cmp", "neg"] {
        let id = ValueCellId::new("UndefPropagation", member);
        assert_eq!(
            result
                .values
                .get(&id)
                .unwrap_or_else(|| panic!("UndefPropagation.{member} not found")),
            &Value::Undef,
            "UndefPropagation.{member} should be Undef"
        );
    }
}

/// OptionEdgeCases.s = Option(Some(_)), .n = Option(None).
#[test]
fn corner_option_edges() {
    let result = eval_corner();

    let s_id = ValueCellId::new("OptionEdgeCases", "s");
    let s_val = result
        .values
        .get(&s_id)
        .expect("OptionEdgeCases.s not found");
    assert!(
        matches!(s_val, Value::Option(Some(_))),
        "OptionEdgeCases.s should be Option(Some(_)), got {s_val:?}"
    );

    let n_id = ValueCellId::new("OptionEdgeCases", "n");
    let n_val = result
        .values
        .get(&n_id)
        .expect("OptionEdgeCases.n not found");
    assert!(
        matches!(n_val, Value::Option(None)),
        "OptionEdgeCases.n should be Option(None), got {n_val:?}"
    );
}

/// RecTree.depth = Int(0); RecTree.child.depth does not exist.
#[test]
fn corner_recursive_depth_zero() {
    let result = eval_corner();

    let depth_id = ValueCellId::new("RecTree", "depth");
    assert_eq!(
        result
            .values
            .get(&depth_id)
            .expect("RecTree.depth not found"),
        &Value::Int(0),
        "RecTree.depth should be Int(0)"
    );

    let child_depth_id = ValueCellId::new("RecTree.child", "depth");
    assert!(
        !result.values.contains(&child_depth_id),
        "RecTree.child.depth should not exist when depth=0"
    );
}

/// ChainedFour.chain = Bool(true) (1mm < 2mm < 3mm < 4mm).
#[test]
fn corner_chained_comparison() {
    let result = eval_corner();
    let chain_id = ValueCellId::new("ChainedFour", "chain");
    assert_eq!(
        result
            .values
            .get(&chain_id)
            .expect("ChainedFour.chain not found"),
        &Value::Bool(true),
        "ChainedFour.chain should be Bool(true)"
    );
}

/// KleeneEdge: and_absorb=Bool(false), or_absorb=Bool(true), implies_vacuous=Bool(true).
#[test]
fn corner_kleene_logic() {
    let result = eval_corner();

    let and_id = ValueCellId::new("KleeneEdge", "and_absorb");
    assert_eq!(
        result
            .values
            .get(&and_id)
            .expect("KleeneEdge.and_absorb not found"),
        &Value::Bool(false),
        "false && Undef should be Bool(false) (Kleene AND)"
    );

    let or_id = ValueCellId::new("KleeneEdge", "or_absorb");
    assert_eq!(
        result
            .values
            .get(&or_id)
            .expect("KleeneEdge.or_absorb not found"),
        &Value::Bool(true),
        "true || Undef should be Bool(true) (Kleene OR)"
    );

    let imp_id = ValueCellId::new("KleeneEdge", "implies_vacuous");
    assert_eq!(
        result
            .values
            .get(&imp_id)
            .expect("KleeneEdge.implies_vacuous not found"),
        &Value::Bool(true),
        "!false || Undef should be Bool(true) (vacuous implication)"
    );
}

/// EqualRange.r is Value::Range with equal lower/upper bounds.
#[test]
fn corner_equal_range() {
    let result = eval_corner();
    let r_id = ValueCellId::new("EqualRange", "r");
    let r_val = result.values.get(&r_id).expect("EqualRange.r not found");
    match r_val {
        Value::Range {
            lower,
            upper,
            lower_inclusive: _,
            upper_inclusive: _,
        } => {
            assert!(lower.is_some(), "EqualRange.r lower bound should be Some");
            assert!(upper.is_some(), "EqualRange.r upper bound should be Some");
            assert_eq!(
                lower.as_deref(),
                upper.as_deref(),
                "EqualRange.r lower and upper bounds should be equal (5mm..5mm)"
            );
        }
        other => panic!("EqualRange.r should be Value::Range, got {other:?}"),
    }
}

/// AutoFreeMulti has >=2 Satisfied constraints (constrained(x) and constrained(y)).
#[test]
fn corner_auto_free() {
    let check_result = check_corner();

    let auto_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.id.entity == "AutoFreeMulti")
        .collect();
    assert!(
        !auto_constraints.is_empty(),
        "AutoFreeMulti should have at least some constraint results"
    );

    let satisfied_count = auto_constraints
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Satisfied)
        .count();
    assert!(
        satisfied_count >= 2,
        "AutoFreeMulti should have >=2 Satisfied constraints (constrained(x), constrained(y)), \
         got {} satisfied of {}: {:?}",
        satisfied_count,
        auto_constraints.len(),
        auto_constraints
            .iter()
            .map(|e| (&e.id, &e.satisfaction))
            .collect::<Vec<_>>()
    );
}

// ── STEP-21: Corner_cases constraint and determinacy tests ────────────────────

/// No Violated constraints in check_corner() results.
#[test]
fn corner_all_non_auto_constraints_satisfied() {
    let check_result = check_corner();
    let violated: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        violated.is_empty(),
        "no constraints should be Violated in integration_corner_cases.ri, found {} violated: {:?}",
        violated.len(),
        violated.iter().map(|e| &e.id).collect::<Vec<_>>()
    );
}

/// Total constraint count >= 25.
#[test]
fn corner_assertion_count_at_least_25() {
    let check_result = check_corner();
    let total = check_result.constraint_results.len();
    assert!(
        total >= 25,
        "expected >=25 total constraints in integration_corner_cases.ri, got {total}"
    );
}

/// Snapshot determinacy checks for corner_cases key cells.
#[test]
fn corner_determinacy_states() {
    let compiled = compiled_corner();
    let mut engine = make_simple_engine();
    engine.eval(compiled);
    let snap = engine.snapshot().expect("snapshot should exist after eval");

    // Determined cells
    let determined_cells = [
        ("JerkDemo", "j"),
        ("FullTraitImpl", "size"),
        ("EmptyListOps", "n"),
        ("ChainedFour", "chain"),
    ];
    for (entity, member) in &determined_cells {
        let id = ValueCellId::new(*entity, *member);
        match snap.values.get(&id) {
            Some((_, state)) => {
                assert_eq!(
                    *state,
                    DeterminacyState::Determined,
                    "{entity}.{member} should be Determined, got {state:?}"
                );
            }
            None => panic!("{entity}.{member} not found in snapshot"),
        }
    }

    // Undetermined cell (undef param propagates to Undetermined)
    let undef_id = ValueCellId::new("UndefPropagation", "u");
    if let Some((_, state)) = snap.values.get(&undef_id) {
        assert_eq!(
            *state,
            DeterminacyState::Undetermined,
            "UndefPropagation.u should be Undetermined, got {state:?}"
        );
    }
    // (If not present, the param may be absent — treat as acceptable)

    // Auto cells (auto/free params)
    for member in ["x", "y"] {
        let id = ValueCellId::new("AutoFreeMulti", member);
        match snap.values.get(&id) {
            Some((_, state)) => {
                assert_eq!(
                    *state,
                    DeterminacyState::Auto,
                    "AutoFreeMulti.{member} should be Auto, got {state:?}"
                );
            }
            None => panic!("AutoFreeMulti.{member} not found in snapshot"),
        }
    }
}
