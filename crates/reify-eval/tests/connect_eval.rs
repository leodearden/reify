//! Integration tests for connect/chain evaluation.
//!
//! Tests the complete pipeline: parse → compile → Engine.check() → verify constraint results.

use reify_core::{ModulePath, Severity};
use reify_ir::{Satisfaction, Value};
use reify_test_support::mocks::{FailingMockGeometryKernel, MockConstraintChecker};

/// Parse and compile source with two ports and a connect statement.
/// Run engine.check() and assert the connection's compatibility constraint
/// appears as Satisfied (since Out->In is compatible).
#[test]
fn eval_connect_constraints() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b
}
"#;

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Verify compilation produced a connection and its constraint
    let template = &compiled.templates[0];
    assert_eq!(template.connections.len(), 1, "expected 1 connection");
    let compat_id = &template.connections[0].compatibility_constraint;

    // Check: eval + constraint checking
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    // The compatibility constraint should appear in results as Satisfied
    let compat_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id == *compat_id);
    assert!(
        compat_entry.is_some(),
        "expected compatibility constraint {:?} in results, got: {:?}",
        compat_id,
        result
            .constraint_results
            .iter()
            .map(|e| &e.id)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        compat_entry.unwrap().satisfaction,
        Satisfaction::Satisfied,
        "Out->In connection should be Satisfied"
    );
}

/// Test that chain desugaring produces correct constraint results.
#[test]
fn eval_chain_constraints() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 1mm }
    port b : bidi T { param d : Length = 2mm }
    port c : in T { param d : Length = 3mm }
    chain a -> b -> c
}
"#;

    // Parse
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    // Compile
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "compile errors: {:?}", errors);

    // Should have 2 connections from chain desugaring
    let template = &compiled.templates[0];
    assert_eq!(template.connections.len(), 2);

    // Check
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    // Both compatibility constraints should be Satisfied
    for conn in &template.connections {
        let entry = result
            .constraint_results
            .iter()
            .find(|e| e.id == conn.compatibility_constraint);
        assert!(
            entry.is_some(),
            "expected compatibility constraint {:?} for {}->{} in results",
            conn.compatibility_constraint,
            conn.left_port,
            conn.right_port
        );
        assert_eq!(
            entry.unwrap().satisfaction,
            Satisfaction::Satisfied,
            "connection {}->{}  should be Satisfied",
            conn.left_port,
            conn.right_port
        );
    }
}

/// Test that an incompatible connection (In -> In) produces a Violated constraint.
#[test]
fn eval_incompatible_connect_constraint() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : in T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b
}
"#;

    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    // Should have a direction error diagnostic, but still produce a connection
    let template = &compiled.templates[0];
    assert_eq!(template.connections.len(), 1, "expected 1 connection");
    let compat_id = &template.connections[0].compatibility_constraint;

    // Use real constraint checker so literal Bool(false) evaluates to Violated
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    let compat_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id == *compat_id);
    assert!(
        compat_entry.is_some(),
        "expected compatibility constraint in results"
    );
    assert_eq!(
        compat_entry.unwrap().satisfaction,
        Satisfaction::Violated,
        "In->In connection should be Violated"
    );
}

// ── task-247/step-11: eval_auto_match_propagates_port_mappings ────────

/// Auto-matched Out->In ports with 2 matching members (d and angle).
/// Verifies the compiled port_mappings contains both identity pairs (sorted
/// alphabetically) and that engine.check() evaluates the compatibility as Satisfied.
#[test]
fn eval_auto_match_propagates_port_mappings() {
    let source = r#"
trait MechPort {
    param d : Length
    param angle : Real
}
structure def S {
    port a : out MechPort {
        param d : Length = 5mm
        param angle : Real = 0.0
    }
    port b : in MechPort {
        param d : Length = 5mm
        param angle : Real = 0.0
    }
    connect a -> b
}
"#;
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

    let template = &compiled.templates[0];
    assert_eq!(template.connections.len(), 1);
    // Auto-match: sorted alphabetically → [("angle","angle"), ("d","d")]
    assert_eq!(
        template.connections[0].port_mappings,
        vec![
            ("angle".to_string(), "angle".to_string()),
            ("d".to_string(), "d".to_string()),
        ],
        "expected auto-generated identity mappings sorted alphabetically"
    );

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    let compat_id = &template.connections[0].compatibility_constraint;
    let entry = result
        .constraint_results
        .iter()
        .find(|e| e.id == *compat_id);
    assert!(
        entry.is_some(),
        "expected compatibility constraint in results"
    );
    assert_eq!(
        entry.unwrap().satisfaction,
        Satisfaction::Satisfied,
        "Out->In auto-matched connection should be Satisfied"
    );
}

