//! Timing bench for `compiled_arg.clone()` cost in `PendingBoundCheck::TraitArgConformance`.
//!
//! ## Background (task 2280)
//!
//! Task 2227 introduced an eager `compiled_arg.clone()` at
//! `crates/reify-compiler/src/entity.rs:949` inside the loop that enqueues
//! `PendingBoundCheck::TraitArgConformance` entries.  The stored field is
//! `compiled_arg: CompiledExpr` (see the variant definition at `entity.rs:1844`).
//! `CompiledExpr` is a deeply-nested, heap-allocated tree (boxed children for
//! `OptionSome`, `BinOp`, `UnOp`, etc.; `Vec<CompiledExpr>` for `ListLiteral`
//! and similar), so the clone is O(literal-tree-size) per arg.
//!
//! ## Tests in this file
//!
//! - [`trait_arg_conformance_correctness_small`]: a small (N=4) **non-ignored** test
//!   that guards against regressions in the recursive `walk_param_against_arg`
//!   dispatcher and runs in every normal `cargo test` invocation.
//! - [`compile_large_literal_trait_arg_conformance_timing`]: an `#[ignore]`'d
//!   timing test that compiles at N=20 and N=200 and prints both elapsed
//!   durations so the per-element cost can be inferred by comparison.
//!
//! ## How to run the timing bench
//!
//! ```text
//! cargo test -p reify-compiler --test trait_arg_conformance_bench -- --ignored --nocapture
//! ```

use std::time::{Duration, Instant};

use reify_compiler::CompiledModule;
use reify_test_support::compile_source_with_stdlib;
use reify_core::Severity;

/// Builds the Reify source for a design that embeds `n` `some(Steel())` elements
/// in a `List<Option<MaterialSpec>>` arg.
fn make_source(n: usize) -> String {
    assert!(n > 0, "make_source requires n >= 1");
    let list_body = (0..n)
        .map(|_| "some(Steel())")
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        r#"
        structure def Steel : MaterialSpec {{
            param density : Real = 7850.0
            param name : String = "steel"
        }}
        structure def Host {{ param ms : List<Option<MaterialSpec>> }}
        structure def Top {{
            sub x = Host(ms: [{list_body}])
        }}
    "#
    )
}

/// Asserts that `module` contains no `Error`-severity diagnostics.
/// The `n` parameter provides context for the panic message.
fn assert_no_errors(module: &CompiledModule, n: usize) {
    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error-severity diagnostics for N={n} some(Steel()) elements, got: {errors:#?}",
    );
}

/// Correctness regression guard for the recursive `walk_param_against_arg`
/// dispatcher introduced in task 2227.  Runs on every `cargo test` invocation
/// (not `#[ignore]`'d).
///
/// Exercises the `Type::List → ListLiteral` and `Type::Option → OptionSome`
/// arms of `crates/reify-compiler/src/conformance/mod.rs`.  A regression in
/// either arm will cause `Error`-severity diagnostics and fail this test.
#[test]
fn trait_arg_conformance_correctness_small() {
    const N: usize = 4;
    let module = compile_source_with_stdlib(&make_source(N));
    assert_no_errors(&module, N);
}

/// Timing bench: compiles at N=20 and N=200 and prints both elapsed durations,
/// allowing the per-element clone cost to be inferred by comparison.
///
/// The `#[ignore]` attribute keeps wall-clock work out of normal `cargo test`
/// runs, matching the precedent set by
/// `crates/reify-lsp/tests/incremental_eval_benchmark.rs`.
/// Run with:
///   `cargo test -p reify-compiler --test trait_arg_conformance_bench -- --ignored --nocapture`
#[test]
#[ignore]
fn compile_large_literal_trait_arg_conformance_timing() {
    for &n in &[20_usize, 200_usize] {
        let source = make_source(n);
        let t = Instant::now();
        let module = compile_source_with_stdlib(&source);
        let elapsed = t.elapsed();
        eprintln!("[task-2280] compiled N={n} List<Option<MaterialSpec>> in {elapsed:?}");
        // Verify correctness at each size so a walker regression surfaces here too.
        assert_no_errors(&module, n);
        // Loose sanity assertion for the N=200 leg only: fire if compile time exceeds 30 s.
        // This is categorical — it only fires on a catastrophic (~100×) regression, not a
        // small ratio slowdown.  No ratio is pinned because exact timing varies by machine;
        // 30 s is generous enough for shared CI runners but catches anything truly broken.
        // Mirrors the pattern at crates/reify-lsp/tests/incremental_eval_benchmark.rs:124-132.
        if n == 200 {
            assert!(
                elapsed < Duration::from_secs(30),
                "[task-2293] catastrophic regression: N=200 trait_arg_conformance compile took \
                 {elapsed:?}, expected < 30 s",
            );
        }
    }
}
