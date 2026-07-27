//! RED tests for cache_key population at ComputeNodeData construction sites
//! (task #3428 step-1).
//!
//! PRD §8-ι (docs/prds/v0_3/compute-node-contract.md): `compute_cache_key` is
//! the named consumer for `ComputeNode.cache_key`. These tests assert that the
//! 3 production ComputeNodeData construction sites in engine_eval.rs populate
//! `cache_key` with a real, input-content-addressed key instead of the
//! placeholder `ContentHash(0)`. Post-review (task #3428) the stored key is the
//! COMPLETE persistent key `Engine::persistent_cache_key` — `compute_cache_key`
//! folded with a hash of the evaluated `arg_values` so loads/supports/options
//! dropped by the shallow `value_inputs` walk still affect the key.
//!
//! Expected RED state (before step-2):
//! - `cache_key_populated_correctly_after_eval` FAILS because cache_key ==
//!   ContentHash(0) but compute_cache_key(&node, &graph) returns a non-zero hash.
//! - `cache_key_changes_when_input_changes` FAILS because both fixture variants
//!   produce ContentHash(0) regardless of the tip-load magnitude.
//!
//! GREEN after step-2: engine_eval.rs wires compute_cache_key at the 3 sites.

use reify_core::ContentHash;
use reify_eval::compute_cache_key;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// The cantilever smoke fixture — loaded at compile time so the test binary is
// always in sync with the user-facing example file (single-source-of-truth).
static CANTILEVER_SRC: &str = include_str!("../../../examples/fea_cantilever_smoke.ri");

/// Eval the cantilever fixture through the @optimized lowering path, returning
/// `(stored_cache_key, computed_cache_key)` for the `solver::elastic_static`
/// ComputeNode.
///
/// `stored_cache_key`  — `node.cache_key` as set by engine_eval.rs.
/// `computed_cache_key` — `compute_cache_key(&node, &graph)` (what it should be).
fn eval_and_extract_cache_keys(source: &str) -> (ContentHash, ContentHash) {
    let compiled = parse_and_compile_with_stdlib(source);
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let _result = engine.eval(&compiled);

    let state = engine
        .eval_state()
        .expect("eval_state must be Some after eval()");
    let snapshot = &state.snapshot;

    let (_, node_data) = snapshot
        .graph
        .compute_nodes
        .iter()
        .find(|(_, d)| d.target == "solver::elastic_static")
        .expect(
            "solver::elastic_static ComputeNode must exist in the graph after eval; \
             check that register_compute_fns is called and the fixture reaches the \
             @optimized lowering site",
        );

    let stored_key = node_data.cache_key;
    let computed_key = compute_cache_key(node_data, &snapshot.graph);
    (stored_key, computed_key)
}

// ── Assertion 1: main correctness check ──────────────────────────────────────

/// The stored `cache_key` must be non-zero AND must be the COMPLETE persistent
/// key — the structural `compute_cache_key(node, &graph)` COMBINED with a hash of
/// the evaluated `arg_values` (`Engine::persistent_cache_key`, task #3428 review
/// fix). It therefore intentionally DIFFERS from the bare `compute_cache_key`:
/// the fold adds the loads/supports/options that the shallow `value_inputs` walk
/// drops, so they can't cause a false persistent-cache hit.
///
/// RED (before step-2): fails because engine_eval.rs hardcodes
/// `cache_key: ContentHash(0)` at all 3 construction sites.
#[test]
fn cache_key_populated_correctly_after_eval() {
    let (stored_key, computed_key) = eval_and_extract_cache_keys(CANTILEVER_SRC);

    assert_ne!(
        stored_key,
        ContentHash(0),
        "cache_key must be non-zero after eval; ContentHash(0) placeholder found. \
         Step-2 must wire the cache key into the 3 engine_eval.rs sites."
    );
    assert_ne!(
        stored_key,
        computed_key,
        "stored cache_key must be the COMPLETE persistent key (persistent_cache_key: \
         compute_cache_key folded with a hash of the evaluated arg_values), so it must \
         NOT equal the bare compute_cache_key(&node, &graph). If they are equal the \
         arg_values fold has been dropped and loads/supports/options can false-hit. \
         stored={:?} computed={:?}",
        stored_key,
        computed_key,
    );
}

