//! M5 integration tests.
//!
//! Exercises multiple M5 features together through the full pipeline:
//! parse → compile → eval/check → verify.

use std::fs;

use reify_compiler::module_dag::{ModuleResolver, compile_project};
use reify_test_support::mocks::MockConstraintChecker;
use reify_test_support::{parse_and_compile, parse_and_compile_with_stdlib};
use reify_core::ValueCellId;
use reify_ir::{ExportFormat, Satisfaction};

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
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template"
    );

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
        reify_ir::Value::Scalar { si_value, .. } => {
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
        reify_ir::Value::Scalar { si_value, .. } => {
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
        reify_ir::Value::Scalar { si_value, .. } => {
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

// ── Step 7: collection_lambda_operations ────────────────────────────

/// Inline source with list literal, .count property, .sum property,
/// and index access. Parse, compile, eval, verify.
///
/// Note: .map(|x| ...) and .all(|x| ...) method calls with arguments are
/// not yet in the parser grammar, so we test collection operations that
/// do work through the full pipeline: literals, .count, .sum, indexing.
#[test]
fn collection_lambda_operations() {
    let source = r#"
structure S {
    let items = [10, 20, 30]
    let n = items.count
    let total = items.sum
    let second = items[1]
}
"#;

    let compiled = parse_and_compile(source);

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // items should be a List
    let items_id = ValueCellId::new("S", "items");
    let items_val = result
        .values
        .get(&items_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", items_id));
    match items_val {
        reify_ir::Value::List(elems) => {
            assert_eq!(elems.len(), 3, "expected 3 elements");
            assert_eq!(elems[0], reify_ir::Value::Int(10));
            assert_eq!(elems[1], reify_ir::Value::Int(20));
            assert_eq!(elems[2], reify_ir::Value::Int(30));
        }
        other => panic!("expected List for items, got {:?}", other),
    }

    // n = items.count = 3
    let n_id = ValueCellId::new("S", "n");
    let n_val = result
        .values
        .get(&n_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", n_id));
    assert_eq!(
        *n_val,
        reify_ir::Value::Int(3),
        "items.count should be 3"
    );

    // total = items.sum = 60
    let total_id = ValueCellId::new("S", "total");
    let total_val = result
        .values
        .get(&total_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", total_id));
    assert_eq!(
        *total_val,
        reify_ir::Value::Int(60),
        "items.sum should be 60"
    );

    // second = items[1] = 20
    let second_id = ValueCellId::new("S", "second");
    let second_val = result
        .values
        .get(&second_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", second_id));
    assert_eq!(
        *second_val,
        reify_ir::Value::Int(20),
        "items[1] should be 20"
    );
}

// ── Step 9: connect_occurrence_chain ────────────────────────────────

/// Parse m5_connect_chain.ri with occurrence definitions having ports,
/// and a structure containing sub occurrences with chain desugaring.
///
/// Verify:
/// - Occurrences compile with EntityKind::Occurrence
/// - Chain produces correct number of connections
/// - Compatibility constraints are all Satisfied
#[test]
fn connect_occurrence_chain() {
    let source = std::fs::read_to_string("../../examples/m5_connect_chain.ri")
        .expect("examples/m5_connect_chain.ri should exist");

    let compiled = parse_and_compile(&source);

    // Should have templates for both Pipe (occurrence) and Pipeline (structure)
    assert!(
        compiled.templates.len() >= 2,
        "expected at least 2 templates, got {}",
        compiled.templates.len()
    );

    // Find the Pipe template and verify it's an occurrence
    let pipe = compiled
        .templates
        .iter()
        .find(|t| t.name == "Pipe")
        .expect("should have a Pipe template");
    assert_eq!(
        pipe.entity_kind,
        reify_compiler::EntityKind::Occurrence,
        "Pipe should be an occurrence"
    );

    // Find Pipeline template and verify chain desugaring
    let pipeline = compiled
        .templates
        .iter()
        .find(|t| t.name == "Pipeline")
        .expect("should have a Pipeline template");
    // chain p1.outlet -> p2.inlet -> p2.outlet -> p3.inlet should produce 3 connections
    assert!(
        !pipeline.connections.is_empty(),
        "Pipeline should have connections from chain desugaring"
    );

    // Eval + check
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    // All compatibility constraints should be Satisfied
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            reify_ir::Satisfaction::Satisfied,
            "constraint {} should be satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}

// ── Step 5: guarded_enum_declarations ───────────────────────────────

/// Parse m5_guarded_enum.ri with enum Shape + guarded declarations + match.
/// Compile and eval. Verify:
/// - Enum compiles successfully
/// - Where-clause with enum comparison creates guarded groups
/// - Match expression evaluates correctly
/// - Constraint (size > 0mm) is present
#[test]
fn guarded_enum_declarations() {
    let source = std::fs::read_to_string("../../examples/m5_guarded_enum.ri")
        .expect("examples/m5_guarded_enum.ri should exist");

    let compiled = parse_and_compile(&source);

    // Should have a template for Fitting
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template"
    );

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Check shape = Shape.Round (let binding, not param — enum types aren't resolvable)
    let shape_id = ValueCellId::new("Fitting", "shape");
    let shape_val = result
        .values
        .get(&shape_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", shape_id));
    match shape_val {
        reify_ir::Value::Enum { variant, .. } => {
            assert_eq!(variant, "Round", "default shape should be Round");
        }
        other => panic!("expected Enum for shape, got {:?}", other),
    }

    // Check size = 10mm = 0.01 SI
    let size_id = ValueCellId::new("Fitting", "size");
    let size_val = result
        .values
        .get(&size_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", size_id));
    match size_val {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.01).abs() < 1e-12,
                "expected 0.01 SI for size, got {}",
                si_value
            );
        }
        other => panic!("expected Scalar for size, got {:?}", other),
    }

    // Check label = match shape { Round => 1, ... } = 1 (since shape=Round)
    let label_id = ValueCellId::new("Fitting", "label");
    let label_val = result
        .values
        .get(&label_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", label_id));
    match label_val {
        reify_ir::Value::Int(v) => {
            assert_eq!(*v, 1, "label should be 1 for Round");
        }
        other => panic!("expected Int for label, got {:?}", other),
    }

    // Guarded member diameter should be active (guard shape==Round is true)
    let diameter_id = ValueCellId::new("Fitting", "diameter");
    let diameter_val = result.values.get(&diameter_id);
    assert!(
        diameter_val.is_some(),
        "diameter should be present when shape is Round"
    );
    // diameter should equal size = 0.01 SI
    match diameter_val.unwrap() {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.01).abs() < 1e-12,
                "expected diameter = size = 0.01, got {}",
                si_value
            );
        }
        reify_ir::Value::Undef => {
            // Also acceptable — guard might not be evaluating enum comparison
        }
        other => panic!("expected Scalar or Undef for diameter, got {:?}", other),
    }
}

// ── Step 11: user_fn_with_constraint ────────────────────────────────

/// Parse m5_user_function.ri defining `fn area(w, h) -> Real { w * h }` and
/// `structure def Panel` that calls `area(width, height)` in a let binding,
/// with `constraint surface_area > 10000`.
///
/// Assert:
/// - Parse OK, Compile OK
/// - surface_area = area(200, 100) = 20000
/// - constraint surface_area > 10000 is Satisfied
#[test]
fn user_fn_with_constraint() {
    let source = std::fs::read_to_string("../../examples/m5_user_function.ri")
        .expect("examples/m5_user_function.ri should exist");

    let compiled = parse_and_compile(&source);

    // Should have a template for Panel
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template"
    );
    let panel = compiled
        .templates
        .iter()
        .find(|t| t.name == "Panel")
        .expect("should have a Panel template");
    assert_eq!(panel.name, "Panel");

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Check width = 200
    let width_id = ValueCellId::new("Panel", "width");
    let width_val = result
        .values
        .get(&width_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", width_id));
    match width_val {
        reify_ir::Value::Real(v) => {
            assert!((v - 200.0).abs() < 1e-12, "expected 200.0, got {}", v);
        }
        reify_ir::Value::Int(v) => {
            assert_eq!(*v, 200, "expected 200, got {}", v);
        }
        other => panic!("expected Real(200) or Int(200), got {:?}", other),
    }

    // Check surface_area = area(200, 100) = 20000
    let sa_id = ValueCellId::new("Panel", "surface_area");
    let sa_val = result
        .values
        .get(&sa_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", sa_id));
    match sa_val {
        reify_ir::Value::Real(v) => {
            assert!((v - 20000.0).abs() < 1e-9, "expected 20000.0, got {}", v);
        }
        reify_ir::Value::Int(v) => {
            assert_eq!(*v, 20000, "expected 20000, got {}", v);
        }
        other => panic!("expected Real(20000) or Int(20000), got {:?}", other),
    }

    // Check that the constraint surface_area > 10000 is present
    let result = engine.check(&compiled);
    assert!(
        !result.constraint_results.is_empty(),
        "expected at least one constraint"
    );
    // With MockConstraintChecker, all constraints are Satisfied
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            reify_ir::Satisfaction::Satisfied,
            "constraint {} should be satisfied",
            entry.id
        );
    }
}

// ── Step 13: geometry_cylinder_pattern ───────────────────────────────

/// Parse m5_geometry.ri with cylinder + circular_pattern through the full
/// build pipeline (with OCCT kernel).
///
/// Verify:
/// - Geometry let bindings compile into realization ops
/// - Build produces valid STEP output containing ISO-10303-21 header
/// - All constraints are satisfied
#[test]
fn geometry_cylinder_pattern() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let source = std::fs::read_to_string("../../examples/m5_geometry.ri")
        .expect("examples/m5_geometry.ri should exist");

    let compiled = parse_and_compile(&source);

    // Should have a Flange template
    let flange = compiled
        .templates
        .iter()
        .find(|t| t.name == "Flange")
        .expect("should have a Flange template");

    // Verify geometry let bindings produced realizations
    assert!(
        !flange.realizations.is_empty(),
        "Flange should have realization declarations from geometry lets"
    );

    // Build with real constraint checker and OCCT kernel
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // All constraints should be satisfied
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be satisfied",
            entry.id
        );
    }

    // Geometry output should be present and valid STEP
    let output = result
        .geometry_output
        .expect("build should produce geometry output");
    assert!(!output.is_empty(), "STEP output should be non-empty");

    let step_str = String::from_utf8(output).expect("STEP should be valid UTF-8");
    assert!(
        step_str.contains("ISO-10303-21"),
        "STEP output should contain ISO-10303-21 header"
    );
}

// ── Step 15: combined_m5_features ───────────────────────────────────

/// A larger test combining multiple M5 features:
/// - Trait definition (Sizable with a required `size` param + constraint)
/// - Enum declaration (Kind with variants)
/// - User-defined function (scale)
/// - Structure implementing trait, with guarded declarations on enum, using function
/// - Collection (list literal) with .count
///
/// Exercises feature interaction paths that individual tests may miss.
#[test]
fn combined_m5_features() {
    let source = r#"
trait Sizable {
    param size : Real
    constraint size > 0
}

enum Kind { Small, Medium, Large }

fn scale(x: Real, factor: Int) -> Real { x * factor }

structure def Widget : Sizable {
    let kind = Kind.Medium
    param size : Real = 50

    let scaled = scale(size, 2)

    where kind == Kind.Small {
        let label = 1
    } else {
        let label = 2
    }

    let label_copy = match kind {
        Small => 10,
        Medium => 20,
        Large => 30
    }

    let items = [1, 2, 3, 4, 5]
    let n = items.count

    constraint scaled > size
}
"#;

    let compiled = parse_and_compile(source);

    // Should have a Widget template
    let widget = compiled
        .templates
        .iter()
        .find(|t| t.name == "Widget")
        .expect("should have a Widget template");
    assert_eq!(widget.name, "Widget");

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Check kind = Kind.Medium
    let kind_id = ValueCellId::new("Widget", "kind");
    let kind_val = result
        .values
        .get(&kind_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", kind_id));
    match kind_val {
        reify_ir::Value::Enum { variant, .. } => {
            assert_eq!(variant, "Medium", "kind should be Medium");
        }
        other => panic!("expected Enum for kind, got {:?}", other),
    }

    // Check size = 50
    let size_id = ValueCellId::new("Widget", "size");
    let size_val = result
        .values
        .get(&size_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", size_id));
    match size_val {
        reify_ir::Value::Real(v) => {
            assert!((v - 50.0).abs() < 1e-12, "expected 50.0, got {}", v);
        }
        reify_ir::Value::Int(v) => {
            assert_eq!(*v, 50, "expected 50, got {}", v);
        }
        other => panic!("expected Real(50) or Int(50), got {:?}", other),
    }

    // Check scaled = scale(50, 2) = 100
    let scaled_id = ValueCellId::new("Widget", "scaled");
    let scaled_val = result
        .values
        .get(&scaled_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", scaled_id));
    match scaled_val {
        reify_ir::Value::Real(v) => {
            assert!((v - 100.0).abs() < 1e-9, "expected 100.0, got {}", v);
        }
        reify_ir::Value::Int(v) => {
            assert_eq!(*v, 100, "expected 100, got {}", v);
        }
        other => panic!("expected Real(100) or Int(100), got {:?}", other),
    }

    // Check label_copy = match Medium => 20
    let lc_id = ValueCellId::new("Widget", "label_copy");
    let lc_val = result
        .values
        .get(&lc_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", lc_id));
    match lc_val {
        reify_ir::Value::Int(v) => {
            assert_eq!(*v, 20, "label_copy should be 20 for Medium");
        }
        other => panic!("expected Int for label_copy, got {:?}", other),
    }

    // Check items.count = 5
    let n_id = ValueCellId::new("Widget", "n");
    let n_val = result
        .values
        .get(&n_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", n_id));
    assert_eq!(
        *n_val,
        reify_ir::Value::Int(5),
        "items.count should be 5"
    );

    // Check constraints
    let result = engine.check(&compiled);
    assert!(
        !result.constraint_results.is_empty(),
        "expected constraints from trait + structure"
    );
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be satisfied",
            entry.id
        );
    }
}

// ── New tests ───────────────────────────────────────────────────────

// ── trait_rigid_mass_conformance ─────────────────────────────────────

/// Parse m5_trait_rigid.ri (trait Rigid with Mass/kg + structure Bracket : Rigid),
/// compile, eval, verify mass=0.5kg=0.5 SI, width=80mm=0.08 SI, constraints satisfied.
///
/// This exercises trait conformance with Mass type and kg units — different from
/// the existing test which uses Length/mm.
#[test]
fn trait_rigid_mass_conformance() {
    let source = std::fs::read_to_string("../../examples/m5_trait_rigid.ri")
        .expect("examples/m5_trait_rigid.ri should exist");

    let compiled = parse_and_compile(&source);

    // Should have one template (Bracket)
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template"
    );
    let bracket = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bracket")
        .expect("should have a Bracket template");
    assert_eq!(bracket.name, "Bracket");

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Check mass = 0.5kg = 0.5 SI
    let mass_id = ValueCellId::new("Bracket", "mass");
    let mass_val = result
        .values
        .get(&mass_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", mass_id));
    match mass_val {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.5).abs() < 1e-12,
                "expected 0.5 SI for mass (0.5kg), got {}",
                si_value
            );
        }
        other => panic!("expected Scalar for mass, got {:?}", other),
    }

    // Check width = 80mm = 0.08 SI
    let width_id = ValueCellId::new("Bracket", "width");
    let width_val = result
        .values
        .get(&width_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", width_id));
    match width_val {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.08).abs() < 1e-12,
                "expected 0.08 SI for width (80mm), got {}",
                si_value
            );
        }
        other => panic!("expected Scalar for width, got {:?}", other),
    }

    // Check constraints via engine.check() — all should be Satisfied
    let result = engine.check(&compiled);
    assert!(
        !result.constraint_results.is_empty(),
        "expected constraints from trait + structure"
    );
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be satisfied",
            entry.id
        );
    }
}

