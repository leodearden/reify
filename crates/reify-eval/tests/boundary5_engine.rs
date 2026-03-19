//! Boundary 5 (cli → eval) — Engine facade tests.
//!
//! These tests verify the Engine API works correctly with mock implementations.

use reify_test_support::*;
use reify_types::Satisfaction;

/// Full pipeline with mocks: compile → evaluate → expected ValueMap.
#[test]

fn full_pipeline_with_mocks() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.eval(&module);
    assert!(!result.values.is_empty());
}

/// Build with mock geometry kernel → produces output.
#[test]

fn build_with_mock_kernel() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let kernel = MockGeometryKernel::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&module, reify_types::ExportFormat::Step);
    assert!(result.geometry_output.is_some());
}

/// Auto param evaluates to (Undef, DeterminacyState::Auto) in snapshot.
#[test]
fn eval_auto_param_undef_auto() {
    use reify_types::{CompiledExpr, DeterminacyState, ModulePath, Type, ValueCellId};

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        .param("S", "y", Type::length(), Some(CompiledExpr::literal(mm(5.0), Type::length())))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // x should be Undef in the values map
    let x_id = ValueCellId::new("S", "x");
    let x_val = result.values.get(&x_id).expect("x should be in values");
    assert!(x_val.is_undef(), "auto param x should be Undef, got {:?}", x_val);

    // y should be evaluated to 0.005 (5mm in SI)
    let y_id = ValueCellId::new("S", "y");
    let y_val = result.values.get(&y_id).expect("y should be in values");
    assert!(!y_val.is_undef(), "normal param y should not be Undef");

    // Check snapshot determinacy
    let snapshot = engine.snapshot().expect("snapshot should exist after eval");
    let (x_snap_val, x_det) = snapshot.values.get(&x_id).expect("x in snapshot");
    assert!(x_snap_val.is_undef(), "snapshot x should be Undef");
    assert_eq!(*x_det, DeterminacyState::Auto, "snapshot x determinacy should be Auto");

    let (_, y_det) = snapshot.values.get(&y_id).expect("y in snapshot");
    assert_eq!(*y_det, DeterminacyState::Determined, "snapshot y determinacy should be Determined");
}

/// eval_cached: auto param gets (Undef, Auto), and override applies Determined.
#[test]
fn eval_cached_auto_param() {
    use reify_types::{CompiledExpr, ModulePath, Type, ValueCellId, VersionId};

    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        .param("S", "y", Type::length(), Some(CompiledExpr::literal(mm(5.0), Type::length())))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);

    // First eval_cached: auto param should be (Undef, Auto)
    let result = engine.eval_cached(&module, VersionId(1));
    let x_id = ValueCellId::new("S", "x");
    let x_val = result.eval_result.values.get(&x_id).expect("x should be in values");
    assert!(x_val.is_undef(), "auto param x should be Undef on cold start");

    // Now set an override for the auto param and re-evaluate
    engine.set_param_and_invalidate(&x_id, mm(10.0));
    let result2 = engine.eval_cached(&module, VersionId(2));
    let x_val2 = result2.eval_result.values.get(&x_id).expect("x should be in values");
    assert!(!x_val2.is_undef(), "auto param x should have override value");
}

/// Constraint on auto param → Indeterminate (Undef propagates).
#[test]
fn constraint_on_auto_param_indeterminate() {
    use reify_types::{ModulePath, Type, ValueCellId};

    // Build module with auto param x and constraint x > 5mm
    let template = TopologyTemplateBuilder::new("S")
        .auto_param("S", "x", Type::length())
        .constraint("S", 0, None, gt(value_ref("S", "x"), literal(mm(5.0))))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(template)
        .build();

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&module);

    // x should be Undef
    let x_id = ValueCellId::new("S", "x");
    let x_val = result.values.get(&x_id).expect("x should be in values");
    assert!(x_val.is_undef(), "auto param x should be Undef");

    // Constraint should be Indeterminate (Undef propagation)
    assert_eq!(result.constraint_results.len(), 1);
    assert_eq!(
        result.constraint_results[0].satisfaction,
        reify_types::Satisfaction::Indeterminate,
        "constraint on auto param should be Indeterminate"
    );
}

