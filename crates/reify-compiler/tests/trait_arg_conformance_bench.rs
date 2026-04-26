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
//! This bench constructs a `List<Option<MaterialSpec>>` arg whose list literal
//! contains `N` `some(Steel())` elements and measures how long a single
//! `compile_source_with_stdlib` call takes.  The result is printed via
//! `eprintln!` for manual inspection; no ratio-assertion is made (the timing is
//! inherently sensitive to the host machine and CI runner load).
//!
//! The `#[ignore]` attribute keeps wall-clock work out of normal `cargo test`
//! runs, matching the precedent set by
//! `crates/reify-lsp/tests/incremental_eval_benchmark.rs`.
//!
//! ## Correctness double-duty
//!
//! The bench also asserts that compilation produces **zero `Error`-severity
//! diagnostics**.  This makes it a regression guard for the recursive
//! `walk_param_against_arg` dispatcher introduced in task 2227: if a bug is
//! introduced in the `Type::List → ListLiteral` or
//! `Type::Option → OptionSome` arms of
//! `crates/reify-compiler/src/conformance/mod.rs`, the assertion fires even
//! on a non-timing run.
//!
//! ## How to run
//!
//! ```text
//! cargo test -p reify-compiler --test trait_arg_conformance_bench -- --ignored --nocapture
//! ```
//!
//! Adjust `N` in the test body to dial the workload up or down.

use std::time::Instant;

use reify_test_support::compile_source_with_stdlib;
use reify_types::Severity;

/// Timing bench: compile a design with `N` nested `some(Steel())` elements
/// inside a `List<Option<MaterialSpec>>` arg.
///
/// The bench exercises the full per-arg `compiled_arg.clone()` path at
/// `entity.rs:949` and the post-pass conformance walker at
/// `conformance/mod.rs:132`.  Timing is printed via `eprintln!`; the only
/// hard assertion is that compilation produces no `Error`-severity diagnostics.
///
/// Ignored in normal CI runs — run with `cargo test ... -- --ignored --nocapture`.
#[test]
#[ignore]
fn compile_large_literal_trait_arg_conformance_timing() {
    const N: usize = 200;

    // Build N copies of "some(Steel()), " to form the list literal body.
    let list_elements = "some(Steel()), ".repeat(N);
    // Strip the trailing ", " for clean syntax.
    let list_body = list_elements.trim_end_matches(", ");

    let source = format!(
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
    );

    let t = Instant::now();
    let module = compile_source_with_stdlib(&source);
    let elapsed = t.elapsed();

    eprintln!("[task-2280] compiled N={N} List<Option<MaterialSpec>> in {elapsed:?}");

    let errors: Vec<_> = module
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error-severity diagnostics for N={N} some(Steel()) elements, got: {errors:#?}",
    );
}
