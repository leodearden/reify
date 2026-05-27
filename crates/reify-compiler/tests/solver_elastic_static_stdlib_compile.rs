//! Tests for `crates/reify-compiler/stdlib/solver_elastic.ri` —
//! the `fn solve_elastic_static` declaration in `std.solver.elastic`.
//!
//! Observable signal for PRD §8 task η (docs/prds/v0_3/compute-node-contract.md):
//! the stdlib function must carry `@optimized("solver::elastic_static")` so the
//! @optimized → ComputeNode lowering fires at eval time.
//!
//! These are RED tests for step-1. They fail until step-2 adds the declaration.

use reify_compiler::*;
use reify_core::*;
use reify_ir::*;

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Return the `std/solver/elastic` CompiledModule from the production stdlib
/// loader. Exercises the exact same code path as production: embedded source,
/// sequential compilation with growing prelude, OnceLock caching.
///
/// Panics with a helpful message (listing available paths) if the module is not
/// found — the expected failure mode before step-2 lands the declaration.
fn load_stdlib_module() -> &'static CompiledModule {
    stdlib_loader::load_stdlib()
        .iter()
        .find(|m| m.path.to_string() == "std/solver/elastic")
        .unwrap_or_else(|| {
            panic!(
                "stdlib should contain std/solver/elastic module; available paths: {:?}",
                stdlib_loader::load_stdlib()
                    .iter()
                    .map(|m| m.path.to_string())
                    .collect::<Vec<_>>()
            )
        })
}

/// Look up `solve_elastic_static` in the stdlib module's `functions` vec.
///
/// Panics if not found — the expected failure mode for step-1 (RED).
fn find_fn() -> &'static CompiledFunction {
    let module = load_stdlib_module();
    module
        .functions
        .iter()
        .find(|f| f.name == "solve_elastic_static")
        .unwrap_or_else(|| {
            panic!(
                "fn solve_elastic_static not found in std/solver/elastic; \
                 available functions: {:?}",
                module
                    .functions
                    .iter()
                    .map(|f| f.name.as_str())
                    .collect::<Vec<_>>()
            )
        })
}

// ─── tests ───────────────────────────────────────────────────────────────────

/// Pin: `fn solve_elastic_static` must carry `@optimized("solver::elastic_static")`.
///
/// The @optimized → ComputeNode lowering in `engine_eval.rs:2793-2944` inspects
/// `CompiledFunction.optimized_target`; if it is `None` the function body is
/// inlined instead of dispatched. This test ensures the lowering fires correctly.
#[test]
fn solve_elastic_static_has_optimized_target() {
    let f = find_fn();
    assert_eq!(
        f.optimized_target,
        Some("solver::elastic_static".to_string()),
        "fn solve_elastic_static must be annotated @optimized(\"solver::elastic_static\")"
    );
}

/// Pin: `fn solve_elastic_static` must have exactly 7 parameters.
///
/// Expected signature:
///   (material: ElasticMaterial, length: Length, width: Length, height: Length,
///    loads: List<Load>, supports: List<Support>, options: ElasticOptions)
///
/// A param-count change here means the trampoline's `value_inputs` indexing
/// (step-8) needs to be updated in lock-step with this test.
#[test]
fn solve_elastic_static_has_seven_params() {
    let f = find_fn();
    assert_eq!(
        f.params.len(),
        7,
        "expected 7 params (material, length, width, height, loads, supports, options), \
         got {:?}",
        f.params.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>()
    );
}
