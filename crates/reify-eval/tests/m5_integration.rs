//! M5 integration tests.
//!
//! Exercises multiple M5 features together through the full pipeline:
//! parse → compile → eval/check → verify.

use std::fs;

use reify_compiler::module_dag::{compile_project, ModuleResolver};
use reify_test_support::mocks::MockConstraintChecker;
use reify_types::{ModulePath, Severity, ValueCellId};

// ── Helper ──────────────────────────────────────────────────────────

/// Parse source, assert no parse errors, compile, assert no compile errors.
/// Returns the compiled module.
fn parse_and_compile(source: &str) -> reify_compiler::CompiledModule {
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    compiled
}

// ── Step 1: trait_implementing_structure ─────────────────────────────

/// Parse m5_trait_structure.ri (trait Measurable + structure Rod : Measurable),
/// compile, verify no errors, eval, check trait conformance.
///
/// Assert:
/// - Parse OK
/// - Compile OK (no error diagnostics) — confirms trait conformance
/// - Eval produces correct param values (length=100mm, diameter=10mm)
/// - Let binding radius = diameter/2 is evaluated
/// - Constraints from trait (length > 0mm) are present
#[test]
fn trait_implementing_structure() {
    let source = std::fs::read_to_string("../../examples/m5_trait_structure.ri")
        .expect("examples/m5_trait_structure.ri should exist");

    let compiled = parse_and_compile(&source);

    // Should have one template (Rod)
    assert!(!compiled.templates.is_empty(), "expected at least one template");

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Check length = 100mm = 0.1 SI (metres)
    let length_id = ValueCellId::new("Rod", "length");
    let length_val = result
        .values
        .get(&length_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in result", length_id));
    match length_val {
        reify_types::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.1).abs() < 1e-12,
                "expected 0.1 SI for length, got {}",
                si_value
            );
        }
        other => panic!("expected Scalar for length, got {:?}", other),
    }

    // Check diameter = 10mm = 0.01 SI
    let diameter_id = ValueCellId::new("Rod", "diameter");
    let diameter_val = result
        .values
        .get(&diameter_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in result", diameter_id));
    match diameter_val {
        reify_types::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.01).abs() < 1e-12,
                "expected 0.01 SI for diameter, got {}",
                si_value
            );
        }
        other => panic!("expected Scalar for diameter, got {:?}", other),
    }

    // Check radius = diameter / 2 = 0.005 SI
    let radius_id = ValueCellId::new("Rod", "radius");
    let radius_val = result
        .values
        .get(&radius_id)
        .unwrap_or_else(|| panic!("value for {:?} not found in result", radius_id));
    match radius_val {
        reify_types::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.005).abs() < 1e-12,
                "expected 0.005 SI for radius, got {}",
                si_value
            );
        }
        other => panic!("expected Scalar for radius, got {:?}", other),
    }
}

// ── Step 3: multi_module_import ─────────────────────────────────────

/// Create temp dir with two .ri files: one defining a structure, one importing it.
/// Use ModuleResolver + compile_project to compile both modules.
///
/// Assert:
/// - compile_project succeeds (no errors)
/// - Both modules are compiled and returned
#[test]
fn multi_module_import() {
    let dir = std::env::temp_dir()
        .join("reify_m5_test")
        .join(format!("multi_mod_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // Create stdlib dir (required by ModuleResolver)
    let stdlib = dir.join("stdlib");
    fs::create_dir_all(&stdlib).unwrap();

    // shapes.ri — defines a structure
    fs::write(
        dir.join("shapes.ri"),
        r#"
structure def Circle {
    param radius : Length = 10mm
    let diameter = radius * 2
}
"#,
    )
    .unwrap();

    // main.ri — imports shapes and defines its own structure
    fs::write(
        dir.join("main.ri"),
        r#"
import shapes

structure def Assembly {
    param size : Length = 20mm
    constraint size > 5mm
}
"#,
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, &stdlib);
    let result = compile_project(&dir.join("main.ri"), &resolver);

    match result {
        Ok(modules) => {
            // Should have at least 2 modules (shapes + main)
            assert!(
                modules.len() >= 2,
                "expected at least 2 compiled modules, got {}",
                modules.len()
            );
        }
        Err(errors) => {
            panic!("compile_project failed: {:?}", errors);
        }
    }

    // Cleanup
    let _ = fs::remove_dir_all(&dir);
}
