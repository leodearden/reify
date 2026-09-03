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

use reify_core::{ContentHash, ValueCellId};
use reify_eval::compute_cache_key;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// The cantilever smoke fixture — loaded at compile time so the test binary is
// always in sync with the user-facing example file (single-source-of-truth).
static CANTILEVER_SRC: &str = include_str!("../../../../examples/fea_cantilever_smoke.ri");

/// What `eval_and_extract_node_facts` reads back off the evaluated
/// `solver::elastic_static` ComputeNode.
struct NodeFacts {
    /// `node.cache_key` as set by engine_eval.rs — the COMPLETE persistent key
    /// (`Engine::persistent_cache_key`), not the bare `compute_cache_key`.
    stored_key: ContentHash,
    /// `compute_cache_key(&node, &graph)` — the structural half only.
    computed_key: ContentHash,
    /// `node.value_inputs` verbatim, as emitted by the `@optimized` lowering.
    value_inputs: Vec<ValueCellId>,
}

/// Eval `source` through the @optimized lowering path and read the facts above
/// off the `solver::elastic_static` ComputeNode.
///
/// `make_simple_engine()` (no kernel) is sufficient: everything under test here
/// — the lowering, the `value_inputs` walk and the cache-key composition — runs
/// BEFORE the trampoline dispatches, so no real solve is needed.
fn eval_and_extract_node_facts(source: &str) -> NodeFacts {
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

    NodeFacts {
        stored_key: node_data.cache_key,
        computed_key: compute_cache_key(node_data, &snapshot.graph),
        value_inputs: node_data.value_inputs.clone(),
    }
}

/// Eval the cantilever fixture through the @optimized lowering path, returning
/// `(stored_cache_key, computed_cache_key)` for the `solver::elastic_static`
/// ComputeNode.
///
/// `stored_cache_key`  — `node.cache_key` as set by engine_eval.rs.
/// `computed_cache_key` — `compute_cache_key(&node, &graph)` (what it should be).
fn eval_and_extract_cache_keys(source: &str) -> (ContentHash, ContentHash) {
    let facts = eval_and_extract_node_facts(source);
    (facts.stored_key, facts.computed_key)
}

/// The reduced form of the task #6661 repro: one value cell reaching TWO
/// parameters of a single `@optimized` call.
///
/// This is `examples/fea_cantilever_smoke.ri` with exactly one substantive
/// edit — `height` occupies BOTH the `width` and the `height` slot of
/// `solve_elastic_static`, i.e. a square cross-section written the way an
/// author naturally writes one. It is the reduced form of the dogfood repro in
/// `prj/printer_v01/printer.ri:1962`
/// (`solve_elastic_static(material_static, half_span, h_eq, h_eq2, ...)`),
/// which had to introduce a `let h_eq2 = h_eq * 1.0` alias solely to dodge the
/// duplicate-`ValueCellId` abort this file now pins against.
const DUP_CELL_SRC: &str = r#"
structure FeaDupCellSmoke {
    // 1 m long beam with a 100 mm SQUARE cross-section — one `height` cell
    // deliberately supplies both section dimensions.
    param length : Length = 1000mm
    param height : Length = 100mm

    let material = Steel_AISI_1045()
    let tip_load = PointLoad(point: "tip", force: 1000.0)
    let mount = FixedSupport(target: "root")

    // `height` in BOTH the width and the height slot: the legal authoring
    // shape that task #6661 is about.
    let result = solve_elastic_static(
        material, length, height, height, [tip_load], [mount], ElasticOptions()
    )
}
"#;

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


// ── Assertion 5: the lowering emits a duplicate-free dependency SET ───────────

/// Task #6661. Passing ONE value cell to TWO parameters of a single `@optimized`
/// call is a legal authoring shape (square cross-section, symmetric span). The
/// `@optimized` lowering must therefore emit `value_inputs` as a genuine
/// dependency SET, mirroring what its sibling
/// `Engine::build_compute_realization_inputs` has always done for
/// `realization_inputs`.
///
/// Two independent guarantees are asserted here:
///
/// 1. **No abort.** Merely reaching this assertion proves `reify eval` no longer
///    kills the process on `DUP_CELL_SRC` — the reported symptom. This half is a
///    permanent regression guard even after the lowering is fixed.
/// 2. **Duplicate-free.** `value_inputs` carries each `ValueCellId` at most
///    once. RED until the lowering dedupes: `height` is pushed twice today.
#[test]
fn duplicate_value_cell_arg_lowers_to_a_duplicate_free_value_inputs_set() {
    let facts = eval_and_extract_node_facts(DUP_CELL_SRC);

    let mut sorted = facts.value_inputs.clone();
    sorted.sort();
    assert!(
        sorted.windows(2).all(|w| w[0] != w[1]),
        "the @optimized lowering must emit value_inputs as a duplicate-free \
         dependency set even when one value cell is passed to two parameters \
         (task #6661); got {:?}",
        facts.value_inputs,
    );

    assert_ne!(
        facts.stored_key,
        ContentHash(0),
        "the duplicate-cell design must still receive a real, input-content-addressed \
         cache_key, not the ContentHash(0) placeholder",
    );
}

// ── Assertion 6: POSITION survives the dedupe ────────────────────────────────

/// Companion guard to the dedupe above, and expected GREEN on arrival. It pins
/// task #6661's "the key must still distinguish POSITION" clause: collapsing
/// duplicates in the value bucket must not cost the stored key its ability to
/// tell `f(.., width, height, ..)` from `f(.., height, width, ..)`.
///
/// The signal deliberately does NOT come from the value bucket, which
/// `compute_cache_key_is_invariant_under_value_input_reordering` pins as
/// order-blind BY DESIGN (both variants below yield the identical
/// `{length, width, height}` set). It comes from `Engine::persistent_cache_key`,
/// which folds `combine_all` over the ORDERED evaluated `arg_values`; because
/// `ContentHash::combine` is order-dependent, swapping two differently-valued
/// args changes the stored key. A future refactor that drops that fold — or that
/// "restores" multiplicity to the value bucket to recover position — fails here.
#[test]
fn stored_cache_key_still_distinguishes_argument_position() {
    // Make width and height DIFFER, so swapping them is observable at all.
    let src_wh = CANTILEVER_SRC.replace(
        "param height : Length = 100mm",
        "param height : Length = 50mm",
    );
    assert_ne!(
        CANTILEVER_SRC, src_wh,
        "fixture must contain `param height : Length = 100mm` for this test to vary it",
    );

    // Same design, same cells, only the ARG ORDER differs.
    let src_hw = src_wh.replace(
        "material, length, width, height, [tip_load]",
        "material, length, height, width, [tip_load]",
    );
    assert_ne!(
        src_wh, src_hw,
        "fixture must contain the literal arg list `material, length, width, height, \
         [tip_load]` for this test to reorder it",
    );

    let facts_wh = eval_and_extract_node_facts(&src_wh);
    let facts_hw = eval_and_extract_node_facts(&src_hw);

    assert_ne!(
        facts_wh.stored_key, facts_hw.stored_key,
        "swapping two differently-valued args (width=100mm, height=50mm) must change the \
         STORED cache_key — position lives in persistent_cache_key's ordered arg_values \
         fold. both keys: {:?}",
        facts_wh.stored_key,
    );

    // And document that the bare structural key is the part that is (correctly)
    // position-blind: the two variants reference the identical cell SET.
    assert_eq!(
        facts_wh.computed_key, facts_hw.computed_key,
        "the bare compute_cache_key is order-invariant by design (3503); if this ever \
         differs, the value bucket has grown a position signal it is not supposed to have",
    );
}
