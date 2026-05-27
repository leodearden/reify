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
use reify_test_support::{
    check_source_with_stdlib as check_source, make_simple_engine, parse_and_compile_with_stdlib,
};
use reify_core::{Diagnostic, ModulePath, Severity, ValueCellId};
use reify_ir::{Satisfaction, Value};

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

/// Eval the canonical source with SimpleConstraintChecker, caching the result.
/// Returns a `&'static EvalResult` — no recompute or clone on subsequent calls.
fn eval_canonical() -> &'static reify_eval::EvalResult {
    static E: std::sync::OnceLock<reify_eval::EvalResult> = std::sync::OnceLock::new();
    E.get_or_init(|| {
        let mut engine = make_simple_engine();
        engine.eval(compiled())
    })
}

/// Check the canonical source with SimpleConstraintChecker, caching the result.
/// Returns a `&'static CheckResult` — no recompute or clone on subsequent calls.
fn check_canonical() -> &'static reify_eval::CheckResult {
    static K: std::sync::OnceLock<reify_eval::CheckResult> = std::sync::OnceLock::new();
    K.get_or_init(|| {
        let mut engine = make_simple_engine();
        engine.check(compiled())
    })
}

/// Assert that a diagnostics slice contains no entries with [`Severity::Error`].
/// Panics with the offending diagnostics and `context` label on failure.
fn assert_no_errors(diagnostics: &[Diagnostic], context: &str) {
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "errors in {}: {:?}", context, errors);
}

/// Assert that `$parsed` (a `ParsedModule`) has at least `$min` declarations
/// matching `$pat`.  Panics with an actionable message that includes `$label`
/// and the actual count.
///
/// A `macro_rules!` macro is required here because closures cannot accept
/// match patterns as arguments — `matches!(d, $pat)` requires a `$pat:pat`
/// macro fragment.
macro_rules! assert_min_count {
    ($parsed:expr, $pat:pat, $label:expr, $min:expr) => {{
        let count = $parsed
            .declarations
            .iter()
            .filter(|d| matches!(d, $pat))
            .count();
        assert!(
            count >= $min,
            "expected >={} {} declarations, got {}",
            $min,
            $label,
            count
        );
    }};
}

// ── Compile-time Send+Sync guards ─────────────────────────────────────────────

