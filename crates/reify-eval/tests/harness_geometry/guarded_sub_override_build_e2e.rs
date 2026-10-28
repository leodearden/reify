//! A guarded sub's per-instance override realization follows eval's guard
//! decision: `Engine::build` realizes a recursive child only where eval
//! admitted it, and realizes it with that instance's own overrides.

use reify_compiler::CompiledModule;
use reify_constraints::SimpleConstraintChecker;
use reify_core::{Diagnostic, Severity};
use reify_eval::{Engine, SolverPosture};
use reify_ir::{ExportFormat, GeometryOp, Value};
use reify_test_support::{MockGeometryKernel, compile_source};

const SIZE_TOLERANCE_M: f64 = 1e-12;

/// A self-recursive `Tower` whose root `depth` is `depth`, followed by
/// `extra_source`.
fn compiled_tower(depth: i64, extra_source: &str) -> CompiledModule {
    let source = format!(
        "structure def Tower {{
    param depth : Int = {depth}
    param size : Length = 10mm
    sub child = Tower(depth: depth - 1, size: size / 2) where depth > 0
    let body = box(size, size, size)
}}
{extra_source}"
    );
    let compiled = compile_source(&source);
    assert_no_errors(&compiled.diagnostics, "Tower compile");
    compiled
}

fn assert_no_errors(diagnostics: &[Diagnostic], context: &str) {
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| &d.message)
        .collect();
    assert!(
        errors.is_empty(),
        "{context} must have no errors; got: {errors:?}"
    );
}

fn engine_with(kernel: MockGeometryKernel) -> Engine {
    Engine::new(
        SolverPosture::SolverFree,
        Box::new(SimpleConstraintChecker),
        Some(Box::new(kernel)),
    )
}

/// Builds `Tower(depth)` and returns the sorted SI `size_x` of every `Box`
/// the kernel was asked to make.
fn built_box_sizes(depth: i64) -> Vec<f64> {
    let compiled = compiled_tower(depth, "");
    let kernel = MockGeometryKernel::new();
    let ops_ref = kernel.operations_ref();
    let result = engine_with(kernel).build(&compiled, ExportFormat::Step);
    assert_no_errors(&result.diagnostics, "Tower build");

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

/// Pins the override VALUE an admitted child realizes with, not how many
/// instances exist: the build realizes one instance more than once, so a Box
/// op count is not an instance count.
#[test]
fn guard_admitted_recursive_sub_realizes_child_with_its_overrides() {
    let sizes = built_box_sizes(1);
    let mut distinct = sizes.clone();
    distinct.dedup_by(|a, b| (*a - *b).abs() < SIZE_TOLERANCE_M);

    assert_eq!(
        count_of_size(&sizes, 0.010),
        1,
        "exactly the root's 10mm body, and no child realized at template scope; got {sizes:?}"
    );
    assert!(
        distinct.len() == 2 && count_of_size(&distinct, 0.005) == 1,
        "the admitted child realizes with its `size / 2` override and at no other size; \
         got {sizes:?}"
    );
}

/// Eval admits `Tower.child` (depth 1) and declines its grandchild (depth 0),
/// so exactly two bodies surface. The second template is what lets a phantom
/// grandchild surface at all: the surfacing walk stops at a depth equal to the
/// module's template count.
#[test]
#[ignore = "blocked on #9547 — the surfacing walk reads the guard verdict by template name, one level deep, so the declined Tower.child.child surfaces a phantom body"]
fn guard_declined_grandchild_of_an_admitted_child_surfaces_no_body() {
    let compiled = compiled_tower(1, "structure def Marker { param m : Int = 0 }");

    let result = engine_with(MockGeometryKernel::new()).tessellate_realizations(&compiled);
    assert_no_errors(&result.diagnostics, "Tower tessellation");

    let mut surfaced: Vec<&str> = result
        .meshes
        .iter()
        .map(|surface| surface.entity_path.as_str())
        .collect();
    surfaced.sort_unstable();
    assert_eq!(
        surfaced,
        vec!["Tower#realization[0]", "Tower.child#realization[0]"]
    );
}
