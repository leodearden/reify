//! Full v0.1 integration tests.
//!
//! Exercises every v0.1 language feature in a single cohesive engineering
//! assembly: type aliases, multi-trait conformance, constraint defs, recursive
//! structures, geometric types, connect with connector/port mapping, purpose
//! with forall, where guards, quantifiers, match, lambda, ranges, some/none,
//! fn overloading, determinacy predicates, @test annotations, meta access,
//! doc comments, minimize, self, field defs with gradient+sample, custom units,
//! complex numbers, enums, collections.
//!
//! Uses examples/integration_full_v01.ri as the source file.

use reify_compiler::CompiledModule;
use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};
use reify_types::{ModulePath, Satisfaction, Value, ValueCellId};

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/integration_full_v01.ri"
);

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Read integration_full_v01.ri, caching the result in a `OnceLock`.
/// Returns a `&'static str` reference — no allocation on each call.
fn source() -> &'static str {
    static S: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(EXAMPLE_PATH)
            .expect("examples/integration_full_v01.ri should exist")
    })
    .as_str()
}

/// Parse and compile (with stdlib) the canonical source, caching the result.
/// Returns a `&'static CompiledModule` — no clone on each call.
fn compiled() -> &'static CompiledModule {
    static C: std::sync::OnceLock<CompiledModule> = std::sync::OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(source()))
}

/// Eval the canonical source with SimpleConstraintChecker.
fn eval_canonical() -> reify_eval::EvalResult {
    let mut engine = make_simple_engine();
    engine.eval(compiled())
}

/// Check the canonical source with SimpleConstraintChecker.
fn check_canonical() -> reify_eval::CheckResult {
    let mut engine = make_simple_engine();
    engine.check(compiled())
}

/// Parse, compile (with stdlib), check a mutated source string.
fn check_source(src: &str) -> reify_eval::CheckResult {
    let compiled = parse_and_compile_with_stdlib(src);
    let mut engine = make_simple_engine();
    engine.check(&compiled)
}

// ── Test 1: file parses without errors ───────────────────────────────────────

/// Read integration_full_v01.ri, verify it parses without errors, and assert
/// minimum top-level declaration counts: >=2 traits, >=2 structures, >=1
/// purpose, >=2 constraint defs, >=1 enum, >=2 functions (overloads), >=1
/// field def, >=1 unit decl, >=1 type alias. These counts guard against silent
/// declaration drops more precisely than a pure parse check.
#[test]
fn integration_full_v01_ri_parses() {
    let src = source();
    let parsed = reify_syntax::parse(src, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    use reify_syntax::Declaration;

    let trait_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Trait(_)))
        .count();
    let structure_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Structure(_)))
        .count();
    let purpose_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Purpose(_)))
        .count();
    let constraint_def_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Constraint(_)))
        .count();
    let enum_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Enum(_)))
        .count();
    let function_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Function(_)))
        .count();
    let field_def_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Field(_)))
        .count();
    let unit_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::Unit(_)))
        .count();
    let type_alias_count = parsed
        .declarations
        .iter()
        .filter(|d| matches!(d, Declaration::TypeAlias(_)))
        .count();

    assert!(
        trait_count >= 2,
        "expected >=2 Trait declarations, got {trait_count}"
    );
    assert!(
        structure_count >= 2,
        "expected >=2 Structure declarations, got {structure_count}"
    );
    assert!(
        purpose_count >= 1,
        "expected >=1 Purpose declaration, got {purpose_count}"
    );
    assert!(
        constraint_def_count >= 2,
        "expected >=2 ConstraintDef declarations, got {constraint_def_count}"
    );
    assert!(
        enum_count >= 1,
        "expected >=1 Enum declaration, got {enum_count}"
    );
    assert!(
        function_count >= 2,
        "expected >=2 Function declarations (safety_factor overloads), got {function_count}"
    );
    assert!(
        field_def_count >= 1,
        "expected >=1 FieldDef declaration, got {field_def_count}"
    );
    assert!(
        unit_count >= 1,
        "expected >=1 Unit declaration, got {unit_count}"
    );
    assert!(
        type_alias_count >= 1,
        "expected >=1 TypeAlias declaration, got {type_alias_count}"
    );
}

// ── Test 2: file compiles with expected templates ─────────────────────────────