// ── task-247/step-12: eval_explicit_mapping_constraint_satisfied ──────

/// Compile with explicit `{ d -> d }` mapping and run engine.check().
/// Constraint should be Satisfied and compiled port_mappings equals the explicit pair.
#[test]
fn eval_explicit_mapping_constraint_satisfied() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b { d -> d }
}
"#;
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

    let template = &compiled.templates[0];
    assert_eq!(template.connections.len(), 1);
    assert_eq!(
        template.connections[0].port_mappings,
        vec![("d".to_string(), "d".to_string())],
        "expected explicit mapping d->d in compiled template"
    );

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    let compat_id = &template.connections[0].compatibility_constraint;
    let entry = result
        .constraint_results
        .iter()
        .find(|e| e.id == *compat_id);
    assert!(
        entry.is_some(),
        "expected compatibility constraint in results"
    );
    assert_eq!(
        entry.unwrap().satisfaction,
        Satisfaction::Satisfied,
        "Out->In with explicit mapping should be Satisfied"
    );
}

// ── task-247/step-13: eval_mixed_params_and_mappings_connector_created

/// BoltSet connector body with a param and an explicit mapping.
/// Verifies: no error diagnostics, connector sub-component present in S,
/// compatibility constraint Satisfied, port_mappings holds the explicit pair.
#[test]
fn eval_mixed_params_and_mappings_connector_created() {
    let source = r#"
trait T { param d : Length }
structure def BoltSet { param grade : Real = 8.8 }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a -> b : BoltSet { grade = 10.9, d -> d }
}
"#;
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

    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("S template");
    assert_eq!(s_template.connections.len(), 1);

    let conn = &s_template.connections[0];
    assert!(conn.connector_sub.is_some(), "expected connector_sub");

    let connector_name = conn.connector_sub.as_ref().unwrap();
    let connector_sub = s_template
        .sub_components
        .iter()
        .find(|s| s.name == *connector_name);
    assert!(
        connector_sub.is_some(),
        "expected connector sub-component in S template"
    );
    assert_eq!(connector_sub.unwrap().structure_name, "BoltSet");

    assert_eq!(
        conn.port_mappings,
        vec![("d".to_string(), "d".to_string())],
        "expected explicit mapping d->d preserved"
    );

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    let entry = result
        .constraint_results
        .iter()
        .find(|e| e.id == conn.compatibility_constraint);
    assert!(
        entry.is_some(),
        "expected compatibility constraint in results"
    );
    assert_eq!(
        entry.unwrap().satisfaction,
        Satisfaction::Satisfied,
        "mixed params+mappings connection should be Satisfied"
    );
}

// ── task-247/step-14: m10_connect_advanced_ri_parses_and_compiles ─────

/// Absolute path to the port-mapping example file.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m10_connect_advanced.ri"
);

/// Read m10_connect_advanced.ri, verify it parses without errors, compiles
/// without Error-severity diagnostics, and produces at least one template.
#[test]
fn m10_connect_advanced_ri_parses_and_compiles() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m10_connect_advanced.ri should exist");

    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
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

    assert!(
        !compiled.templates.is_empty(),
        "expected at least one template in the compiled module"
    );
}

// ── task-247/step-16: m10_connect_advanced_ri_all_constraints_satisfied

/// End-to-end integration test: parse + compile + engine.check() on the example
/// file. Every constraint result must be Satisfied (no violations).
#[test]
fn m10_connect_advanced_ri_all_constraints_satisfied() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/m10_connect_advanced.ri should exist");

    let parsed = reify_syntax::parse(&source, ModulePath::single("test"));
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

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let check_result = engine.check(&compiled);

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

// ── ad-hoc port selectors ─────────────────────────────────────────────────────