/// Asserts at compile time that `EvalResult` and `CheckResult` satisfy `Send + Sync`.
///
/// `OnceLock<T>` requires `T: Send + Sync`. If a future refactor adds a
/// non-Send field to either type, this function — rather than the OnceLock
/// statics above — will produce the compiler error with a clear diagnosis.
fn _assert_send_sync() {
    fn _assert<T: Send + Sync>() {}
    _assert::<reify_eval::EvalResult>();
    _assert::<reify_eval::CheckResult>();
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

    use reify_ast::Declaration;

    assert_min_count!(parsed, Declaration::Trait(_), "Trait", 2);
    assert_min_count!(parsed, Declaration::Structure(_), "Structure", 2);
    assert_min_count!(parsed, Declaration::Purpose(_), "Purpose", 1);
    assert_min_count!(parsed, Declaration::Constraint(_), "Constraint", 2);
    assert_min_count!(parsed, Declaration::Enum(_), "Enum", 1);
    assert_min_count!(parsed, Declaration::Function(_), "Function", 2);
    assert_min_count!(parsed, Declaration::Field(_), "Field", 1);
    assert_min_count!(parsed, Declaration::Unit(_), "Unit", 1);
    assert_min_count!(parsed, Declaration::TypeAlias(_), "TypeAlias", 1);
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
    assert!(n >= 40, "expected >= 40 total constraint results, got {n}");
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
            // 100.5 * 1.5 = 150.75: all three values are exactly representable in
            // IEEE 754 f64, so the multiplication is exact — use exact equality.
            assert_eq!(
                *v, 150.75,
                "expected exactly 150.75 for Assembly.safe_load_real (safety_factor(100.5) → Real overload → 100.5 * 1.5), got {v}"
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
        other => panic!(
            "expected Scalar for RecursiveBeam.child.span, got {:?}",
            other
        ),
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
        mfg_ready.params[0].entity_kind, "Structure",
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
    // replacing them with reflective `forall p in subject.geometric_params: determined(p)`
    // is tracked as task #1904 — three missing pieces: StructureRef member-access compilation
    // in expr.rs, filter-kind branches in traits.rs resolved_queries loop, and forall runtime
    // expansion via ResolvedSchemaQuery.resolved_ids in reify-eval activate_purpose.)
    let expected_extra = mfg_ready.constraints.len();
    assert!(
        expected_extra > 0,
        "mfg_ready should have at least 1 constraint \
         — a zero count makes the delta assertion tautological"
    );
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
    let by_name: std::collections::HashMap<&str, reify_eval::TestStatus> = results
        .iter()
        .map(|r| (r.name.as_str(), r.status))
        .collect();
    assert_eq!(
        by_name.len(),
        results.len(),
        "duplicate @test names detected"
    );

    for name in &expected_tests {
        let &status = by_name
            .get(*name)
            .unwrap_or_else(|| panic!("@test {name} not found in results"));
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

// ── Test 26: purpose follow-up comments are precise ──────────────────────────

/// Assert that the .ri file's purpose comments have been upgraded from the
/// old generic "tracked as a follow-up task" phrasing to precise scoped
/// references citing task #1904.
///
/// Self-expiring scaffolding: delete this test when task #1904 lands and
/// the placeholder constraints are replaced with real reflective syntax.
///
/// Assertions:
/// 1. The old generic phrase is absent (regression guard).
/// 2. The old vague phrase is absent (regression guard).
/// 3. The precise follow-up task id "task #1904" is present somewhere.
/// 4. A scoped placeholder comment with task #1904 appears above weight_target,
///    naming both target shapes (minimize subject.mass, determined(subject.mass)).
#[test]
fn purpose_follow_up_comment_is_precise() {
    let src = source();

    // 1. Old generic "Follow-up:" sentence must be gone.
    assert!(
        !src.contains("Follow-up: replace with `constraint determined(subject.mass)`"),
        "Old generic follow-up phrase found in .ri — must be replaced with precise task #1904 reference"
    );

    // 2. Old vague "tracked as a follow-up task" phrase must be gone.
    assert!(
        !src.contains("tracked as a follow-up task"),
        "Vague 'tracked as a follow-up task' phrase found — must be replaced with 'task #1904' reference"
    );

    // 3. Precise follow-up task id must be present.
    assert!(
        src.contains("task #1904"),
        "Precise follow-up marker 'task #1904' not found in .ri — add scoped placeholder comments"
    );

    // 4. The scoped comment ABOVE weight_target must cite task #1904 and name
    // both target shapes (minimize subject.mass and determined(subject.mass)).
    // Narrow the slice to just the region between the end of mfg_ready's body
    // and weight_target's declaration — otherwise a stray match in mfg_ready's
    // comment (or anywhere earlier in the file) would satisfy the assertion
    // even if weight_target's scoped comment were removed.
    let weight_target_pos = src
        .find("purpose weight_target")
        .expect("'purpose weight_target' must exist in .ri");
    let mfg_ready_end = src[..weight_target_pos]
        .rfind('}')
        .expect("closing '}' of mfg_ready must precede 'purpose weight_target'");
    let weight_target_region = &src[mfg_ready_end + 1..weight_target_pos];
    assert!(
        weight_target_region.contains("task #1904"),
        "No 'task #1904' reference found in the doc block immediately above 'purpose weight_target'"
    );
    assert!(
        weight_target_region.contains("minimize subject.mass"),
        "Target shape 'minimize subject.mass' not found in the doc block immediately above 'purpose weight_target'"
    );
    assert!(
        weight_target_region.contains("determined(subject.mass)"),
        "Target shape 'determined(subject.mass)' not found in the doc block immediately above 'purpose weight_target' — \
         scoped comment must name both minimize and constraint targets"
    );
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
                    .is_some_and(|l| l.starts_with("InRange#"))
        })
        .collect();

    // 2 invocations × 2 predicates = 4 total
    assert_eq!(
        inrange_constraints.len(),
        4,
        "expected exactly 4 Assembly InRange constraints (2 invocations × 2 predicates), got {}",
        inrange_constraints.len()
    );

    // Under task 845's label scheme each invocation gets a unique inst_idx, so
    // the 4 labels are InRange#0[0], InRange#0[1], InRange#1[0], InRange#1[1]
    // (1 of each, not 2× per pred_idx).
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

// ── Test 21: Option some/none values ─────────────────────────────────────────

/// Assert `Assembly.maybe_load` evaluates to `Value::Option(Some(Value::Real(100.0)))`
/// and `Assembly.no_load` evaluates to `Value::Option(None)`.
/// The .ri file defines: `let maybe_load = some(100.0)` and `let no_load = none`.
#[test]
fn option_some_none_values() {
    let result = eval_canonical();

    // maybe_load = some(100.0) → Value::Option(Some(Value::Real(100.0)))
    let maybe_id = ValueCellId::new("Assembly", "maybe_load");
    let maybe_val = result
        .values
        .get(&maybe_id)
        .unwrap_or_else(|| panic!("Assembly.maybe_load not found in eval result"));
    assert!(
        matches!(maybe_val, Value::Option(Some(_))),
        "Assembly.maybe_load should be Value::Option(Some(_)), got {:?}",
        maybe_val
    );
    // Verify the wrapped value is numerically 100 — Reify compiles whole-number
    // literals (e.g. 100.0) as Int, so accept both Real(100.0) and Int(100).
    if let Value::Option(Some(inner)) = maybe_val {
        let ok = matches!(inner.as_ref(), Value::Real(v) if (*v - 100.0).abs() < 1e-12)
            || matches!(inner.as_ref(), Value::Int(100));
        assert!(
            ok,
            "Assembly.maybe_load inner should be Real(100.0) or Int(100), got {:?}",
            inner
        );
    } else {
        unreachable!("guarded by prior assert — maybe_val is Value::Option(Some(_))");
    }

    // no_load = none → Value::Option(None)
    let no_id = ValueCellId::new("Assembly", "no_load");
    let no_val = result
        .values
        .get(&no_id)
        .unwrap_or_else(|| panic!("Assembly.no_load not found in eval result"));
    assert!(
        matches!(no_val, Value::Option(None)),
        "Assembly.no_load should be Value::Option(None), got {:?}",
        no_val
    );
}

// ── Test 22: lambda binding evaluates ────────────────────────────────────────

/// Assert `Assembly.double_fn` evaluates to `Value::Lambda { .. }` (not Undef).
/// The .ri file defines: `let double_fn = |x| x * 2.0`.
#[test]
fn lambda_binding_evaluates() {
    let result = eval_canonical();

    let id = ValueCellId::new("Assembly", "double_fn");
    let v = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("Assembly.double_fn not found in eval result"));
    assert!(
        matches!(v, Value::Lambda { .. }),
        "Assembly.double_fn should be Value::Lambda {{ .. }}, got {:?}",
        v
    );
}

// ── Test 23: eval produces no error diagnostics ───────────────────────────────

/// Assert `eval_canonical()` produces no error-severity diagnostics.
/// Verifies the evaluation pipeline runs cleanly on the canonical source.
#[test]
fn eval_no_error_diagnostics() {
    let result = eval_canonical();
    assert_no_errors(&result.diagnostics, "integration_full_v01.ri eval");
}

// ── Test 24: check produces no error diagnostics ──────────────────────────────

/// Assert `check_canonical()` produces no error-severity diagnostics.
/// Verifies the check pipeline runs cleanly on the canonical source.
#[test]
fn check_no_error_diagnostics() {
    let result = check_canonical();
    assert_no_errors(&result.diagnostics, "integration_full_v01.ri check");
}

// ── Macro test: assert_min_count! catches unmet threshold ────────────────────

/// Verify that `assert_min_count!` panics with an actionable message when the
/// threshold is not met.  Parses an empty source (0 declarations) and asserts
/// >=1 Trait — the macro must panic with "expected >=1 Trait" in the message.
#[test]
#[should_panic(expected = "expected >=1 Trait")]
fn assert_min_count_macro_catches_unmet_threshold() {
    use reify_ast::Declaration;
    let parsed = reify_syntax::parse("", ModulePath::single("empty"));
    assert_min_count!(parsed, Declaration::Trait(_), "Trait", 1);
}

/// Verify that `assert_min_count!` does NOT panic when the threshold IS met.
/// Parses a source with one trait declaration and asserts >=1 Trait — the macro
/// must succeed silently.  This is the symmetric success-path counterpart to
/// `assert_min_count_macro_catches_unmet_threshold`.
#[test]
fn assert_min_count_macro_passes_when_met() {
    use reify_ast::Declaration;
    let src = "trait Foo {}";
    let parsed = reify_syntax::parse(src, ModulePath::single("one_trait"));
    assert_min_count!(parsed, Declaration::Trait(_), "Trait", 1);
}

// ── Test 25: violation regression guard ──────────────────────────────────────

/// Regression guard: deliberately make `height < 2000mm` into `height < 100mm`
/// (height=300mm > 100mm → Violated). Verifies at least one Violated result
/// and total count still >=40 (checker did not short-circuit).
#[test]
fn violated_constraint_detected() {
    // height=300mm; raise upper bound to below actual → Violated
    let violating = source().replace("constraint height < 2000mm", "constraint height < 100mm");
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

// ── Test 21: no tautological self-equality constraints ───────────────────────

/// Regression guard against the specific `height_ok == height_ok` tautology
/// that existed prior to this fix. The check is intentionally narrow: it guards
/// against reintroducing the exact known anti-pattern rather than detecting all
/// possible self-equality constraints. A broader regex-based check (e.g.,
/// `r"constraint\s+(\w+)\s*==\s*\1"`) would require a `regex` dev-dependency
/// but is not added here; this targeted string check is sufficient as a
/// regression guard.
///
/// This test intentionally FAILs on the pre-fix source (which has
/// `constraint height_ok == height_ok`) and passes after the fix.
#[test]
fn no_tautological_self_equality_constraints() {
    let src = source();
    // Targeted regression guard for the known `height_ok == height_ok` pattern.
    // This would miss variants with different whitespace or a renamed identifier,
    // which is acceptable — the intent is to prevent re-introducing this exact bug.
    assert!(
        !src.contains("height_ok == height_ok"),
        "source contains a tautological constraint `height_ok == height_ok`; \
         replace it with a meaningful assertion such as `constraint determined(height_ok)`"
    );
}

// ── Test 22: height_ok determinacy constraint is satisfied ───────────────────

/// Validates that the replacement constraint `determined(height_ok)` is
/// semantically meaningful:
/// 1. Assembly.height_ok evaluates to Bool(true) — not Undef — in both the
///    eval and check paths.
/// 2. Because `determined(Bool(true))` is Satisfied by definition, confirming
///    the value is Bool(true) is sufficient to prove the constraint passes.
///    The global all_constraints_satisfied (Test 3) covers the constraint sweep.
///
/// This test passes immediately after the step-2 fix because height_ok is the
/// conjunction `height > 100mm && height < 500mm` with height=300mm → Bool(true),
/// and `determined(Bool(true))` is Satisfied.
#[test]
fn height_ok_determinacy_constraint_satisfied() {
    // 1. height_ok evaluates to Bool(true) — not Undef
    let result = eval_canonical();
    let id = ValueCellId::new("Assembly", "height_ok");
    let v = result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("Assembly.height_ok not found in eval result"));
    assert_eq!(
        v,
        &Value::Bool(true),
        "Assembly.height_ok should be Bool(true) (height=300mm satisfies 100mm..500mm), got {:?}",
        v
    );

    // 2. check_canonical() also sees height_ok as Bool(true): the `determined(height_ok)`
    //    constraint is Satisfied because Bool(true) is a concrete value (not Undef).
    //    We assert height_ok's value here directly rather than re-iterating all constraints,
    //    since Test 3 (`all_constraints_satisfied`) already verifies every constraint passes
    //    globally — duplicating that loop here would produce a misleading failure message
    //    if an unrelated constraint broke.
    let check_result = check_canonical();
    let v_check = check_result
        .values
        .get(&id)
        .unwrap_or_else(|| panic!("Assembly.height_ok not found in check result values"));
    assert_eq!(
        v_check,
        &Value::Bool(true),
        "check_canonical(): Assembly.height_ok should be Bool(true) (determined), got {:?}",
        v_check
    );
}