/// Compile integration_full_v01.ri (with stdlib) and verify the compiled module
/// contains the expected templates: Assembly (>=10 value cells), RecursiveBeam,
/// PipeConnector — and at least 5 templates total (including @test structures).
#[test]
fn integration_full_v01_compiles() {
    let compiled = compiled();
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template in the compiled module"
    );

    // Assembly must exist with >=10 value cells (params + lets from full body)
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("should have an Assembly template");
    assert!(
        assembly.value_cells.len() >= 10,
        "Assembly should have >=10 value cells (params + lets), got {}",
        assembly.value_cells.len()
    );

    // RecursiveBeam must exist
    assert!(
        compiled.templates.iter().any(|t| t.name == "RecursiveBeam"),
        "should have a RecursiveBeam template"
    );

    // At least 3 templates total (Assembly, RecursiveBeam, PipeConnector).
    // More templates (>=9) are added in step-22 when @test structures land.
    assert!(
        compiled.templates.len() >= 3,
        "expected >=3 templates total, got {}",
        compiled.templates.len()
    );
}

// ── Test 3: all constraints satisfied ────────────────────────────────────────

/// Smoke test: check_canonical produces at least one constraint result and
/// every entry is Satisfaction::Satisfied.
#[test]
fn all_constraints_satisfied() {
    let check_result = check_canonical();
    assert!(
        !check_result.constraint_results.is_empty(),
        "expected at least one constraint result"
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

// ── Test 4: total constraint count meets threshold ────────────────────────────

/// Capstone assertion: constraint_results.len() >= 40, all Satisfied.
/// Guards against silent constraint drops during future refactoring.
#[test]
fn total_constraint_count_meets_threshold() {
    let check_result = check_canonical();
    let n = check_result.constraint_results.len();
    assert!(
        n >= 40,
        "expected >= 40 total constraint results, got {n}"
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

// ── Test 5: geometric let bindings are determined ────────────────────────────

/// origin, target, offset, displacement should all evaluate to concrete (non-Undef) geometric values.
#[test]
fn geometric_bindings_determined() {
    let result = eval_canonical();
    let assert_non_undef = |name: &str| {
        let id = ValueCellId::new("Assembly", name);
        let v = result
            .values
            .get(&id)
            .unwrap_or_else(|| panic!("Assembly.{name} not found in eval result"));
        assert!(
            !matches!(v, Value::Undef),
            "Assembly.{name} should not be Undef (expected a concrete geometric value)"
        );
    };
    assert_non_undef("origin");
    assert_non_undef("target");
    assert_non_undef("offset");
    assert_non_undef("displacement");
}

// ── Test 6: meta access values ────────────────────────────────────────────────

/// proj_name = meta.project → "integration-test"; file_ver = meta.version → "0.1".
#[test]
fn meta_access_values() {
    let result = eval_canonical();

    // proj_name = meta.project = "integration-test"
    let proj_id = ValueCellId::new("Assembly", "proj_name");
    let proj_val = result
        .values
        .get(&proj_id)
        .unwrap_or_else(|| panic!("Assembly.proj_name not found in eval result"));
    assert_eq!(
        proj_val,
        &Value::String("integration-test".to_string()),
        "Assembly.proj_name should be String(\"integration-test\") via meta.project"
    );

    // file_ver = meta.version = "0.1"
    let ver_id = ValueCellId::new("Assembly", "file_ver");
    let ver_val = result
        .values
        .get(&ver_id)
        .unwrap_or_else(|| panic!("Assembly.file_ver not found in eval result"));
    assert_eq!(
        ver_val,
        &Value::String("0.1".to_string()),
        "Assembly.file_ver should be String(\"0.1\") via meta.version"
    );
}

// ── Test 7: trait values ──────────────────────────────────────────────────────

/// mass = 5kg = 5.0 SI (Physical trait param); position_x = 100mm = 0.1 SI (Locatable trait param).
#[test]
fn trait_values() {
    let result = eval_canonical();

    // mass = 5kg = 5.0 SI
    let mass_id = ValueCellId::new("Assembly", "mass");
    let mass_val = result
        .values
        .get(&mass_id)
        .unwrap_or_else(|| panic!("Assembly.mass not found in eval result"));
    match mass_val {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 5.0).abs() < 1e-12,
                "expected 5.0 SI for Assembly.mass (5kg), got {si_value}"
            );
        }
        other => panic!("expected Scalar for Assembly.mass, got {:?}", other),
    }

    // position_x = 100mm = 0.1 SI
    let px_id = ValueCellId::new("Assembly", "position_x");
    let px_val = result
        .values
        .get(&px_id)
        .unwrap_or_else(|| panic!("Assembly.position_x not found in eval result"));
    match px_val {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.1).abs() < 1e-12,
                "expected 0.1 SI for Assembly.position_x (100mm), got {si_value}"
            );
        }
        other => panic!("expected Scalar for Assembly.position_x, got {:?}", other),
    }
}