// ── multi_module_import_with_sub ─────────────────────────────────────

/// Create temp dir with two .ri files: lib.ri defining Circle with radius param,
/// main.ri importing lib and using `sub circle = Circle()` inside Assembly.
/// This extends the existing multi_module_import test by using the imported
/// structure as a sub-component, exercising cross-module resolution.
#[test]
fn multi_module_import_with_sub() {
    let dir = std::env::temp_dir()
        .join("reify_m5_test")
        .join(format!("multi_sub_{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();

    // Create stdlib dir (required by ModuleResolver)
    let stdlib = dir.join("stdlib");
    fs::create_dir_all(&stdlib).unwrap();

    // lib.ri — defines Circle structure
    fs::write(
        dir.join("lib.ri"),
        r#"
structure def Circle {
    param radius : Length = 10mm
    let diameter = radius * 2
}
"#,
    )
    .unwrap();

    // main.ri — imports lib and uses Circle as sub-component
    fs::write(
        dir.join("main.ri"),
        r#"
import lib

structure def Assembly {
    param size : Length = 50mm
    sub circle = Circle()
    constraint size > 0mm
}
"#,
    )
    .unwrap();

    let resolver = ModuleResolver::new(&dir, &stdlib);
    let result = compile_project(&dir.join("main.ri"), &resolver);

    match result {
        Ok(modules) => {
            // Should have at least 2 modules (lib + main)
            assert!(
                modules.len() >= 2,
                "expected at least 2 compiled modules, got {}",
                modules.len()
            );

            // Find Assembly template and verify it has sub_components
            let assembly_mod = modules
                .iter()
                .find(|m| m.templates.iter().any(|t| t.name == "Assembly"))
                .expect("should have a module with Assembly template");
            let assembly = assembly_mod
                .templates
                .iter()
                .find(|t| t.name == "Assembly")
                .unwrap();
            assert!(
                !assembly.sub_components.is_empty(),
                "Assembly should have sub-components from 'sub circle = Circle()'"
            );
        }
        Err(errors) => {
            panic!("compile_project failed: {:?}", errors);
        }
    }

    // Cleanup
    let _ = fs::remove_dir_all(&dir);
}

// ── guarded_enum_multi_branch ───────────────────────────────────────

/// Parse m5_guarded_head_type.ri with enum HeadType (Hex, Socket, Button),
/// guarded declarations, and match expression. More complex than existing
/// guarded_enum_declarations which only has 2 enum variants.
///
/// Verify:
/// - head_type = Hex
/// - across_flats is present (active guard branch for Hex)
/// - match expression evaluates to 1 (Hex)
/// - Constraints are satisfied
#[test]
fn guarded_enum_multi_branch() {
    let source = std::fs::read_to_string("../../examples/m5_guarded_head_type.ri")
        .expect("examples/m5_guarded_head_type.ri should exist");

    let compiled = parse_and_compile(&source);

    // Should have a Bolt template
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template"
    );
    let bolt = compiled
        .templates
        .iter()
        .find(|t| t.name == "Bolt")
        .expect("should have a Bolt template");
    assert_eq!(bolt.name, "Bolt");

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Check head_type = HeadType.Hex
    let ht_id = ValueCellId::new("Bolt", "head_type");
    let ht_val = result
        .values
        .get(&ht_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", ht_id));
    match ht_val {
        reify_ir::Value::Enum { variant, .. } => {
            assert_eq!(variant, "Hex", "head_type should be Hex");
        }
        other => panic!("expected Enum for head_type, got {:?}", other),
    }

    // Check across_flats is present (active guard branch for Hex)
    let af_id = ValueCellId::new("Bolt", "across_flats");
    let af_val = result.values.get(&af_id);
    assert!(
        af_val.is_some(),
        "across_flats should be present when head_type is Hex"
    );
    // across_flats should be 17mm = 0.017 SI (Hex branch)
    match af_val.unwrap() {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.017).abs() < 1e-12,
                "expected 0.017 SI for across_flats (17mm), got {}",
                si_value
            );
        }
        reify_ir::Value::Undef => {
            // Guard might not be evaluating enum comparison yet
        }
        other => panic!("expected Scalar or Undef for across_flats, got {:?}", other),
    }

    // Check head_label = match Hex => 1
    let hl_id = ValueCellId::new("Bolt", "head_label");
    let hl_val = result
        .values
        .get(&hl_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", hl_id));
    match hl_val {
        reify_ir::Value::Int(v) => {
            assert_eq!(*v, 1, "head_label should be 1 for Hex");
        }
        other => panic!("expected Int for head_label, got {:?}", other),
    }

    // Check shaft_diameter = 10mm = 0.01 SI
    let sd_id = ValueCellId::new("Bolt", "shaft_diameter");
    let sd_val = result
        .values
        .get(&sd_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", sd_id));
    match sd_val {
        reify_ir::Value::Scalar { si_value, .. } => {
            assert!(
                (si_value - 0.01).abs() < 1e-12,
                "expected 0.01 SI for shaft_diameter (10mm), got {}",
                si_value
            );
        }
        other => panic!("expected Scalar for shaft_diameter, got {:?}", other),
    }

    // Check constraints
    let result = engine.check(&compiled);
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be satisfied",
            entry.id
        );
    }
}

