//! A guarded sub's per-instance override realization follows eval's guard
//! decision: `Engine::build` realizes a recursive child only where eval
//! admitted it, and realizes it with that instance's own overrides.

use reify_constraints::SimpleConstraintChecker;
use reify_core::Severity;
use reify_eval::{Engine, SolverPosture};
use reify_ir::{ExportFormat, GeometryOp, Value};
use reify_test_support::{MockGeometryKernel, compile_source};

const SIZE_TOLERANCE_M: f64 = 1e-12;

/// Builds a self-recursive `Tower` whose root `depth` is `depth`, and returns
/// the sorted SI `size_x` of every `Box` the kernel was asked to make.
fn built_box_sizes(depth: i64) -> Vec<f64> {
    let source = format!(
        "structure def Tower {{
    param depth : Int = {depth}
    param size : Length = 10mm
    sub child = Tower(depth: depth - 1, size: size / 2) where depth > 0
    let body = box(size, size, size)
}}"
    );
    let compiled = compile_source(&source);
    let compile_errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| &d.message)
        .collect();
    assert!(
        compile_errors.is_empty(),
        "Tower must compile without errors; got: {compile_errors:?}"
    );

    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let mut engine = Engine::new(
        SolverPosture::SolverFree,
        Box::new(SimpleConstraintChecker),
        Some(Box::new(kernel)),
    );
    let result = engine.build(&compiled, ExportFormat::Step);

    let build_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| &d.message)
        .collect();
    assert!(
        build_errors.is_empty(),
        "Tower(depth = {depth}) must build without errors; got: {build_errors:?}"
    );

    let mut sizes: Vec<f64> = ops_ref
        .lock()
        .unwrap()
        .iter()
        .filter_map(|rec| match &rec.op {
            GeometryOp::Box {
                size_x: Value::Scalar { si_value, .. },
                ..
            } => Some(*si_value),
            _ => None,
        })
        .collect();
    sizes.sort_by(f64::total_cmp);
    sizes
}

fn count_of_size(sizes: &[f64], size_m: f64) -> usize {
    sizes
        .iter()
        .filter(|size| (*size - size_m).abs() < SIZE_TOLERANCE_M)
        .count()
}

#[test]
fn guard_declined_recursive_sub_realizes_no_child_instance() {
    let sizes = built_box_sizes(0);

    assert!(
        sizes.len() == 1 && count_of_size(&sizes, 0.010) == 1,
        "only the root's 10mm body may be realized, got Box sizes {sizes:?}"
    );
}

#[test]
fn guard_admitted_recursive_sub_realizes_child_with_its_overrides() {
    let sizes = built_box_sizes(1);
    let root_bodies = count_of_size(&sizes, 0.010);
    let child_bodies = count_of_size(&sizes, 0.005);

    assert_eq!(
        root_bodies, 1,
        "exactly the root's 10mm body, and no child realized at template scope; got {sizes:?}"
    );
    assert!(
        child_bodies >= 1,
        "the admitted child must realize with its `size / 2` override; got {sizes:?}"
    );
    assert_eq!(
        root_bodies + child_bodies,
        sizes.len(),
        "no Box of any other size; got {sizes:?}"
    );
}