/// End-to-end: parse → compile → eval → check with auto param.
#[test]
fn e2e_parse_compile_eval_auto_param() {
    use reify_compiler::ValueCellKind;
    use reify_types::{DeterminacyState, ModulePath, Satisfaction, ValueCellId};

    let source = r#"structure S {
    param x : Scalar = auto
    param y : Scalar = 5mm
    let z = y * 2
    constraint x > 2mm
}"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("e2e_auto"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);
    // Check that x is ValueCellKind::Auto
    let template = &compiled.templates[0];
    let x_cell = template
        .value_cells
        .iter()
        .find(|c| c.id == ValueCellId::new("S", "x"))
        .expect("x cell not found");
    assert_eq!(x_cell.kind, ValueCellKind::Auto);

    // Evaluate and check
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    // x should be Undef
    let x_id = ValueCellId::new("S", "x");
    let x_val = result.values.get(&x_id).expect("x should be in values");
    assert!(x_val.is_undef(), "auto param x should be Undef, got {:?}", x_val);

    // y should be ~0.005 SI (5mm)
    let y_id = ValueCellId::new("S", "y");
    let y_val = result.values.get(&y_id).expect("y should be in values");
    let y_f64 = y_val.as_f64().expect("y should be a number");
    assert!((y_f64 - 0.005).abs() < 1e-10, "y should be 0.005 SI, got {}", y_f64);

    // z = y * 2 ≈ 0.01 SI
    let z_id = ValueCellId::new("S", "z");
    let z_val = result.values.get(&z_id).expect("z should be in values");
    let z_f64 = z_val.as_f64().expect("z should be a number");
    assert!((z_f64 - 0.01).abs() < 1e-10, "z should be 0.01 SI, got {}", z_f64);

    // Constraint on x should be Indeterminate
    assert_eq!(result.constraint_results.len(), 1);
    assert_eq!(
        result.constraint_results[0].satisfaction,
        Satisfaction::Indeterminate,
        "constraint on auto param x should be Indeterminate"
    );

    // Check snapshot determinacy
    let snapshot = engine.snapshot().expect("snapshot should exist");
    let (_, x_det) = snapshot.values.get(&x_id).expect("x in snapshot");
    assert_eq!(*x_det, DeterminacyState::Auto);

    let (_, y_det) = snapshot.values.get(&y_id).expect("y in snapshot");
    assert_eq!(*y_det, DeterminacyState::Determined);
}

/// Engine with predetermined constraint results → reports violations.
#[test]

fn engine_reports_violations() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new()
        .with_result(cnid("Bracket", 0), Satisfaction::Violated);
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&module);

    let violated: Vec<_> = result
        .constraint_results
        .iter()
        .filter(|c| c.satisfaction == Satisfaction::Violated)
        .collect();
    assert!(!violated.is_empty(), "should report violations");
}

/// Sub-component param values appear in the eval result with scoped IDs.
#[test]
fn sub_component_params_appear_in_eval_result() {
    use reify_types::ValueCellId;

    let module = parent_child_module();
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Parent.rib.height should be width * 0.5 = 80mm * 0.5 = 40mm = 0.04 SI
    let scoped_id = ValueCellId::new("Parent.rib", "height");
    let val = result.values.get(&scoped_id)
        .expect("Parent.rib.height should be in eval result values");
    let f = val.as_f64().expect("should be numeric");
    assert!(
        (f - 0.04).abs() < 1e-10,
        "Parent.rib.height should be ~0.04 SI (40mm), got {}",
        f
    );
}

