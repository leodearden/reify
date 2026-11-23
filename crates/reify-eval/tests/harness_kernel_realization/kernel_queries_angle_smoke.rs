//! End-to-end smoke test for the `angle(Direction, Direction) -> Angle`
//! eval-side dispatch arm (task 3614, PRD
//! `docs/prds/v0_3/kernel-geometry-queries.md` §9 KGQ-ε; Direction operands
//! since direction-typing θ-b, task 9645).
//!
//! The fixture `examples/kernel_queries/angle_smoke.ri` contains:
//!
//! ```ri
//! structure def AngleSmoke {
//!     let a = dir(1, 0, 0)
//!     let b = dir(0, 1, 0)
//!     let angle_ab = angle(a, b)
//! }
//! ```
//!
//! The user-observable signal: `angle_ab` evaluates to exactly
//! `Value::angle(FRAC_PI_2)`. No kernel call is required — `angle` is
//! pure-math.
//!
//! Modelled on `block_inertia_evals_moment_of_inertia_to_tensor` in
//! `topology_selector_smoke_tests.rs` (run-time example path fn +
//! `fixture_scaffolding::read_and_compile_fixture` + `Engine::new` +
//! `engine.build` + `result.values.get` assert pattern).

use reify_constraints::SimpleConstraintChecker;
use reify_core::ValueCellId;
use reify_eval::Engine;
use reify_ir::{ExportFormat, Value};
use reify_test_support::MockGeometryKernel;

use super::fixture_scaffolding::read_and_compile_fixture;

fn angle_smoke_path() -> std::path::PathBuf {
    reify_test_support::examples_corpus::examples_dir().join("kernel_queries/angle_smoke.ri")
}

/// Pins the user-observable signal for KGQ-ε: `angle(dir(1,0,0), dir(0,1,0))`
/// must evaluate to exactly `Value::angle(FRAC_PI_2)` (1.5707963267948966 rad).
/// The unit operands' dot product is exactly 0.0 and `acos(0.0)` is the
/// correctly rounded π/2, so no epsilon is needed.
///
/// A bare `MockGeometryKernel::new()` is sufficient because `angle` never
/// queries the kernel — it is pure-math.
#[test]
fn angle_smoke_evals_to_ninety_degrees() {
    let compiled = read_and_compile_fixture(
        &angle_smoke_path(),
        "examples/kernel_queries/angle_smoke.ri (task 3614 step-4)",
    );

    let kernel = MockGeometryKernel::new();
    let checker = SimpleConstraintChecker;
    let mut engine = Engine::new(reify_eval::SolverPosture::SolverFree, Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("AngleSmoke", "angle_ab");
    assert_eq!(
        result.values.get(&cell),
        Some(&Value::angle(std::f64::consts::FRAC_PI_2)),
        "AngleSmoke.angle_ab must be exactly Value::angle(FRAC_PI_2)"
    );
}