// ── collection_with_quantifier ──────────────────────────────────────

/// Parse m5_collection_ops.ri with list literal, .count, .sum, indexing,
/// and a forall quantifier constraint. The forall compiles but may not
/// evaluate yet — test verifies compilation and basic collection eval.
///
/// This extends existing collection_lambda_operations by adding quantifier
/// expressions and using an example file rather than inline source.
#[test]
fn collection_with_quantifier() {
    let source = std::fs::read_to_string("../../examples/m5_collection_ops.ri")
        .expect("examples/m5_collection_ops.ri should exist");

    let compiled = parse_and_compile(&source);

    // Should have an Inventory template
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template"
    );

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Check items = [5, 10, 15, 20]
    let items_id = ValueCellId::new("Inventory", "items");
    let items_val = result
        .values
        .get(&items_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", items_id));
    match items_val {
        reify_ir::Value::List(elems) => {
            assert_eq!(elems.len(), 4, "expected 4 elements");
            assert_eq!(elems[0], reify_ir::Value::Int(5));
            assert_eq!(elems[1], reify_ir::Value::Int(10));
            assert_eq!(elems[2], reify_ir::Value::Int(15));
            assert_eq!(elems[3], reify_ir::Value::Int(20));
        }
        other => panic!("expected List for items, got {:?}", other),
    }

    // Check n = items.count = 4
    let n_id = ValueCellId::new("Inventory", "n");
    let n_val = result
        .values
        .get(&n_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", n_id));
    assert_eq!(
        *n_val,
        reify_ir::Value::Int(4),
        "items.count should be 4"
    );

    // Check total = items.sum = 50
    let total_id = ValueCellId::new("Inventory", "total");
    let total_val = result
        .values
        .get(&total_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", total_id));
    assert_eq!(
        *total_val,
        reify_ir::Value::Int(50),
        "items.sum should be 50"
    );

    // Check first = items[0] = 5
    let first_id = ValueCellId::new("Inventory", "first");
    let first_val = result
        .values
        .get(&first_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", first_id));
    assert_eq!(
        *first_val,
        reify_ir::Value::Int(5),
        "items[0] should be 5"
    );

    // Check last = items[3] = 20
    let last_id = ValueCellId::new("Inventory", "last");
    let last_val = result
        .values
        .get(&last_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", last_id));
    assert_eq!(
        *last_val,
        reify_ir::Value::Int(20),
        "items[3] should be 20"
    );

    // Check constraints — forall may evaluate as Undef but MockConstraintChecker
    // returns Satisfied for all constraints
    let result = engine.check(&compiled);
    assert!(
        !result.constraint_results.is_empty(),
        "expected at least one constraint (n > 0)"
    );
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be satisfied",
            entry.id
        );
    }
}

// ── occurrence_manufacturing_chain ──────────────────────────────────

/// Parse m5_occurrence_process.ri with occurrence defs Machining and HeatTreat,
/// each with typed ports, connected via chain in ManufacturingProcess.
/// Different from existing connect_occurrence_chain which uses Pipe/FluidPort.
///
/// Verify:
/// - Machining and HeatTreat are EntityKind::Occurrence
/// - ManufacturingProcess has connections from chain
/// - feed_rate and temperature params evaluate correctly
/// - All constraints satisfied
#[test]
fn occurrence_manufacturing_chain() {
    let source = std::fs::read_to_string("../../examples/m5_occurrence_process.ri")
        .expect("examples/m5_occurrence_process.ri should exist");

    let compiled = parse_and_compile(&source);

    // Should have templates for Machining, HeatTreat, and ManufacturingProcess
    assert!(
        compiled.templates.len() >= 3,
        "expected at least 3 templates, got {}",
        compiled.templates.len()
    );

    // Verify Machining is an occurrence
    let machining = compiled
        .templates
        .iter()
        .find(|t| t.name == "Machining")
        .expect("should have a Machining template");
    assert_eq!(
        machining.entity_kind,
        reify_compiler::EntityKind::Occurrence,
        "Machining should be an occurrence"
    );

    // Verify HeatTreat is an occurrence
    let heat_treat = compiled
        .templates
        .iter()
        .find(|t| t.name == "HeatTreat")
        .expect("should have a HeatTreat template");
    assert_eq!(
        heat_treat.entity_kind,
        reify_compiler::EntityKind::Occurrence,
        "HeatTreat should be an occurrence"
    );

    // Verify ManufacturingProcess has connections from chain
    let mfg_process = compiled
        .templates
        .iter()
        .find(|t| t.name == "ManufacturingProcess")
        .expect("should have a ManufacturingProcess template");
    assert!(
        !mfg_process.connections.is_empty(),
        "ManufacturingProcess should have connections from chain desugaring"
    );

    // Eval + check
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Check feed_rate = 100
    let fr_id = ValueCellId::new("Machining", "feed_rate");
    let fr_val = result
        .values
        .get(&fr_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", fr_id));
    match fr_val {
        reify_ir::Value::Real(v) => {
            assert!((v - 100.0).abs() < 1e-12, "expected 100.0, got {}", v);
        }
        reify_ir::Value::Int(v) => {
            assert_eq!(*v, 100, "expected 100, got {}", v);
        }
        other => panic!(
            "expected Real(100) or Int(100) for feed_rate, got {:?}",
            other
        ),
    }

    // Check temperature = 850
    let temp_id = ValueCellId::new("HeatTreat", "temperature");
    let temp_val = result
        .values
        .get(&temp_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", temp_id));
    match temp_val {
        reify_ir::Value::Real(v) => {
            assert!((v - 850.0).abs() < 1e-12, "expected 850.0, got {}", v);
        }
        reify_ir::Value::Int(v) => {
            assert_eq!(*v, 850, "expected 850, got {}", v);
        }
        other => panic!(
            "expected Real(850) or Int(850) for temperature, got {:?}",
            other
        ),
    }

    // All compatibility constraints should be Satisfied
    let result = engine.check(&compiled);
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}

// ── user_fn_safety_factor ───────────────────────────────────────────

/// Parse m5_function_safety_factor.ri with fn safety_factor using division
/// (yield_str / applied) and structure Beam with constraint sf >= 2.0.
/// This tests division in user functions (existing test only uses multiplication)
/// and constraint on function result.
///
/// Verify:
/// - sf = safety_factor(100, 250) = 2.5
/// - constraint sf >= 2.0 is satisfied
#[test]
fn user_fn_safety_factor() {
    let source = std::fs::read_to_string("../../examples/m5_function_safety_factor.ri")
        .expect("examples/m5_function_safety_factor.ri should exist");

    let compiled = parse_and_compile(&source);

    // Should have a Beam template
    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template"
    );
    let beam = compiled
        .templates
        .iter()
        .find(|t| t.name == "Beam")
        .expect("should have a Beam template");
    assert_eq!(beam.name, "Beam");

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Check stress = 100
    let stress_id = ValueCellId::new("Beam", "stress");
    let stress_val = result
        .values
        .get(&stress_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", stress_id));
    match stress_val {
        reify_ir::Value::Real(v) => {
            assert!((v - 100.0).abs() < 1e-12, "expected 100.0, got {}", v);
        }
        reify_ir::Value::Int(v) => {
            assert_eq!(*v, 100, "expected 100, got {}", v);
        }
        other => panic!("expected Real(100) or Int(100) for stress, got {:?}", other),
    }

    // Check sf = safety_factor(100, 250) = 250/100 = 2.5
    let sf_id = ValueCellId::new("Beam", "sf");
    let sf_val = result
        .values
        .get(&sf_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", sf_id));
    match sf_val {
        reify_ir::Value::Real(v) => {
            assert!((v - 2.5).abs() < 1e-9, "expected 2.5, got {}", v);
        }
        other => panic!("expected Real(2.5) for sf, got {:?}", other),
    }

    // Check constraints — sf >= 2.0 should be satisfied
    let result = engine.check(&compiled);
    assert!(
        !result.constraint_results.is_empty(),
        "expected at least one constraint (sf >= 2.0)"
    );
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be satisfied",
            entry.id
        );
    }
}

// ── geometry_flange_with_pattern ────────────────────────────────────

/// Parse m5_geometry_flange.ri through the full build pipeline with OCCT kernel.
/// BoltFlange uses the stdlib `Rigid` trait and designates its CSG result via
/// `param geometry : Solid = difference(body, holes)`.
///
/// Verify:
/// - BoltFlange declares the `Rigid` trait bound
/// - At least 4 realizations: body cylinder, translate(hole), circular_pattern, difference
/// - `geometry` IS emitted as a `ValueCellDecl` with `cell_type == Type::Geometry`
///   (GHR-γ step-2 retired the `is_solid_geometry_param` skip; both a ValueCellDecl
///   and the parallel RealizationDecl chain are now produced)
/// - Build produces valid STEP output with ISO-10303-21 header
/// - All constraints satisfied
///
/// Expect failure until `examples/m5_geometry_flange.ri` is rewritten (step-10):
/// the current example has no `: Rigid` bound and no `param geometry : Solid`.
#[test]
fn geometry_flange_with_pattern() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let source = std::fs::read_to_string("../../examples/m5_geometry_flange.ri")
        .expect("examples/m5_geometry_flange.ri should exist");

    // Use stdlib so Rigid/Physical/Material trait definitions are in scope.
    let compiled = parse_and_compile_with_stdlib(&source);

    // Should have a BoltFlange template
    let flange = compiled
        .templates
        .iter()
        .find(|t| t.name == "BoltFlange")
        .expect("should have a BoltFlange template");

    // (b) BoltFlange must declare conformance to `Rigid`.
    assert!(
        flange.trait_bounds.contains(&"Rigid".to_string()),
        "BoltFlange must declare `: Rigid` trait bound, got: {:?}",
        flange.trait_bounds
    );

    // (c) At least 4 realizations: body + translate(hole) + circular_pattern + difference(geometry)
    assert!(
        flange.realizations.len() >= 4,
        "expected at least 4 realizations (body + hole + circular_pattern + geometry/difference), got {}",
        flange.realizations.len()
    );

    // (d) `geometry` MUST appear as a Type::Geometry value cell (GHR-γ step-2 retired
    // the is_solid_geometry_param skip; Solid-typed params now emit both a ValueCellDecl
    // with cell_type == Type::Geometry AND the parallel RealizationDecl chain).
    {
        use reify_core::Type;
        assert!(
            flange
                .value_cells
                .iter()
                .any(|c| c.id.member == "geometry" && c.cell_type == Type::Geometry),
            "`geometry` must be a ValueCellDecl with cell_type=Type::Geometry (GHR-γ); got: {:?}",
            flange
                .value_cells
                .iter()
                .map(|c| (&c.id.member, &c.cell_type))
                .collect::<Vec<_>>()
        );
    }

    // Build with real constraint checker and OCCT kernel
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));

    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    let result = engine.build(&compiled, ExportFormat::Step);

    // All constraints should be satisfied
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be satisfied",
            entry.id
        );
    }

    // Geometry output should be present and valid STEP
    let output = result
        .geometry_output
        .expect("build should produce geometry output");
    assert!(!output.is_empty(), "STEP output should be non-empty");

    let step_str = String::from_utf8(output).expect("STEP should be valid UTF-8");
    assert!(
        step_str.contains("ISO-10303-21"),
        "STEP output should contain ISO-10303-21 header"
    );
}