/// Sub-component child let-bindings are evaluated and appear in the result.
#[test]
fn sub_component_child_lets_evaluated() {
    use reify_types::ValueCellId;

    let module = parent_child_module();
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Parent.rib.half_h should be height / 2 = 40mm / 2 = 20mm = 0.02 SI
    let scoped_id = ValueCellId::new("Parent.rib", "half_h");
    let val = result.values.get(&scoped_id)
        .expect("Parent.rib.half_h should be in eval result values");
    let f = val.as_f64().expect("should be numeric");
    assert!(
        (f - 0.02).abs() < 1e-10,
        "Parent.rib.half_h should be ~0.02 SI (20mm), got {}",
        f
    );
}

/// Sub-component with no args falls back to child's default param value.
#[test]
fn sub_component_default_param_when_no_arg() {
    use reify_types::{CompiledExpr, ModulePath, Type, ValueCellId};

    // Child: param height = 10mm
    let child_template = TopologyTemplateBuilder::new("Child")
        .param("Child", "height", Type::length(), Some(CompiledExpr::literal(mm(10.0), Type::length())))
        .build();

    // Parent: param width = 80mm, sub rib = Child() — no args
    let parent_template = TopologyTemplateBuilder::new("Parent")
        .param("Parent", "width", Type::length(), Some(CompiledExpr::literal(mm(80.0), Type::length())))
        .sub_component("rib", "Child", vec![])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(child_template)
        .template(parent_template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Parent.rib.height should use Child's default: 10mm = 0.01 SI
    let scoped_id = ValueCellId::new("Parent.rib", "height");
    let val = result.values.get(&scoped_id)
        .expect("Parent.rib.height should be in eval result values");
    let f = val.as_f64().expect("should be numeric");
    assert!(
        (f - 0.01).abs() < 1e-10,
        "Parent.rib.height should be ~0.01 SI (10mm default), got {}",
        f
    );
}

/// Sub-component referencing a missing structure is skipped gracefully.
#[test]
fn sub_component_missing_structure_skipped_gracefully() {
    use reify_types::{CompiledExpr, ModulePath, Type, ValueCellId};

    // Parent template with sub referencing nonexistent "NonExistent"
    let parent_template = TopologyTemplateBuilder::new("Parent")
        .param("Parent", "width", Type::length(), Some(CompiledExpr::literal(mm(80.0), Type::length())))
        .sub_component("thing", "NonExistent", vec![])
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .template(parent_template)
        .build();

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // Should not panic; Parent's own param should be evaluated correctly
    let width_id = ValueCellId::new("Parent", "width");
    let val = result.values.get(&width_id)
        .expect("Parent.width should be in values");
    let f = val.as_f64().expect("should be numeric");
    assert!(
        (f - 0.08).abs() < 1e-10,
        "Parent.width should be ~0.08 SI, got {}",
        f
    );

    // No scoped entries for "Parent.thing" should exist
    let has_scoped = result.values.iter().any(|(id, _)| {
        format!("{}", id).contains("Parent.thing")
    });
    assert!(!has_scoped, "no Parent.thing entries should exist when structure is missing");
}

/// Engine-level verification: stdlib functions evaluate correctly in let-bindings.
#[test]
fn engine_eval_stdlib_function_in_let() {
    use reify_types::{ModulePath, ValueCellId};

    let source = r#"structure S {
    param w: Scalar = 80mm
    param h: Scalar = 100mm
    let diag = sqrt(w * w + h * h)
}"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("stdlib_test"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // diag = sqrt(0.08^2 + 0.1^2) = sqrt(0.0064 + 0.01) = sqrt(0.0164) ≈ 0.128062
    let diag_id = ValueCellId::new("S", "diag");
    let val = result.values.get(&diag_id)
        .expect("S.diag should be in eval result");
    let f = val.as_f64().expect("should be numeric");
    let expected = (0.08_f64.powi(2) + 0.1_f64.powi(2)).sqrt();
    assert!(
        (f - expected).abs() < 1e-10,
        "S.diag should be ~{} (sqrt of w²+h²), got {}",
        expected, f
    );
}

