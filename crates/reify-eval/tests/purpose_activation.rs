//! Purpose activation lifecycle tests (Task 260).
//!
//! Exercises the full purpose activate/deactivate lifecycle against the
//! Engine API delivered by Task 259:
//!   - activate_purpose / deactivate_purpose / is_purpose_active
//!   - Constraint injection and removal (snapshot.graph.constraints counts)
//!   - Reflective .params inspection via CompiledPurpose.resolved_queries
//!   - Optimization objective injection (minimize / maximize)
//!   - Example-file integration (m10_purpose_activation.ri)
//!
//! Two `#[ignore]`-annotated placeholder tests (steps 23-24) document the
//! expected API for unimplemented categories (.geometric_params filtering
//! and forall-over-reflective-queries) so a follow-up task has a landing spot.

use reify_constraints::SimpleConstraintChecker;
use reify_test_support::{make_engine, parse_and_compile, parse_and_compile_with_stdlib};
use reify_types::{ModulePath, OptimizationObjective, Satisfaction, Severity};

/// Absolute path to the example file, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/m10_purpose_activation.ri"
);
