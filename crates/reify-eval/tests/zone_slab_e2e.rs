//! End-to-end B6 CI signal for `zone_slab` (task 4477 step-7/8).
//!
//! Compiles `zone_slab(rectangle(width: 40mm, height: 20mm), 2mm)` through the
//! full source → parse → compile → Engine (OCCT) → Volume-query pipeline and
//! asserts the right-prism volume identity V = w·A within 1e-9 relative error
//! (B6 acceptance criterion: PRD §9 `docs/prds/v0_6/gdt-geometric-zones-and-containment.md`).
//!
//! The compile-clean assertion runs unconditionally so a grammar/compile
//! regression fails on every runner. The kernel build + numeric assertion is
//! gated on `reify_kernel_occt::OCCT_AVAILABLE` and skips cleanly otherwise.

use reify_constraints::SimpleConstraintChecker;
use reify_core::{DimensionVector, ValueCellId};
use reify_ir::{ExportFormat, Value};
use reify_test_support::{errors_only, parse_and_compile_with_stdlib};

/// Compile `source` through the full pipeline with a real OCCT kernel and return
/// the `BuildResult`. Returns `None` when OCCT is unavailable.
fn compile_and_build_occt(source: &str) -> Option<reify_eval::BuildResult> {
    let compiled = parse_and_compile_with_stdlib(source);
    assert!(
        errors_only(&compiled).is_empty(),
        "zone_slab fixture should compile with no error-severity diagnostics, got:\n{:#?}",
        errors_only(&compiled)
    );

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping real-OCCT assertions: OCCT not available");
        return None;
    }

    let checker = SimpleConstraintChecker;
    let mut planner = reify_geometry::SingleKernelHolder::new();
    planner.register_kernel(Box::new(reify_kernel_occt::OcctKernelHandle::spawn()));
    let mut engine = reify_eval::Engine::new(Box::new(checker), Some(Box::new(planner)));
    Some(engine.build(&compiled, ExportFormat::Step))
}

const ZONE_SLAB_SOURCE: &str = r#"
structure S {
    let f = rectangle(width: 40mm, height: 20mm)
    let s = zone_slab(f, 2mm)
    let v = volume(s)
}
"#;

/// B6 CI signal: `zone_slab(rectangle(40mm, 20mm), 2mm)` realizes through the
/// OCCT engine and the queried volume matches the right-prism identity
/// V = w·A = 0.002 · (0.040 × 0.020) = 1.6e-6 m³ within 1e-9 relative error.
#[test]
fn zone_slab_planar_rectangle_volume_identity_e2e() {
    let Some(result) = compile_and_build_occt(ZONE_SLAB_SOURCE) else {
        return;
    };

    let geom_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == reify_core::Severity::Error)
        .collect();
    assert!(
        geom_errors.is_empty(),
        "zone_slab build produced unexpected Error diagnostics: {:#?}",
        geom_errors
    );

    // w·A = 0.002 m × (0.040 m × 0.020 m) = 1.6e-6 m³
    let width = 0.002_f64;
    let area = 0.040 * 0.020;
    let expected_volume = width * area;

    let v_cell = result.values.get(&ValueCellId::new("S", "v"));
    match v_cell {
        Some(Value::Scalar {
            si_value,
            dimension,
        }) => {
            assert_eq!(
                *dimension,
                DimensionVector::VOLUME,
                "volume() cell must have VOLUME dimension, got {:?}",
                dimension
            );
            let rel_err = (si_value - expected_volume).abs() / expected_volume;
            assert!(
                rel_err < 1e-9,
                "zone_slab volume should be {:.6e} m³ (w·A), got {:.6e} (rel_err={:.4e})",
                expected_volume,
                si_value,
                rel_err
            );
        }
        Some(other) => panic!(
            "expected Value::Scalar(Volume) for 'v', got {:?}",
            other
        ),
        None => panic!(
            "no value cell 'v' in build result; available cells: {:?}",
            result.values.iter().map(|(k, _)| k).collect::<Vec<_>>()
        ),
    }
}
