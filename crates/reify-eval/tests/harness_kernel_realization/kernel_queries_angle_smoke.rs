//! End-to-end smoke test for the `angle(Vec3, Vec3) -> Angle` eval-side
//! dispatch arm (task 3614, PRD `docs/prds/v0_3/kernel-geometry-queries.md`
//! §9 KGQ-ε).
//!
//! The fixture `examples/kernel_queries/angle_smoke.ri` contains:
//!
//! ```ri
//! structure def AngleSmoke {
//!     let a = vec3(1.0, 0.0, 0.0)
//!     let b = vec3(0.0, 1.0, 0.0)
//!     let angle_ab = angle(a, b)
//! }
//! ```
//!
//! The user-observable signal: `angle_ab` evaluates to `Angle(90 deg)`
//! (`Value::angle(FRAC_PI_2)`). No kernel call is required — `angle` is
//! pure-math.
//!
//! Modelled on `block_inertia_evals_moment_of_inertia_to_tensor` in
//! `topology_selector_smoke_tests.rs` (CARGO_MANIFEST_DIR path const +
//! `parse_and_compile_with_stdlib` + `Engine::new` + `engine.build` +
//! `result.values.get` assert pattern).

use reify_constraints::SimpleConstraintChecker;
use reify_core::ValueCellId;
use reify_eval::Engine;
use reify_ir::{ExportFormat, Value};
use reify_test_support::{MockGeometryKernel, errors_only, parse_and_compile_with_stdlib};

const ANGLE_SMOKE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/kernel_queries/angle_smoke.ri"
);

/// Pins the user-observable signal for KGQ-ε: `angle(vec3(1,0,0), vec3(0,1,0))`
/// must evaluate to `Value::angle(FRAC_PI_2)` (90 degrees / π/2 radians).
///
/// A bare `MockGeometryKernel::new()` is sufficient because `angle` never
/// queries the kernel — it is pure-math.
#[test]
fn angle_smoke_evals_to_ninety_degrees() {
    let source = std::fs::read_to_string(ANGLE_SMOKE_PATH)
        .expect("examples/kernel_queries/angle_smoke.ri should exist (task 3614 step-4)");
    let compiled = parse_and_compile_with_stdlib(&source);
    assert!(
        errors_only(&compiled).is_empty(),
        "examples/kernel_queries/angle_smoke.ri should compile with no error-severity \
         diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    let kernel = MockGeometryKernel::new();
    let checker = SimpleConstraintChecker;
    let mut engine = Engine::new(Box::new(checker), Some(Box::new(kernel)));
    let result = engine.build(&compiled, ExportFormat::Step);

    let cell = ValueCellId::new("AngleSmoke", "angle_ab");
    let actual = result.values.get(&cell);

    // Allow a small floating-point epsilon on the si_value (acos can drift
    // by ~1 ULP on some platforms) while requiring exact ANGLE dimension.
    match actual {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) if *dimension == reify_core::DimensionVector::ANGLE => {
            let expected = std::f64::consts::FRAC_PI_2;
            let epsilon = 1e-12;
            assert!(
                (si_value - expected).abs() < epsilon,
                "AngleSmoke.angle_ab si_value should be FRAC_PI_2 (≈{expected:.15}), \
                 got {si_value:.15} (delta {delta:.3e})",
                delta = (si_value - expected).abs()
            );
        }
        other => panic!(
            "AngleSmoke.angle_ab should be Value::angle(FRAC_PI_2), got: {:?}",
            other
        ),
    }
}