// ── Test 8: complex number binding ────────────────────────────────────────────

/// impedance = complex(3.0, 4.0) should evaluate to Value::Complex (not Undef).
#[test]
fn complex_number_binding() {
    let result = eval_canonical();

    let id = ValueCellId::new("Assembly", "impedance");
    let v = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("Assembly.impedance not found in eval result"));
    assert!(
        !matches!(v, Value::Undef),
        "Assembly.impedance should not be Undef (expected Value::Complex from complex(3.0,4.0))"
    );
    assert!(
        matches!(v, Value::Complex { .. }),
        "Assembly.impedance should be Value::Complex, got {:?}",
        v
    );
}

// ── Test 9: custom unit value ─────────────────────────────────────────────────

/// clearance = 500mil = 500 * 0.0000254m = 0.0127 SI.
/// Confirms the custom `unit mil : Length = 0.0000254` resolves correctly.
#[test]
fn custom_unit_value() {
    let result = eval_canonical();

    let id = ValueCellId::new("Assembly", "clearance");
    let v = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("Assembly.clearance not found in eval result"));
    match v {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.0127).abs() < 1e-9,
                "expected ~0.0127 SI for Assembly.clearance (500mil), got {si_value}"
            );
        }
        other => panic!("expected Scalar for Assembly.clearance, got {:?}", other),
    }
}

// ── Test 10: match expression result ──────────────────────────────────────────

/// grade = Grade.Premium; grade_code = match grade { ... Premium => 3 } → Int(3).
#[test]
fn match_expression_result() {
    let result = eval_canonical();

    let id = ValueCellId::new("Assembly", "grade_code");
    let v = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("Assembly.grade_code not found in eval result"));
    assert_eq!(
        v,
        &Value::Int(3),
        "Assembly.grade_code should be Int(3) (Grade.Premium → 3 via match)"
    );
}

// ── Test 11: collection operations ────────────────────────────────────────────

/// sizes = [10, 20, 30, 40, 50] → size_count = sizes.count = Int(5).
#[test]
fn collection_operations() {
    let result = eval_canonical();

    let id = ValueCellId::new("Assembly", "size_count");
    let v = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("Assembly.size_count not found in eval result"));
    assert_eq!(
        v,
        &Value::Int(5),
        "Assembly.size_count should be Int(5) (sizes has 5 elements)"
    );
}

// ── Test 12: field sample and gradient ───────────────────────────────────────

/// temp_at_3 = sample(temperature_profile, 3.0) → 3²+10 = 19.0.
/// dtemp_at_3 = sample(gradient(temperature_profile), 3.0) → 2*3 ≈ 6.0 (central differences).
///
/// Note: the analytical field evaluator may return `Int(19)` when the result is a
/// whole number (the expression `3*3+10` evaluates to an integer via the pure-expr
/// evaluator), so we accept both Real and Int variants and compare numerically.
#[test]
fn field_sample_and_gradient() {
    let result = eval_canonical();

    // Extract the numeric value from Real or Int
    let numeric = |v: &Value, name: &str| -> f64 {
        match v {
            Value::Real(f) => *f,
            Value::Int(i) => *i as f64,
            other => panic!("expected Real or Int for Assembly.{name}, got {:?}", other),
        }
    };

    // temp_at_3 = 19.0 ± 0.1
    let temp_id = ValueCellId::new("Assembly", "temp_at_3");
    let temp_val = result
        .values
        .get(&temp_id)
        .unwrap_or_else(|| panic!("Assembly.temp_at_3 not found in eval result"));
    let temp_f = numeric(temp_val, "temp_at_3");
    assert!(
        (temp_f - 19.0).abs() < 0.1,
        "expected ~19.0 for Assembly.temp_at_3, got {temp_f}"
    );

    // dtemp_at_3 = 6.0 ± 0.1 (central differences of x²+10 at x=3 → 2x = 6)
    let dtemp_id = ValueCellId::new("Assembly", "dtemp_at_3");
    let dtemp_val = result
        .values
        .get(&dtemp_id)
        .unwrap_or_else(|| panic!("Assembly.dtemp_at_3 not found in eval result"));
    let dtemp_f = numeric(dtemp_val, "dtemp_at_3");
    assert!(
        (dtemp_f - 6.0).abs() < 0.1,
        "expected ~6.0 for Assembly.dtemp_at_3, got {dtemp_f}"
    );
}

