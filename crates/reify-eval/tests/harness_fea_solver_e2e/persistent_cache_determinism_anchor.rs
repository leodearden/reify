//! Cross-session determinism anchor for the FEA persistent cache
//! (task #2980, PRD case 6).
//!
//! # What this pins
//!
//! PRD `docs/prds/v0_3/persistent-fea-cache.md` §"Determinism implication": a
//! warm cache must not merely make a re-run FASTER, it must make it the SAME
//! run. This is the load-bearing reproducibility guarantee of the whole feature
//! — if a cached FEA result differed in any bit from the one that was computed,
//! an auto-resolve search sitting on top of it could take a different branch and
//! land on a different design, and the cache would have silently changed the
//! engineering answer.
//!
//! The subject is deliberately the SHIPPED example from task #2930
//! (`examples/fea_bracket_minimize_mass.ri`) driven through a REAL
//! Nelder-Mead-over-FEA auto-resolve loop, read from disk rather than
//! `include_str!`-ed, so the thing under test is the artifact users actually run.
//!
//! # Why the counters are the assertion, not a proxy for it
//!
//! Session 2's `persistent_miss_count() == 0` is a direct trajectory-identity
//! claim, not a stand-in for one. Every distinct candidate thickness the
//! optimizer visits mints a distinct content-derived cache key, and the counters
//! move only for persistable compute targets (`solver::elastic_static` here). So
//! ANY divergence from session 1's trajectory would necessarily visit a key that
//! session 1 never wrote, and register a miss. Pairing it with
//! `persistent_hit_count() == s1_lookups` upgrades the claim from "session 2
//! never went anywhere new" to "session 2 walked exactly the same path, to the
//! same length" — without the pairing, a session 2 that stopped early would
//! also show zero misses.
//!
//! The bit-identity leg is then structurally guaranteed rather than hoped for:
//! session 2 performs zero solves, so every FEA value it consumes is a
//! deserialization of session 1's fixed bytes, and the optimizer arithmetic
//! above them is deterministic.
//!
//! # STATUS: this test does not pass today, and that is a FINDING (esc-2980-8)
//!
//! The module is deliberately NOT registered in `harness_fea_solver_e2e.rs`, so
//! nothing compiles or runs it. Registering it and running in release profile
//! fails the anti-vacuity leg below:
//!
//! ```text
//! session 1 must have visited at least 2 solver candidates ... got 0 lookups
//! (0 hits + 0 misses)
//! test result: FAILED. 0 passed; 1 failed; finished in 79.65s
//! ```
//!
//! Session 1 ran the full real search for 79.65 s and consulted the persistent
//! cache ZERO times. The auto-resolve constraint-solver cost loop reaches
//! `solver::elastic_static` through `OptimizedComputeDispatcher`
//! (`crates/reify-eval/src/engine_compute.rs`) — an OWNED SNAPSHOT whose entire
//! state is `fns: HashMap<&'static str, ComputeFn>`. It holds no `&Engine`, so
//! it has no `persistent_cache_dir` and no counters, and its
//! `impl ComputeDispatch` block contains zero occurrences of "persistent". The
//! persistent-cache hook lives only in the OTHER dispatch path
//! (engine_compute.rs:301-334). So the cache is bypassed entirely on the
//! auto-resolve path — the path whose reproducibility this test exists to
//! anchor.
//!
//! Measured end-to-end with the release binary:
//! `reify eval --cache-dir <tmp> examples/fea_bracket_minimize_mass.ri` exits 0
//! in 145 s and writes ZERO files under the cache root, while the same binary
//! and flag on non-auto-resolve inputs (`fea_cantilever_deterministic.ri`,
//! `examples/fea_cantilever_smoke.ri`) writes one `.bin` each.
//!
//! The assertions below are deliberately left at full strength rather than
//! weakened to match current behaviour: weakening them would retire PRD case 6
//! and enshrine the gap. Wiring the cache into the auto-resolve path is
//! production work in `crates/reify-eval/src/`, outside task #2980's test-only
//! scope. When it lands, register this module and the test should pass as
//! written.
//!
//! # Runtime
//!
//! Dominated by session 1's cold search — the same real loop that
//! `fea_bracket_minimize_mass_e2e.rs` measures at 66 s / 93 s / 105 s across
//! three runs on a contended 32-core host. Session 2 is all cache hits and adds
//! little. That is ~30x this harness's 2.25 s per-test mean, so the test is
//! evicted from the merge gate by its own test-scoped atom in
//! `scripts/heavy-test-filter-lib.sh` and is additionally
//! `debug_assertions`-gated to release-only.

use crate::fea_design_loop_support::fea_loop_engine;
use reify_core::ValueCellId;
use reify_ir::Value;
use reify_test_support::{collect_errors, compile_source_with_stdlib};

