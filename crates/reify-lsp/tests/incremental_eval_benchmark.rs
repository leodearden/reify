//! Incremental evaluation benchmark tests.
//! Re-establishes the deliverable from task #479 that was lost when commit 00a86da53
//! was reverted off the trunk. Verifies content_hash determinism and engine state
//! across calls to compute_diagnostics_with_state, with an ignored timing test.

use reify_lsp::diagnostics::{EvalState, compute_diagnostics_with_state};
use tower_lsp::lsp_types::Url;

fn test_uri() -> Url {
    Url::parse("file:///bracket.ri").unwrap()
}

/// Content hash is stable across two identical calls to compute_diagnostics_with_state.
#[test]
fn content_hash_preserved_across_identical_evals() {
    let uri = test_uri();
    let source = reify_test_support::bracket_source();
    let mut state = EvalState::new();

    assert_eq!(
        state.last_content_hash(),
        None,
        "fresh EvalState must have no last_content_hash"
    );

    compute_diagnostics_with_state(&mut state, source, &uri);
    let h1 = state
        .last_content_hash()
        .expect("last_content_hash must be Some after first call");

    compute_diagnostics_with_state(&mut state, source, &uri);
    let h2 = state
        .last_content_hash()
        .expect("last_content_hash must be Some after second call");

    assert_eq!(
        h1, h2,
        "identical source must produce the same content_hash"
    );
}

/// Engine is initialized after a single eval call.
#[test]
fn engine_initialized_after_eval() {
    let uri = test_uri();
    let source = reify_test_support::bracket_source();
    let mut state = EvalState::new();

    assert!(
        !state.is_engine_initialized(),
        "fresh EvalState engine must not be initialized"
    );

    compute_diagnostics_with_state(&mut state, source, &uri);

    assert!(
        state.is_engine_initialized(),
        "engine must be initialized after eval"
    );
}

/// Content hash differs when source changes structure (bracket_source_violating has
/// a different thickness param value, producing a different CompiledModule hash).
#[test]
fn content_hash_changes_for_different_source() {
    let uri = test_uri();
    let source_valid = reify_test_support::bracket_source();
    let source_violating = reify_test_support::bracket_source_violating();
    let mut state = EvalState::new();

    compute_diagnostics_with_state(&mut state, source_valid, &uri);
    let h1 = state
        .last_content_hash()
        .expect("hash after valid source must be Some");

    compute_diagnostics_with_state(&mut state, &source_violating, &uri);
    let h2 = state
        .last_content_hash()
        .expect("hash after violating source must be Some");

    assert_ne!(
        h1, h2,
        "different source must produce a different content_hash"
    );
}

/// Timing baseline: 10 cold-start evals vs 10 incremental eval_cached calls.
/// Asserts the relative-ordering invariant: the eval_cached path must be strictly faster
/// than 10 cold starts. This catches silent regressions where eval_cached degenerates
/// into a full cold-start without any correctness failure.
/// Ignored in normal CI runs to avoid flakiness — run with `cargo test -- --ignored`.
#[test]
#[ignore = "timing benchmark; flaky under CI load - run explicitly with --ignored"]
fn timing_cold_start_vs_incremental_baseline() {
    use std::time::Instant;

    let uri = test_uri();
    let source = reify_test_support::bracket_source();

    // Cold-start baseline: 10 fresh states, each doing one eval
    let cold_start = {
        let start = Instant::now();
        for _ in 0..10 {
            let mut state = EvalState::new();
            compute_diagnostics_with_state(&mut state, source, &uri);
        }
        start.elapsed()
    };

    // Warm incremental baseline: one state, 10 identical-source calls (eval_cached after first)
    let incremental = {
        let mut state = EvalState::new();
        // Prime the engine with a cold-start
        compute_diagnostics_with_state(&mut state, source, &uri);
        let start = Instant::now();
        for _ in 0..10 {
            compute_diagnostics_with_state(&mut state, source, &uri);
        }
        start.elapsed()
    };

    eprintln!(
        "[timing] 10× cold-start: {:?}  |  10× incremental (eval_cached): {:?}",
        cold_start, incremental
    );

    // Loose sanity assertion: the eval_cached path must be strictly faster than 10 cold starts.
    // This is categorical — it only fires if the cache-hit path degenerates to cold-start speed,
    // not if it is merely "slower than expected by some ratio." No ratio is pinned because the
    // exact speedup varies by machine; the ordering is what matters.
    //
    // The test remains #[ignore]d to keep wall-clock work out of normal CI runs (timing is
    // inherently flaky on shared runners). Run with:
    //   cargo test -p reify-lsp -- --ignored --nocapture
    assert!(
        incremental < cold_start,
        "incremental eval_cached path must be faster than 10× cold-start; \
         if not, eval_cached has likely degenerated into a cold-start. \
         cold_start={cold_start:?}, incremental={incremental:?}",
    );
}