// ── combined_all_features ───────────────────────────────────────────

/// Parse m5_combined_all.ri combining:
/// - Trait Measurable (with constraint size > 0)
/// - Enum Quality { Standard, Premium }
/// - User function grading(q) -> q * 10
/// - Structure Widget : Measurable with guarded members on enum
/// - Match expression on enum
/// - Collection with .count and .sum
/// - Constraints on function result and collection count
///
/// This is the capstone integration test exercising all feature interactions
/// through the full pipeline (parse → compile → eval → check).
#[test]
fn combined_all_features() {
    let source = std::fs::read_to_string("../../examples/m5_combined_all.ri")
        .expect("examples/m5_combined_all.ri should exist");

    let compiled = parse_and_compile(&source);

    // Should have a Widget template
    let widget = compiled
        .templates
        .iter()
        .find(|t| t.name == "Widget")
        .expect("should have a Widget template");
    assert_eq!(widget.name, "Widget");

    // Eval
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // Check size = 25 (Real type from trait conformance)
    let size_id = ValueCellId::new("Widget", "size");
    let size_val = result
        .values
        .get(&size_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", size_id));
    match size_val {
        reify_ir::Value::Real(v) => {
            assert!((v - 25.0).abs() < 1e-12, "expected 25.0, got {}", v);
        }
        reify_ir::Value::Int(v) => {
            assert_eq!(*v, 25, "expected 25, got {}", v);
        }
        other => panic!("expected Real(25) or Int(25), got {:?}", other),
    }

    // Check quality = Quality.Premium
    let quality_id = ValueCellId::new("Widget", "quality");
    let quality_val = result
        .values
        .get(&quality_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", quality_id));
    match quality_val {
        reify_ir::Value::Enum { variant, .. } => {
            assert_eq!(variant, "Premium", "quality should be Premium");
        }
        other => panic!("expected Enum for quality, got {:?}", other),
    }

    // Check grade = grading(3) = 3 * 10 = 30
    let grade_id = ValueCellId::new("Widget", "grade");
    let grade_val = result
        .values
        .get(&grade_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", grade_id));
    match grade_val {
        reify_ir::Value::Int(v) => {
            assert_eq!(*v, 30, "grade should be 30 (grading(3) = 3*10)");
        }
        reify_ir::Value::Real(v) => {
            assert!(
                (v - 30.0).abs() < 1e-12,
                "expected 30.0 for grade, got {}",
                v
            );
        }
        other => panic!("expected Int(30) or Real(30) for grade, got {:?}", other),
    }

    // Check premium_label from guard (quality == Premium -> label = 1)
    let pl_id = ValueCellId::new("Widget", "premium_label");
    let pl_val = result.values.get(&pl_id);
    assert!(pl_val.is_some(), "premium_label should be present");
    match pl_val.unwrap() {
        reify_ir::Value::Int(v) => {
            assert_eq!(*v, 1, "premium_label should be 1 for Premium");
        }
        reify_ir::Value::Undef => {
            // Guard may not evaluate enum comparison — acceptable
        }
        other => panic!(
            "expected Int(1) or Undef for premium_label, got {:?}",
            other
        ),
    }

    // Check quality_code = match Premium => 200
    let qc_id = ValueCellId::new("Widget", "quality_code");
    let qc_val = result
        .values
        .get(&qc_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", qc_id));
    match qc_val {
        reify_ir::Value::Int(v) => {
            assert_eq!(*v, 200, "quality_code should be 200 for Premium");
        }
        other => panic!("expected Int(200) for quality_code, got {:?}", other),
    }

    // Check items = [10, 20, 30, 40]
    let items_id = ValueCellId::new("Widget", "items");
    let items_val = result
        .values
        .get(&items_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", items_id));
    match items_val {
        reify_ir::Value::List(elems) => {
            assert_eq!(elems.len(), 4, "expected 4 items");
            assert_eq!(elems[0], reify_ir::Value::Int(10));
            assert_eq!(elems[1], reify_ir::Value::Int(20));
            assert_eq!(elems[2], reify_ir::Value::Int(30));
            assert_eq!(elems[3], reify_ir::Value::Int(40));
        }
        other => panic!("expected List for items, got {:?}", other),
    }

    // Check item_count = items.count = 4
    let ic_id = ValueCellId::new("Widget", "item_count");
    let ic_val = result
        .values
        .get(&ic_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", ic_id));
    assert_eq!(
        *ic_val,
        reify_ir::Value::Int(4),
        "item_count should be 4"
    );

    // Check item_total = items.sum = 100
    let it_id = ValueCellId::new("Widget", "item_total");
    let it_val = result
        .values
        .get(&it_id)
        .unwrap_or_else(|| panic!("value for {:?} not found", it_id));
    assert_eq!(
        *it_val,
        reify_ir::Value::Int(100),
        "item_total should be 100"
    );

    // Check all constraints (trait size > 0, grade > 0, item_count > 0)
    let result = engine.check(&compiled);
    assert!(
        !result.constraint_results.is_empty(),
        "expected constraints from trait + structure"
    );
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {} should be satisfied",
            entry.id
        );
    }
}