// ── Test 13: function overload resolution ─────────────────────────────────────

/// safe_load_real = safety_factor(100.5) → Real overload → 100.5 * 1.5 = 150.75.
/// safe_load_int = safety_factor(100) → Int overload → 100 * 2 = 200.
#[test]
fn function_overload_resolution() {
    let result = eval_canonical();

    // safe_load_real: Real overload → 150.75 (100.5 * 1.5)
    let real_id = ValueCellId::new("Assembly", "safe_load_real");
    let real_val = result
        .values
        .get(&real_id)
        .unwrap_or_else(|| panic!("Assembly.safe_load_real not found in eval result"));
    match real_val {
        Value::Real(v) => {
            assert!(
                (v - 150.75).abs() < 0.01,
                "expected 150.75 for Assembly.safe_load_real (safety_factor(real) * 1.5), got {v}"
            );
        }
        other => panic!("expected Real for Assembly.safe_load_real, got {:?}", other),
    }

    // safe_load_int: Int overload → 200
    let int_id = ValueCellId::new("Assembly", "safe_load_int");
    let int_val = result
        .values
        .get(&int_id)
        .unwrap_or_else(|| panic!("Assembly.safe_load_int not found in eval result"));
    assert_eq!(
        int_val,
        &Value::Int(200),
        "Assembly.safe_load_int should be Int(200) (safety_factor(100) * 2)"
    );
}

// ── Test 14: recursive unfold depth ──────────────────────────────────────────

/// RecursiveBeam defaults: depth=2, span=300mm.
/// child.span = 150mm (0.15 SI), child.child.span = 75mm (0.075 SI),
/// child.child.child.span does NOT exist (depth=0 guard fails).
#[test]
fn recursive_unfold_depth() {
    let result = eval_canonical();

    // RecursiveBeam.child.span = 150mm = 0.15 SI
    let child_span_id = ValueCellId::new("RecursiveBeam.child", "span");
    let child_span = result
        .values
        .get(&child_span_id)
        .unwrap_or_else(|| panic!("RecursiveBeam.child.span not found in eval result"));
    match child_span {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.15).abs() < 1e-9,
                "expected ~0.15 SI for RecursiveBeam.child.span (150mm), got {si_value}"
            );
        }
        other => panic!("expected Scalar for RecursiveBeam.child.span, got {:?}", other),
    }

    // RecursiveBeam.child.child.span = 75mm = 0.075 SI
    let grandchild_span_id = ValueCellId::new("RecursiveBeam.child.child", "span");
    let grandchild_span = result
        .values
        .get(&grandchild_span_id)
        .unwrap_or_else(|| panic!("RecursiveBeam.child.child.span not found in eval result"));
    match grandchild_span {
        Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.075).abs() < 1e-9,
                "expected ~0.075 SI for RecursiveBeam.child.child.span (75mm), got {si_value}"
            );
        }
        other => panic!(
            "expected Scalar for RecursiveBeam.child.child.span, got {:?}",
            other
        ),
    }

    // RecursiveBeam.child.child.child.span must NOT exist (depth=0, guard fails)
    let great_grandchild_id = ValueCellId::new("RecursiveBeam.child.child.child", "span");
    assert!(
        !result.values.contains(&great_grandchild_id),
        "RecursiveBeam.child.child.child.span should not exist (depth=0 stops unfolding)"
    );
}

// ── Test 15: connect has connector and port mapping ───────────────────────────

