//! End-to-end example test for `map_or` via the worked example
//! `examples/option_map_or.ri` (task 4595 step-11/step-12).
//!
//! This is the user-observable completion signal for task 4595: an arrow-type
//! parameter `f: (T) -> U` parses (no ERROR nodes), type-checks, and evaluates
//! end-to-end, and `map_or(some(...), dflt, |x: T| ...)` APPLIES the function to
//! the unwrapped Some value while the `none` case yields the default.
//!
//! Model:
//!   - `generic_stdlib_fn_e2e.rs` — Engine::eval + value-cell read.
//!   - `compose_example_smoke.rs` — load the fixture from `examples/` at
//!     compile time via `concat!(env!("CARGO_MANIFEST_DIR"), ...)`.
//!
//! RED before step-12: `examples/option_map_or.ri` does not exist yet, so the
//! fixture read fails (panic) — the missing-example RED signal.

use reify_core::{DimensionVector, Severity, ValueCellId};
use reify_test_support::{collect_errors, parse_and_compile_with_stdlib};

/// Absolute path to the example, resolved at compile time from the crate root.
const EXAMPLE_PATH: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/option_map_or.ri");

/// A `Length` scalar of `mm` millimetres, in SI (metres).
fn len_mm(mm: f64) -> reify_ir::Value {
    reify_ir::Value::Scalar {
        si_value: mm / 1000.0,
        dimension: DimensionVector::LENGTH,
    }
}

/// `examples/option_map_or.ri` must compile + eval with zero Error diagnostics
/// (so the `(T) -> U` arrow-type annotation no longer ERROR-nodes end-to-end),
/// and the two arrow-type `map_or` cells must evaluate correctly:
///   - `map_or(some(5mm), 0mm, |x: Length| x * 2.0)` → 10mm   (lambda APPLIED)
///   - `map_or(none,      7mm, |x: Length| x * 2.0)` → 7mm     (default)
///
/// The some-case value (10mm) is the discriminating signal: the typecheck-only
/// `.ri` placeholder body `{ dflt }` returns 0mm, so only the real ctx-aware
/// intercept that applies `f` to the inner value yields f(5mm)=10mm.
#[test]
fn option_map_or_example_evals_end_to_end() {
    let source = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("examples/option_map_or.ri should exist");

    let compiled = parse_and_compile_with_stdlib(&source);

    // Arrow-type param `f: (T) -> U` must parse + type-check end-to-end:
    // zero Error diagnostics (no ERROR nodes from the (T) -> U annotation).
    let errors = collect_errors(&compiled.diagnostics);
    assert!(
        errors.is_empty(),
        "examples/option_map_or.ri should compile with no Error diagnostics, got: {errors:?}"
    );

    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::new(Box::new(checker), None);
    let result = engine.eval(&compiled);

    let eval_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(eval_errors.is_empty(), "eval errors: {eval_errors:?}");

    // some(5mm) → |x: Length| x * 2.0 → 10mm (proves the lambda was applied:
    // the placeholder body returns dflt=0mm, so 10mm != 0mm is the real signal).
    let mapped_id = ValueCellId::new("MapOrDemo", "mapped");
    let mapped = result.values.get(&mapped_id).unwrap_or_else(|| {
        panic!(
            "eval should produce MapOrDemo.mapped; available: {:?}",
            result
                .values
                .iter()
                .map(|(k, _)| k.to_string())
                .collect::<Vec<_>>()
        )
    });
    assert_eq!(
        *mapped,
        len_mm(10.0),
        "map_or(some(5mm), 0mm, |x: Length| x * 2.0) must evaluate to 10mm (lambda applied to inner)"
    );

    // none → default 7mm (lambda not applied).
    let defaulted_id = ValueCellId::new("MapOrDemo", "defaulted");
    let defaulted = result
        .values
        .get(&defaulted_id)
        .expect("eval should produce MapOrDemo.defaulted");
    assert_eq!(
        *defaulted,
        len_mm(7.0),
        "map_or(none, 7mm, |x: Length| x * 2.0) must evaluate to the default 7mm"
    );
}