/// Engine-level verification: imports are transparent to evaluation.
#[test]
fn engine_eval_with_import() {
    use reify_types::{CompiledExpr, ModulePath, Type, ValueCellId};

    let template = TopologyTemplateBuilder::new("S")
        .param("S", "x", Type::length(), Some(CompiledExpr::literal(mm(50.0), Type::length())))
        .build();

    let module = CompiledModuleBuilder::new(ModulePath::single("test"))
        .import("std/math")
        .template(template)
        .build();

    // Verify imports are stored
    assert_eq!(module.imports.len(), 1);
    assert_eq!(module.imports[0].path, "std/math");

    // Evaluate — imports should not affect evaluation
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    let x_id = ValueCellId::new("S", "x");
    let val = result.values.get(&x_id).expect("S.x should be in values");
    let f = val.as_f64().expect("should be numeric");
    assert!(
        (f - 0.05).abs() < 1e-10,
        "S.x should be ~0.05 SI (50mm), got {}",
        f
    );
}

/// Comprehensive E2E: all three features (import, stdlib, sub-component) through Engine.
#[test]
fn e2e_all_three_features_through_engine() {
    use reify_types::{ModulePath, ValueCellId};

    let source = r#"import "std/math"

structure Child {
    param size: Scalar = 10mm
    let half = size / 2
}

structure Parent {
    param w: Scalar = 80mm
    let diag = sqrt(w * w)
    sub part = Child(size: w / 2)
    constraint diag > 0mm
}"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("e2e_all_three"));
    assert!(parsed.errors.is_empty(), "parse errors: {:?}", parsed.errors);

    let compiled = reify_compiler::compile(&parsed);

    // (a) Module has 1 import
    assert_eq!(compiled.imports.len(), 1);
    assert_eq!(compiled.imports[0].path, "std/math");

    // Evaluate
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    // (b) Parent values
    let w_id = ValueCellId::new("Parent", "w");
    let w_val = result.values.get(&w_id).expect("Parent.w should exist");
    let w_f = w_val.as_f64().expect("numeric");
    assert!((w_f - 0.08).abs() < 1e-10, "Parent.w should be ~0.08, got {}", w_f);

    let diag_id = ValueCellId::new("Parent", "diag");
    let diag_val = result.values.get(&diag_id).expect("Parent.diag should exist");
    let diag_f = diag_val.as_f64().expect("numeric");
    // sqrt(0.08^2) = 0.08
    assert!((diag_f - 0.08).abs() < 1e-10, "Parent.diag should be ~0.08, got {}", diag_f);

    // (c) Sub-component param: Parent.part.size = w / 2 = 0.04
    let size_id = ValueCellId::new("Parent.part", "size");
    let size_val = result.values.get(&size_id).expect("Parent.part.size should exist");
    let size_f = size_val.as_f64().expect("numeric");
    assert!((size_f - 0.04).abs() < 1e-10, "Parent.part.size should be ~0.04, got {}", size_f);

    // (d) Sub-component let: Parent.part.half = size / 2 = 0.02
    let half_id = ValueCellId::new("Parent.part", "half");
    let half_val = result.values.get(&half_id).expect("Parent.part.half should exist");
    let half_f = half_val.as_f64().expect("numeric");
    assert!((half_f - 0.02).abs() < 1e-10, "Parent.part.half should be ~0.02, got {}", half_f);

    // (e) Constraint check
    let (constraint_results, _check_diags) = engine.check_constraints_with_values(&result.values);
    assert!(!constraint_results.is_empty(), "should have at least one constraint result");

    // (f) No error-level diagnostics (only import warning is allowed)
    let errors: Vec<_> = result.diagnostics.iter()
        .filter(|d| d.severity == reify_types::Severity::Error)
        .collect();
    assert!(errors.is_empty(), "no error diagnostics expected, got {:?}", errors);
}
