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

/// Engine::eval captures dependency traces for let bindings.
#[test]
fn eval_captures_traces_for_let_bindings() {
    use reify_types::{NodeId, ValueCellId};

    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&module);

    // The volume let binding reads width, height, thickness
    let volume_node = NodeId::ValueCell(ValueCellId::new("Bracket", "volume"));
    let volume_trace = result.traces.get(&volume_node)
        .expect("volume node should have a trace");

    assert_eq!(volume_trace.reads.len(), 3);
    assert_eq!(volume_trace.reads[0], ValueCellId::new("Bracket", "width"));
    assert_eq!(volume_trace.reads[1], ValueCellId::new("Bracket", "height"));
    assert_eq!(volume_trace.reads[2], ValueCellId::new("Bracket", "thickness"));

    // Reverse index: width's dependents should include volume
    let width_deps = result.reverse_deps.dependents_of(&ValueCellId::new("Bracket", "width"));
    assert!(width_deps.contains(&volume_node));

    // Params have no trace (they're inputs, not computed)
    let width_node = NodeId::ValueCell(ValueCellId::new("Bracket", "width"));
    assert!(!result.traces.contains_key(&width_node));
}

/// Engine::check captures constraint traces in addition to value cell traces.
#[test]
fn check_captures_constraint_traces() {
    use reify_types::{ConstraintNodeId, NodeId, ValueCellId};

    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new();
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.check(&module);

    // Constraint 0: thickness > 2mm → reads thickness
    let c0_node = NodeId::Constraint(ConstraintNodeId::new("Bracket", 0));
    let c0_trace = result.traces.get(&c0_node)
        .expect("constraint 0 should have a trace");
    assert!(c0_trace.reads.contains(&ValueCellId::new("Bracket", "thickness")));

    // Constraint 1: thickness < width / 4 → reads thickness, width
    let c1_node = NodeId::Constraint(ConstraintNodeId::new("Bracket", 1));
    let c1_trace = result.traces.get(&c1_node)
        .expect("constraint 1 should have a trace");
    assert!(c1_trace.reads.contains(&ValueCellId::new("Bracket", "thickness")));
    assert!(c1_trace.reads.contains(&ValueCellId::new("Bracket", "width")));

    // Reverse deps: thickness should be depended on by volume, c0, c1, c2
    let thickness_deps = result.reverse_deps
        .dependents_of(&ValueCellId::new("Bracket", "thickness"));
    let volume_node = NodeId::ValueCell(ValueCellId::new("Bracket", "volume"));
    assert!(thickness_deps.contains(&volume_node));
    assert!(thickness_deps.contains(&c0_node));
    assert!(thickness_deps.contains(&c1_node));
}
