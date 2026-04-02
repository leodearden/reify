//! Incremental evaluation benchmark tests.
//!
//! Verifies content_hash preservation and engine state across calls to
//! compute_diagnostics_with_state.

use tower_lsp::lsp_types::Url;

use reify_lsp::diagnostics::{EvalState, compute_diagnostics_with_state};

fn test_uri() -> Url {
    Url::parse("file:///benchmark.ri").unwrap()
}

/// Calling compute_diagnostics_with_state twice with identical source
/// produces the same content_hash — compilation is deterministic.
#[test]
fn content_hash_preserved_across_identical_evals() {
    let source = reify_test_support::bracket_source();
    let mut state = EvalState::new();
    assert!(
        state.last_content_hash().is_none(),
        "no hash before first eval"
    );

    compute_diagnostics_with_state(&mut state, source, &test_uri());
    let hash1 = state.last_content_hash().expect("hash after first eval");

    compute_diagnostics_with_state(&mut state, source, &test_uri());
    let hash2 = state.last_content_hash().expect("hash after second eval");

    assert_eq!(
        hash1, hash2,
        "content_hash should be identical for identical source"
    );
}

/// After evaluation, the engine is initialized (eval_state is Some).
#[test]
fn engine_initialized_after_eval() {
    let source = reify_test_support::bracket_source();
    let mut state = EvalState::new();

    compute_diagnostics_with_state(&mut state, source, &test_uri());
    assert!(
        state.is_engine_initialized(),
        "engine should be initialized after eval"
    );
}

/// Timing baseline: cold-start eval. Gated behind #[ignore] to avoid
/// flaky CI. Run manually with `cargo test -p reify-lsp -- --ignored`.
#[test]
#[ignore]
fn timing_cold_start_baseline() {
    use std::time::Instant;
    let source = reify_test_support::bracket_source();
    let mut state = EvalState::new();

    let start = Instant::now();
    for _ in 0..10 {
        compute_diagnostics_with_state(&mut state, source, &test_uri());
    }
    let elapsed = start.elapsed();
    eprintln!(
        "10 cold-start evals: {:?} ({:?}/eval)",
        elapsed,
        elapsed / 10
    );
    // No hard assertion — this is a baseline measurement for comparison
    // with future incremental eval (task 480).
}