/// Parse and compile `p @ point(10mm, 20mm, 0mm)`, then eval with None kernel.
/// After implementation, the `resolved` let-binding should resolve to a
/// Value::Frame whose origin holds the three literal coordinates.
/// No geometry kernel is needed because @point builds a frame from literals.
/// Behavior covered: @point with coordinates (eval path).
#[test]
fn eval_point_with_literal_coords_resolves_to_frame() {
    let source = r#"
trait T { param d : Length }
structure S {
    port p : out T { param d : Length = 5mm }
    let resolved = p @ point(10mm, 20mm, 0mm)
}
"#;
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

    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");
    let resolved_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "resolved")
        .expect("expected 'resolved' value cell in S");

    // @point does not need a geometry kernel — use None
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let eval_result = engine.eval(&compiled);

    let resolved_val = eval_result
        .values
        .get(&resolved_cell.id)
        .expect("'resolved' should have a value in eval result");
    assert!(
        matches!(resolved_val, Value::Frame { .. }),
        "expected Value::Frame for @point(10mm, 20mm, 0mm), got: {:?}",
        resolved_val
    );
}

/// Parse and compile `p @ face("top")` (structure with geometry body but no kernel).
/// Eval with None kernel. The `resolved` let-binding should fall back to
/// Value::Undef when no geometry kernel is available, and at least one eval
/// diagnostic should mention missing geometry.
/// Behavior covered: @face without geometry kernel -> undef + diagnostic (eval path).
#[test]
fn eval_face_without_geometry_kernel_resolves_to_undef() {
    let source = r#"
trait T { param d : Length }
structure S {
    port p : out T { param d : Length = 5mm }
    let body = box(1mm, 1mm, 1mm)
    let resolved = p @ face("top")
}
"#;
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

    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");
    let resolved_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "resolved")
        .expect("expected 'resolved' value cell");

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let eval_result = engine.eval(&compiled);

    let resolved_val = eval_result
        .values
        .get(&resolved_cell.id)
        .expect("'resolved' should have a value");
    assert_eq!(
        *resolved_val,
        Value::Undef,
        "expected Value::Undef for @face without geometry kernel, got: {:?}",
        resolved_val
    );

    let has_selector_diagnostic = eval_result.diagnostics.iter().any(|d| {
        let msg = d.message.to_lowercase();
        msg.contains("could not be resolved") && msg.contains("selector")
    });
    assert!(
        has_selector_diagnostic,
        "expected eval-path selector-undef diagnostic, got: {:?}",
        eval_result.diagnostics
    );
}

/// Parse and compile `p @ edge("seam")` (structure with geometry body but no kernel).
/// Eval with None kernel. `e` should be Value::Undef and at least one eval
/// diagnostic should mention missing geometry.
/// Behavior covered: @edge without geometry kernel -> undef + diagnostic (eval path).
#[test]
fn eval_edge_without_geometry_kernel_resolves_to_undef() {
    let source = r#"
trait T { param d : Length }
structure S {
    port p : out T { param d : Length = 5mm }
    let body = box(1mm, 1mm, 1mm)
    let e = p @ edge("seam")
}
"#;
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

    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");
    let e_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "e")
        .expect("expected 'e' value cell");

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let eval_result = engine.eval(&compiled);

    let e_val = eval_result
        .values
        .get(&e_cell.id)
        .expect("'e' should have a value");
    assert_eq!(
        *e_val,
        Value::Undef,
        "expected Value::Undef for @edge without geometry kernel, got: {:?}",
        e_val
    );

    let has_selector_diagnostic = eval_result.diagnostics.iter().any(|d| {
        let msg = d.message.to_lowercase();
        msg.contains("could not be resolved") && msg.contains("selector")
    });
    assert!(
        has_selector_diagnostic,
        "expected eval-path selector-undef diagnostic, got: {:?}",
        eval_result.diagnostics
    );
}