/// The shipped example under test, resolved from `CARGO_MANIFEST_DIR` so it
/// works in any worktree. Matches `fea_bracket_minimize_mass_e2e.rs:79-83`.
const EXAMPLE_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../examples/fea_bracket_minimize_mass.ri"
);

const TEMPLATE_NAME: &str = "FeaBracketMinimizeMass";

/// Case 6 — a warm cross-session cache replays the auto-resolve trajectory
/// identically, down to the bits of the resolved design.
#[cfg_attr(
    debug_assertions,
    ignore = "heavy FEA-in-the-loop design solve; release-only"
)]
#[test]
fn auto_resolve_trajectory_replays_identically_from_a_warm_cross_session_cache() {
    // ── (a) The shipped example, from disk, compiled clean ──────────────────

    let src = std::fs::read_to_string(EXAMPLE_PATH)
        .expect("failed to read examples/fea_bracket_minimize_mass.ri");
    let compiled = compile_source_with_stdlib(&src);
    let compile_errors = collect_errors(&compiled.diagnostics);
    assert!(
        compile_errors.is_empty(),
        "examples/fea_bracket_minimize_mass.ri must compile without Error \
         diagnostics: {compile_errors:#?}",
    );

    let cache = tempfile::TempDir::new().expect("cache tempdir must be creatable");

    /// Read the resolved `thickness` SI magnitude out of an eval result, or
    /// panic naming the cell.
    fn resolved_thickness(result: &reify_eval::EvalResult) -> f64 {
        match result
            .values
            .get(&ValueCellId::new(TEMPLATE_NAME, "thickness"))
        {
            Some(Value::Scalar { si_value, .. }) => *si_value,
            other => panic!(
                "{TEMPLATE_NAME}.thickness must resolve to a Scalar for the \
                 determinism anchor to mean anything, got: {other:?}"
            ),
        }
    }

    // ── (b) Session 1 — cold cache, a real search that populates it ─────────
    //
    // `set_persistent_cache_dir` is called BEFORE eval because the setter zeroes
    // both counters (engine_admin.rs:2537-2541); calling it afterwards would
    // erase exactly the statistics this test reads.

    let mut session1 = fea_loop_engine();
    session1.set_persistent_cache_dir(Some(cache.path().to_path_buf()));
    let result1 = session1.eval(&compiled);
    let eval_errors1 = collect_errors(&result1.diagnostics);
    assert!(
        eval_errors1.is_empty(),
        "session 1 must eval without Error diagnostics: {eval_errors1:#?}",
    );
    let thickness_s1 = resolved_thickness(&result1);

    let s1_hits = session1.persistent_hit_count();
    let s1_misses = session1.persistent_miss_count();
    let s1_lookups = s1_hits + s1_misses;

    // ANTI-VACUITY. Without this, a loop that collapsed to a single solve — or
    // one that never dispatched a persistable target at all — would sail through
    // the identity assertions below, which are trivially true over an empty or
    // one-element trajectory. This is the leg that makes the test falsifiable.
    assert!(
        s1_lookups >= 2,
        "session 1 must have visited at least 2 solver candidates for a \
         trajectory-identity claim to have content; got {s1_lookups} lookups \
         ({s1_hits} hits + {s1_misses} misses)",
    );

    // ── (c) Session 2 — a fresh engine over the SAME warm cache ─────────────

    let mut session2 = fea_loop_engine();
    session2.set_persistent_cache_dir(Some(cache.path().to_path_buf()));
    let result2 = session2.eval(&compiled);
    let eval_errors2 = collect_errors(&result2.diagnostics);
    assert!(
        eval_errors2.is_empty(),
        "session 2 must eval without Error diagnostics: {eval_errors2:#?}",
    );
    let thickness_s2 = resolved_thickness(&result2);

    // ── (d) THE TRAJECTORY-IDENTITY ASSERTION ───────────────────────────────

    assert_eq!(
        session2.persistent_miss_count(),
        0,
        "session 2 must never visit a cache key session 1 did not write — a \
         single miss means the warm run took a different branch through the \
         search space, i.e. the cache changed the answer. \
         session 1: {s1_hits} hits + {s1_misses} misses; \
         session 2: {} hits + {} misses",
        session2.persistent_hit_count(),
        session2.persistent_miss_count(),
    );
    assert_eq!(
        session2.persistent_hit_count(),
        s1_lookups,
        "session 2 must perform the SAME NUMBER of solver lookups as session 1 \
         ({s1_lookups}) — equal-and-zero misses alone would also be satisfied by \
         a run that stopped early, so this is what pins the trajectory's LENGTH \
         rather than merely a prefix of it",
    );

    // ── (e) Bit-identical resolved design ───────────────────────────────────

    assert_eq!(
        thickness_s2.to_bits(),
        thickness_s1.to_bits(),
        "the resolved thickness must be BIT-identical across sessions, not \
         merely close: session 1 = {thickness_s1:?}, session 2 = {thickness_s2:?}",
    );
}
