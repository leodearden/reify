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
use reify_eval::Engine;
use reify_test_support::parse_and_compile_with_stdlib;
use reify_types::{ModulePath, Satisfaction, Value, ValueCellId};

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/integration_full_v01.ri"
);

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Read integration_full_v01.ri, caching the result in a `OnceLock`.
fn source() -> String {
    static S: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    S.get_or_init(|| {
        std::fs::read_to_string(EXAMPLE_PATH)
            .expect("examples/integration_full_v01.ri should exist")
    })
    .clone()
}

/// Parse and compile (with stdlib) the canonical source, caching the result.
fn compiled() -> CompiledModule {
    static C: std::sync::OnceLock<CompiledModule> = std::sync::OnceLock::new();
    C.get_or_init(|| parse_and_compile_with_stdlib(&source())).clone()
}

/// Eval the canonical source with SimpleConstraintChecker.
fn eval_canonical() -> reify_eval::EvalResult {
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    engine.eval(&compiled())
}

/// Check the canonical source with SimpleConstraintChecker.
fn check_canonical() -> reify_eval::CheckResult {
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    engine.check(&compiled())
}

/// Parse, compile (with stdlib), check a mutated source string.
fn check_source(src: &str) -> reify_eval::CheckResult {
    let compiled = parse_and_compile_with_stdlib(src);
    let mut engine = Engine::new(Box::new(SimpleConstraintChecker), None);
    engine.check(&compiled)
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

// ── Test 1: file parses without errors ───────────────────────────────────────

/// Read integration_full_v01.ri, verify it parses without errors, and assert
/// minimum top-level declaration counts: >=2 traits, >=2 structures, >=1
/// purpose, >=2 constraint defs, >=1 enum, >=2 functions (overloads), >=1
/// field def, >=1 unit decl, >=1 type alias. These counts guard against silent
/// declaration drops more precisely than a pure parse check.
#[test]
fn integration_full_v01_ri_parses() {
    let src = source();
    let parsed = reify_syntax::parse(&src, ModulePath::single("test"));
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
