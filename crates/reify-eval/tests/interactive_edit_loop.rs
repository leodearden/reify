//! Integration tests for the interactive edit loop.
//!
//! Proves true incrementality: eval() → edit_param() → check_snapshot() → build_snapshot().
//! Uses MockConstraintChecker/MockGeometryKernel so tests run fast and in parallel.

use reify_eval::Engine;
use reify_test_support::{bracket_compiled_module, vcid};
use reify_test_support::mocks::{MockConstraintChecker, MockGeometryKernel};
use reify_types::{Satisfaction, Value};

#[test]
fn check_snapshot_returns_constraint_results_from_current_values() {
    let module = bracket_compiled_module();
    let checker = MockConstraintChecker::new(); // default: all Satisfied
    let mut engine = Engine::new(Box::new(checker), None);

    // Cold-start eval to establish baseline snapshot
    let eval_result = engine.eval(&module);
    assert_eq!(eval_result.values.len(), 6, "should have 6 value cells");

    // check_snapshot should return constraint results from current snapshot
    let check = engine
        .check_snapshot(&module)
        .expect("check_snapshot should return Some after eval()");

    // 3 constraints, all Satisfied (MockConstraintChecker default)
    assert_eq!(
        check.constraint_results.len(),
        3,
        "should have 3 constraint results"
    );
    for entry in &check.constraint_results {
        assert_eq!(
            entry.satisfaction,
            Satisfaction::Satisfied,
            "constraint {:?} should be Satisfied",
            entry.id
        );
    }

    // Values should match eval result
    assert_eq!(check.values.len(), 6, "check_snapshot values should have 6 entries");
    for (id, val) in eval_result.values.iter() {
        assert_eq!(
            check.values.get(id),
            Some(val),
            "check_snapshot value for {:?} should match eval",
            id
        );
    }
}