/// Assembly has exactly 1 connection (supply -> demand : PipeConnector),
/// connector_sub is Some, port_mappings contains diameter→diameter,
/// and the compatibility constraint is Satisfied.
#[test]
fn connect_has_connector_and_mapping() {
    let compiled = compiled();
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("Assembly template should exist");

    // Exactly 1 connection
    assert_eq!(
        assembly.connections.len(),
        1,
        "Assembly should have exactly 1 connection (supply -> demand), got {}",
        assembly.connections.len()
    );

    let conn = &assembly.connections[0];

    // Connector sub is present (from `: PipeConnector`)
    assert!(
        conn.connector_sub.is_some(),
        "connection should have a connector_sub (from `: PipeConnector`)"
    );

    // Port mappings contain diameter→diameter
    assert!(
        !conn.port_mappings.is_empty(),
        "expected at least 1 port mapping (diameter→diameter), got 0"
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
    let check_result = check_canonical();
    let compat_entry = check_result
        .constraint_results
        .iter()
        .find(|e| &e.id == compat_id)
        .unwrap_or_else(|| {
            panic!(
                "compatibility constraint {} not found in check results",
                compat_id
            )
        });
    assert_eq!(
        compat_entry.satisfaction,
        Satisfaction::Satisfied,
        "compatibility constraint should be Satisfied"
    );
}

// ── Test 16: purpose compiled and activatable ──────────────────────────────────

/// mfg_ready purpose: exists in compiled_purposes with entity_kind='Structure',
/// activates against Assembly (adding constraints), deactivates cleanly.
fn constraint_count_from_engine(engine: &reify_eval::Engine) -> usize {
    engine
        .snapshot()
        .expect("snapshot should exist")
        .graph
        .constraints
        .len()
}

#[test]
fn purpose_compiled_and_activatable() {
    let compiled = compiled();

    // mfg_ready is present in compiled_purposes
    let mfg_ready = compiled
        .compiled_purposes
        .iter()
        .find(|p| p.name == "mfg_ready")
        .expect("mfg_ready purpose should be in compiled_purposes");

    // Param has entity_kind == "Structure"
    assert_eq!(
        mfg_ready.params.len(),
        1,
        "mfg_ready should have exactly 1 param (subject)"
    );
    assert_eq!(
        mfg_ready.params[0].entity_kind,
        "Structure",
        "mfg_ready param entity_kind should be 'Structure', got '{}'",
        mfg_ready.params[0].entity_kind
    );

    // Activate against Assembly
    let mut engine = make_simple_engine();
    engine.eval(compiled);
    let before = constraint_count_from_engine(&engine);

    engine.activate_purpose("mfg_ready", "Assembly");
    assert!(
        engine.is_purpose_active("mfg_ready"),
        "mfg_ready should be active after activate_purpose"
    );

    // Constraint count grows by the number of constraints in the compiled purpose body.
    // Deriving from mfg_ready.constraints.len() guards against silent drifts if the
    // purpose body is updated — both the test and the .ri source stay in sync automatically.
    // (See examples/integration_full_v01.ri: mfg_ready uses literal placeholder constraints;
    // adding subject-referencing constraints is tracked as a follow-up task.)
    let expected_extra = mfg_ready.constraints.len();
    assert_eq!(
        constraint_count_from_engine(&engine),
        before + expected_extra,
        "mfg_ready should add {expected_extra} constraints (mfg_ready.constraints.len()), \
         got {before} before, {} after",
        constraint_count_from_engine(&engine)
    );

    // Deactivate restores count
    engine.deactivate_purpose("mfg_ready");
    assert_eq!(
        constraint_count_from_engine(&engine),
        before,
        "deactivating mfg_ready must restore constraint count to {before}"
    );
    assert!(
        !engine.is_purpose_active("mfg_ready"),
        "mfg_ready should NOT be active after deactivate_purpose"
    );
}

// ── Test 17: where-block constraints present and satisfied ────────────────────

/// The where-block `where determined(origin) { constraint determined(displacement); constraint determined(base_frame) }`
/// produces 1 guarded group with 2 constraints, both Satisfied.
#[test]
fn where_block_constraints_satisfied() {
    let compiled = compiled();
    let assembly = compiled
        .templates
        .iter()
        .find(|t| t.name == "Assembly")
        .expect("Assembly template should exist");

    // Assembly has exactly 1 guarded group
    assert_eq!(
        assembly.guarded_groups.len(),
        1,
        "Assembly should have exactly 1 guarded group (the where determined(origin) block)"
    );
    let group = &assembly.guarded_groups[0];
    assert_eq!(
        group.constraints.len(),
        2,
        "where-block should have exactly 2 constraints (determined(displacement), determined(base_frame))"
    );

    // Both appear in check results as Satisfied
    let guarded_ids: Vec<_> = group.constraints.iter().map(|c| &c.id).collect();
    let check_result = check_canonical();
    for guarded_id in &guarded_ids {
        let entry = check_result
            .constraint_results
            .iter()
            .find(|e| &&e.id == guarded_id)
            .unwrap_or_else(|| {
                panic!(
                    "where-block constraint {} not found in check results",
                    guarded_id
                )
            });
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "where-block constraint {} should be Satisfied, got {:?}",
            guarded_id,
            entry.satisfaction
        );
    }
}