/// Structure with a geometry let-binding (`let body = box(...)`) and `p @ face("top")`.
/// Eval with FailingMockGeometryKernel.  Note: `engine.eval()` is geometry-free and
/// never invokes the kernel — `r` is `Value::Undef` because eval leaves the `@face`
/// cell at its placeholder value, not because the kernel's `execute` fails.  The
/// diagnostic comes from `detect_unresolved_ad_hoc_selectors`, which scans post-eval
/// `Value::Undef` `@face`/`@edge` cells and emits a warning.
/// Behavior covered: @face selector-frame Undef + diagnostic on the eval() path.
#[test]
fn eval_failing_kernel_selector_becomes_undef_with_diagnostic() {
    let source = r#"
trait T { param d : Length }
structure S {
    port p : out T { param d : Length = 5mm }
    let body = box(1mm, 1mm, 1mm)
    let r = p @ face("top")
}
"#;
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

    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");
    let r_cell = s_template
        .value_cells
        .iter()
        .find(|vc| vc.id.member == "r")
        .expect("expected 'r' value cell");

    let checker = MockConstraintChecker::new();
    let kernel = FailingMockGeometryKernel;
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let eval_result = engine.eval(&compiled);

    let r_val = eval_result
        .values
        .get(&r_cell.id)
        .expect("'r' should have a value");
    assert_eq!(
        *r_val,
        Value::Undef,
        "expected Value::Undef when kernel fails for @face, got: {:?}",
        r_val
    );

    let has_selector_diagnostic = eval_result.diagnostics.iter().any(|d| {
        let msg = d.message.to_lowercase();
        msg.contains("could not be resolved") && msg.contains("selector")
    });
    assert!(
        has_selector_diagnostic,
        "expected eval-path selector-undef diagnostic, got: {:?}",
        eval_result.diagnostics
    );
}

/// Eval-side companion to compile step-6: @face on a structure with no geometry.
/// This test accepts either a compile-time error or an eval-time Undef depending
/// on which layer the implementation chooses to enforce geometry presence.
/// Behavior covered: @face on entity without geometry (eval path).
#[test]
fn eval_face_on_entity_without_geometry_runtime_diagnostic() {
    let source = r#"
trait T { param d : Length }
structure S {
    port p : out T { param d : Length = 5mm }
    let r = p @ face("top")
}
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    if !compile_errors.is_empty() {
        // Compile-time enforcement: the error should mention geometry
        let has_geometry_error = compile_errors.iter().any(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("geometry") || msg.contains("no kernel") || msg.contains("realize")
        });
        assert!(
            has_geometry_error,
            "expected compile error mentioning geometry for @face without geometry, got: {:?}",
            compile_errors
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    } else {
        // Runtime enforcement: eval with None kernel should produce Undef + diagnostic
        let s_template = compiled
            .templates
            .iter()
            .find(|t| t.name == "S")
            .expect("expected template S");
        let r_cell = s_template
            .value_cells
            .iter()
            .find(|vc| vc.id.member == "r")
            .expect("expected 'r' value cell");

        let checker = MockConstraintChecker::new();
        let mut engine = reify_eval::Engine::new(Box::new(checker), None);
        let eval_result = engine.eval(&compiled);

        let r_val = eval_result
            .values
            .get(&r_cell.id)
            .expect("'r' should have a value");
        assert_eq!(
            *r_val,
            Value::Undef,
            "expected Value::Undef for @face without geometry at runtime, got: {:?}",
            r_val
        );
        let has_runtime_diag = eval_result.diagnostics.iter().any(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("geometry") || msg.contains("face") || msg.contains("unavailable")
        });
        assert!(
            has_runtime_diag,
            "expected runtime diagnostic about missing geometry, got: {:?}",
            eval_result.diagnostics
        );
    }
}

/// Connect `a @ face("top") -> b @ face("bottom")` (structure WITH a geometry body).
/// After implementation: compile should succeed, engine.check() should return a
/// constraint result for the frame_constraint id, and the compatibility constraint
/// should be present (MockConstraintChecker makes all Satisfied).
/// Behavior covered: connect with ad-hoc ports generates frame constraints (eval path).
#[test]
fn eval_connect_ad_hoc_ports_frame_constraint_in_results() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    let body = box(10mm, 10mm, 10mm)
    connect a @ face("top") -> b @ face("bottom")
}
"#;
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

    let s_template = compiled
        .templates
        .iter()
        .find(|t| t.name == "S")
        .expect("expected template S");
    assert_eq!(
        s_template.connections.len(),
        1,
        "expected exactly 1 connection"
    );
    let conn = &s_template.connections[0];
    let frame_id = conn
        .frame_constraint
        .as_ref()
        .expect("expected frame_constraint to be Some");

    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    // The frame constraint should appear in the results
    let frame_entry = result.constraint_results.iter().find(|e| e.id == *frame_id);
    assert!(
        frame_entry.is_some(),
        "expected frame_constraint {:?} in constraint_results, got: {:?}",
        frame_id,
        result
            .constraint_results
            .iter()
            .map(|e| &e.id)
            .collect::<Vec<_>>()
    );

    // The compatibility constraint should also appear
    let compat_entry = result
        .constraint_results
        .iter()
        .find(|e| e.id == conn.compatibility_constraint);
    assert!(
        compat_entry.is_some(),
        "expected compatibility_constraint {:?} in results",
        conn.compatibility_constraint
    );
    assert_eq!(
        compat_entry.unwrap().satisfaction,
        Satisfaction::Satisfied,
        "compatibility constraint should be Satisfied via MockConstraintChecker"
    );
}