// ── Assertion 2: determinism ──────────────────────────────────────────────────

/// Two fresh engines evaluating the same source must produce the same `cache_key`
/// for the `solver::elastic_static` ComputeNode.
///
/// This assertion passes even in RED state (both engines return ContentHash(0)),
/// but only meaningfully pins determinism after step-2 when real keys are produced.
#[test]
fn cache_key_is_deterministic_across_fresh_engines() {
    let (stored_a, _) = eval_and_extract_cache_keys(CANTILEVER_SRC);
    let (stored_b, _) = eval_and_extract_cache_keys(CANTILEVER_SRC);

    assert_eq!(
        stored_a,
        stored_b,
        "two fresh engines evaluating the same source must produce the same cache_key; \
         engine_A={:?} vs engine_B={:?}",
        stored_a,
        stored_b,
    );
}

// ── Assertion 3: sensitivity to input changes ─────────────────────────────────

/// Changing a param that is a direct ValueRef input must change the cache_key.
/// `length` is passed directly as a ValueRef arg to solve_elastic_static and is
/// therefore captured in value_inputs; its content_hash encodes the default-expr,
/// so changing the default changes the cache_key.
///
/// (Note: `[tip_load]` is a list literal in the arg list, not a direct ValueRef,
/// so changing the tip_load let-binding does NOT affect value_inputs. We vary
/// `param length` instead.)
///
/// RED: fails because both variants produce ContentHash(0) — the cache_key is
/// not populated from the inputs at all.
///
/// GREEN after step-2: the key is input-content-addressed and reflects the param.
#[test]
fn cache_key_changes_when_input_changes() {
    // Default fixture: length = 1000mm.
    let (key_1m, _) = eval_and_extract_cache_keys(CANTILEVER_SRC);

    // Modified fixture: length = 2000mm (doubles beam length).
    // `length` is a param passed directly to solve_elastic_static as a ValueRef,
    // so its content_hash is captured in value_inputs and thus the cache key.
    let src_2m = CANTILEVER_SRC.replace(
        "param length : Length = 1000mm",
        "param length : Length = 2000mm",
    );
    let (key_2m, _) = eval_and_extract_cache_keys(&src_2m);

    assert_ne!(
        key_1m,
        key_2m,
        "changing `param length` from 1000mm to 2000mm must change the cache_key \
         (the `length` value cell's content_hash encodes the default-expr and is \
         captured in value_inputs); both produced identical keys: {:?}",
        key_1m,
    );
}

// ── Assertion 4: SOUNDNESS lock — load changes must change the key ────────────

/// Soundness regression lock (task #3428 review). Changing ONLY the tip-load
/// magnitude must change the persistent cache_key — even though `[tip_load]` is a
/// list-literal arg that the shallow `value_inputs` walk DROPS (see the note on
/// `cache_key_changes_when_input_changes`).
///
/// Before `Engine::persistent_cache_key` folded the evaluated `arg_values` into
/// the key, two solves differing only in load collided on the same key — a FALSE
/// persistent-cache HIT that would return the 1000 N stress/displacement for a
/// 2000 N solve. This test fails if that fold is ever removed.
#[test]
fn cache_key_changes_when_load_changes() {
    // Default fixture: tip load = 1000 N.
    let (key_1000n, _) = eval_and_extract_cache_keys(CANTILEVER_SRC);

    // Double the tip load. `[tip_load]` is a list-literal arg (NOT a direct
    // ValueRef), so this changes neither value_inputs nor the bare
    // compute_cache_key — only the evaluated arg_values folded into the key.
    let src_2000n = CANTILEVER_SRC.replace("force: 1000.0", "force: 2000.0");
    assert_ne!(
        CANTILEVER_SRC, src_2000n,
        "fixture must contain the literal `force: 1000.0` for this test to vary it",
    );
    let (key_2000n, _) = eval_and_extract_cache_keys(&src_2000n);

    assert_ne!(
        key_1000n,
        key_2000n,
        "changing the tip-load magnitude (1000 N -> 2000 N) MUST change the \
         persistent cache_key; otherwise two solves with different loads collide \
         and the cache returns a stale result. The arg_values fold in \
         persistent_cache_key closes this hole. both keys: {:?}",
        key_1000n,
    );
}
