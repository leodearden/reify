//! Structured golden eval test for the structural-query ε integration gate
//! (task 3992).
//!
//! Reads `examples/structural_query_bom.ri`, parses, compiles, and evaluates
//! it.  Asserts:
//!   - zero Error diagnostics at parse, compile, and eval stages
//!   - `Assembly.part_count == Value::Int(6)` (members: plate + bracket +
//!     bolts[0..2] + jig = 6, one level flattened, aux included)
//!   - `Assembly.bolt_count == Value::Int(4)` (transitive Bolt-conformers:
//!     bracket.anchor + bolts[0..2] = 4)
//!
//! These two distinct values demonstrate that `self.members` (one-level,
//! aux-included) and `filter(self.descendants, Bolt)` (transitive,
//! trait-filtered) are independent observables correctly discriminated by the
//! evaluator.
//!
//! RED today (step-5): `examples/structural_query_bom.ri` does not exist →
//! `read_to_string` panics with "should exist".
//! GREEN after step-6: the example is created and evaluates to the expected
//! golden values.

use reify_core::{ModulePath, Severity, ValueCellId};
use reify_ir::Value;
use reify_test_support::mocks::MockConstraintChecker;
use reify_eval::Engine;

const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/structural_query_bom.ri"
);

/// Golden eval: reads `examples/structural_query_bom.ri` and asserts that
/// `Assembly.part_count == Int(6)` and `Assembly.bolt_count == Int(4)` with
/// zero Error diagnostics at all stages.
///
/// RED today: file not found (examples/structural_query_bom.ri doesn't exist).
/// GREEN after step-6 creates the example.
#[test]
fn example_structural_query_bom_ri_evals_to_golden_counts() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/structural_query_bom.ri should exist (created by step-6)");

    // ── parse ──
    let parsed = reify_syntax::parse(
        &source,
        ModulePath::single("structural_query_bom_example"),
    );
    assert!(
        parsed.errors.is_empty(),
        "example parse errors: {:?}",
        parsed.errors
    );

    // ── compile ──
    let compiled = reify_compiler::compile(&parsed);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "example compile errors: {:?}",
        compile_errors
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // ── eval ──
    let checker = MockConstraintChecker::new();
    let mut engine = Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        eval_errors.is_empty(),
        "example eval errors (no panic from determined(m) over members): {:?}",
        eval_errors.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // ── golden: part_count = 6 ──
    //
    // `self.members` (one structural level, aux included):
    //   plate + bracket + bolts[0] + bolts[1] + bolts[2] + jig = 6
    let part_count_id = ValueCellId::new("Assembly", "part_count");
    match result.values.get(&part_count_id) {
        Some(Value::Int(n)) => {
            assert_eq!(
                *n, 6,
                "Assembly.part_count should be 6 (plate + bracket + \
                 bolts[0..2] + jig, one level flattened, aux included); got: {}",
                n
            );
        }
        other => panic!(
            "Assembly.part_count should be Value::Int(6); got: {:?}",
            other
        ),
    }

    // ── golden: bolt_count = 4 ──
    //
    // `filter(self.descendants, Bolt)` (transitive, Bolt-conforming):
    //   bracket.anchor (HexBolt) + bolts[0] + bolts[1] + bolts[2] = 4
    let bolt_count_id = ValueCellId::new("Assembly", "bolt_count");
    match result.values.get(&bolt_count_id) {
        Some(Value::Int(n)) => {
            assert_eq!(
                *n, 4,
                "Assembly.bolt_count should be 4 (bracket.anchor + \
                 bolts[0..2], transitive Bolt-conformers); got: {}",
                n
            );
        }
        other => panic!(
            "Assembly.bolt_count should be Value::Int(4); got: {:?}",
            other
        ),
    }
}