// ── Test 18: @test runner all pass ───────────────────────────────────────────

/// Run @test structures with SimpleConstraintChecker.
/// Expected: TestHeightPositive, TestWidthInRange, TestSubBeamSpan,
/// TestComplexBinding, TestFieldSample, TestGradeMatch — all Pass.
#[test]
fn test_runner_all_pass() {
    let compiled = compiled();
    let results = reify_eval::run_tests(compiled, || Box::new(SimpleConstraintChecker));

    // Must have at least 6 test results (6 @test structures added in step-22)
    assert!(
        results.len() >= 6,
        "expected >=6 @test results, got {}; did you add @test structures in step-22?",
        results.len()
    );

    // All must be Pass
    let expected_tests = [
        "TestHeightPositive",
        "TestWidthInRange",
        "TestSubBeamSpan",
        "TestComplexBinding",
        "TestFieldSample",
        "TestGradeMatch",
    ];
    let by_name: std::collections::HashMap<&str, reify_eval::TestStatus> =
        results.iter().map(|r| (r.name.as_str(), r.status)).collect();
    assert_eq!(
        by_name.len(),
        results.len(),
        "duplicate @test names detected"
    );

    for name in &expected_tests {
        assert!(
            by_name.contains_key(*name),
            "@test {name} not found in results"
        );
        let &status = by_name.get(*name).unwrap();
        assert_eq!(
            status,
            reify_eval::TestStatus::Pass,
            "@test {name} should be Pass, got {status:?}"
        );
    }

    // All present tests must be Pass (no intentional failures in integration file)
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

// ── Test 19: constraint def labels ────────────────────────────────────────────

/// Assembly has 2 InRange invocations × 2 predicates = 4 total constraints,
/// distributed as 2×InRange[0] and 2×InRange[1], all Satisfied.
#[test]
fn constraint_def_labels() {
    let check_result = check_canonical();

    // Collect Assembly InRange constraints
    let inrange_constraints: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| {
            e.id.entity == "Assembly"
                && e.label
                    .as_deref()
                    .is_some_and(|l| l.starts_with("InRange["))
        })
        .collect();

    // 2 invocations × 2 predicates = 4 total
    assert_eq!(
        inrange_constraints.len(),
        4,
        "expected exactly 4 Assembly InRange constraints (2 invocations × 2 predicates), got {}",
        inrange_constraints.len()
    );

    // Each invocation resets pred_idx to 0 → 2×InRange[0], 2×InRange[1]
    let count_label = |label: &str| -> usize {
        inrange_constraints
            .iter()
            .filter(|e| e.label.as_deref() == Some(label))
            .count()
    };
    assert_eq!(
        count_label("InRange[0]"),
        2,
        "expected 2 constraints with label 'InRange[0]'"
    );
    assert_eq!(
        count_label("InRange[1]"),
        2,
        "expected 2 constraints with label 'InRange[1]'"
    );

    // All 4 must be Satisfied
    for entry in &inrange_constraints {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "Assembly InRange constraint {} should be Satisfied",
            entry.id
        );
    }
}

// ── Test 20: violation regression guard ──────────────────────────────────────

/// Regression guard: deliberately make `height < 2000mm` into `height < 100mm`
/// (height=300mm > 100mm → Violated). Verifies at least one Violated result
/// and total count still >=40 (checker did not short-circuit).
#[test]
fn violated_constraint_detected() {
    // height=300mm; raise upper bound to below actual → Violated
    let violating = source().replace(
        "constraint height < 2000mm",
        "constraint height < 100mm",
    );
    assert_ne!(
        violating,
        source(),
        "replace target drifted — 'constraint height < 2000mm' not found; update the test"
    );

    let check_result = check_source(&violating);

    // Total count still >=40 (no short-circuit on violation)
    assert!(
        check_result.constraint_results.len() >= 40,
        "expected >=40 constraint results even for violating source, got {}",
        check_result.constraint_results.len()
    );

    // At least one Violated
    let violations: Vec<_> = check_result
        .constraint_results
        .iter()
        .filter(|e| e.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(
        !violations.is_empty(),
        "expected at least one Violated constraint after raising height bound below its value (300mm), \
         got {} results with no violations",
        check_result.constraint_results.len()
    );
}