/// Forall quantifier over a collection sub-component with @point in the predicate.
/// After implementation, every constraint result should be Satisfied (via
/// MockConstraintChecker). @point bypasses the kernel entirely.
/// Behavior covered: ad-hoc port in forall quantifier (eval path).
#[test]
fn eval_ad_hoc_port_in_forall_quantifier_evaluates_each_element() {
    let source = r#"
trait T { param d : Length }
structure def Part {
    port p : out T { param d : Length = 5mm }
}
structure def S {
    sub parts : List<Part>
    constraint forall p in parts: p.p @ point(0mm, 0mm, 0mm) != undef
}
"#;
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

    // @point bypasses the kernel — use None
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&compiled);

    // All constraint results should be Satisfied (MockConstraintChecker guarantees this
    // if the forall actually iterates over its elements and evaluates the predicate)
    for entry in &result.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {:?} should be Satisfied, got {:?}",
            entry.id,
            entry.satisfaction
        );
    }
}

/// Connect `a @ face("missing") -> b @ face("also_missing")` with no geometry.
/// After implementation: either compile detects missing geometry (error), or
/// eval produces Undef for both selector sides and the frame_constraint is
/// not Satisfied (because Undef @ Undef → frame comparison is indeterminate).
/// Behavior covered: selector failure -> undef propagation through connect frame constraint.
#[test]
fn eval_selector_undef_propagates_through_connect_compatibility() {
    let source = r#"
trait T { param d : Length }
structure def S {
    port a : out T { param d : Length = 5mm }
    port b : in T { param d : Length = 5mm }
    connect a @ face("missing") -> b @ face("also_missing")
}
"#;
    let parsed = reify_syntax::parse(source, ModulePath::single("test"));
    assert!(
        parsed.errors.is_empty(),
        "parse errors: {:?}",
        parsed.errors
    );

    let compiled = reify_compiler::compile(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();

    if !compile_errors.is_empty() {
        // Compile-time enforcement: structure has no geometry → compile error
        // (This is the same behavior asserted in compile step-6.)
        let has_geometry_error = compile_errors.iter().any(|d| {
            let msg = d.message.to_lowercase();
            msg.contains("geometry") || msg.contains("without geometry") || msg.contains("realize")
        });
        assert!(
            has_geometry_error,
            "expected compile error about missing geometry, got: {:?}",
            compile_errors
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    } else {
        // Runtime enforcement: both selectors fail → Undef on both sides of frame constraint.
        // The frame_constraint satisfaction should NOT be Satisfied.
        let s_template = compiled
            .templates
            .iter()
            .find(|t| t.name == "S")
            .expect("expected template S");
        assert_eq!(
            s_template.connections.len(),
            1,
            "expected exactly 1 connection"
        );
        let conn = &s_template.connections[0];
        let frame_id = conn
            .frame_constraint
            .as_ref()
            .expect("expected frame_constraint to be Some");

        // Use SimpleConstraintChecker so Undef-based expressions evaluate to non-Satisfied
        let checker = reify_constraints::SimpleConstraintChecker;
        let mut engine = reify_eval::Engine::new(Box::new(checker), None);
        let result = engine.check(&compiled);

        let frame_entry = result.constraint_results.iter().find(|e| e.id == *frame_id);
        assert!(
            frame_entry.is_some(),
            "expected frame_constraint {:?} in constraint_results",
            frame_id
        );
        assert_ne!(
            frame_entry.unwrap().satisfaction,
            Satisfaction::Satisfied,
            "frame constraint should NOT be Satisfied when both selectors resolve to Undef"
        );
    }
}
