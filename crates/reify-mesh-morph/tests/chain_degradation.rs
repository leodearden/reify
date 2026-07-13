//! PRD `docs/prds/v0_3/mesh-morphing.md` task #14 — mesh-morph
//! chain-degradation bounds test (50+ tick parameter sweep).
//!
//! Empirically verifies claim (c) of the "morph-source =
//! from-most-recent-in-memory only" policy: for elasticity morphing, chain
//! degradation is BOUNDED — each morph is a fresh BVP, not an iterative
//! perturbation — so a long monotonic auto-resolve chain stays within the
//! quality envelope without periodic remeshes, and a non-monotonic
//! (not-nearest) chain is caught by the quality check and self-recovers via
//! remesh-fallback.
//!
//! ## Algorithm-level chain harness (NOT the real engine)
//!
//! [`runner::run_chain`] models the engine's morph-source policy explicitly
//! over the task #13 calibration fixtures (`calibration/fixtures.rs`) and
//! the `elasticity_morph`/`quality_check` primitives — deliberately not
//! driving the real `Engine`, whose morph-arm e2e needs OCCT+gmsh and
//! SIGSEGVs on OCCT surfaces (esc-4744-89/96), hence `#[ignore]`'d in CI.
//! This isolates PRD claim (c) — a property of `elasticity_morph` itself —
//! from that fragile path, and is fully deterministic. See `runner`'s module
//! docs for the exact per-tick decision tree.
//!
//! Helper modules are pulled in via `#[path = …]` so Cargo does NOT compile
//! them as standalone integration-test binaries — only this file is (same
//! discipline as `tests/calibration.rs`).
//!
//! Provenance: task #2951 (PRD task #14).

#[path = "calibration/fixtures.rs"]
mod fixtures;

#[path = "chain/runner.rs"]
mod runner;

use std::sync::OnceLock;

// ── Step-1: chain-runner smoke on a short monotonic plate chain ──────────────

#[test]
fn chain_runner_smoke_on_short_monotonic_plate_chain_accepts_all_ticks() {
    // Same plate fixture + relaxed-pct options profile as the task #13
    // calibration sweeps: outer=1.0, thickness=0.1, n_radial=4, n_through=2,
    // hole_diameter is the swept parameter.
    let fixture = |hole_diameter: f64| fixtures::plate_with_hole(1.0, hole_diameter, 0.1, 4, 2);
    let params = [0.30_f64, 0.304, 0.308, 0.312];
    // Relaxed pct floor only — mirrors `calibration_sweep_options()` in
    // tests/calibration.rs (see that fn's doc for the fixture-specific
    // rationale). min_sj/AR stay at production defaults. Reuses the shared
    // `chain_options()` helper (task 2951 amendment, reviewer_comprehensive
    // finding #1) instead of a hand-rolled literal, so this profile can't
    // silently drift from the other chain tests.
    let opts = chain_options();

    let report = runner::run_chain(fixture, &params, &opts);

    // (a) One TickRecord per param.
    assert_eq!(
        report.ticks.len(),
        params.len(),
        "run_chain must produce one TickRecord per param, got {} ticks for {} params",
        report.ticks.len(),
        params.len()
    );

    // (b) Every tick's ACTUAL in-use mesh shares connectivity with the root
    // ("morph preserves connectivity"). `TickRecord::tet_indices_len` (task
    // 2951 amendment, reviewer_comprehensive finding #1) is recorded by
    // `run_chain` directly off the mesh it actually carries forward into the
    // next tick — the accepted `elasticity_morph` candidate on a
    // `fell_back == false` tick, or the fresh remesh on a `fell_back ==
    // true` tick — so asserting on it is a direct check of the morphed mesh
    // itself. Previously this re-derived `fixture(tick.param)` and checked
    // THAT mesh's tet_indices length, which only proved the FIXTURE's own
    // connectivity invariance and could not catch a regression where
    // `elasticity_morph` altered tet_indices length while the fixture
    // stayed connectivity-invariant.
    let root_tet_len = report.ticks[0].tet_indices_len;
    for tick in &report.ticks {
        assert_eq!(
            tick.tet_indices_len,
            root_tet_len,
            "tick param={}: in-use mesh's tet_indices length must match the root's \
             (connectivity must be preserved along the chain)",
            tick.param
        );
    }

    // (c) Every tick's min_scaled_j is finite and > 0.
    for tick in &report.ticks {
        assert!(
            tick.min_scaled_j.is_finite() && tick.min_scaled_j > 0.0,
            "tick param={}: min_scaled_j must be finite and > 0, got {}",
            tick.param,
            tick.min_scaled_j
        );
    }

    // (d) Tick 0 is the fresh root; ticks ≥1 with these tiny steps are
    // accepted (fell_back == false).
    assert!(
        !report.ticks[0].fell_back,
        "tick 0 (chain root) must not be marked fell_back"
    );
    for tick in &report.ticks[1..] {
        assert!(
            !tick.fell_back,
            "tiny monotonic step at param={} should be accepted (fell_back=false), \
             chain self-reported fell_back=true",
            tick.param
        );
    }
}

// ── Step-3: fallback + self-recovery + summary-helper behavior ───────────────

#[test]
fn chain_runner_large_jump_triggers_fallback_and_chain_self_recovers() {
    // Same plate fixture as the step-1 smoke test, but with a deliberate
    // large jump sandwiched between two other steps, so this short 4-tick
    // chain is fully hand-enumerable ("known"):
    //   tick 0 (0.30):  fresh chain root.
    //   tick 1 (0.302): tiny step from tick 0 — expected accepted.
    //   tick 2 (0.60):  large jump from tick 1 — expected quality-rejected
    //                   (mirrors calibration.rs's pinned plate 0.30→0.60
    //                   SoftFail finding, task #3451/#2950) — fallback resets
    //                   the chain to a fresh remesh.
    //   tick 3 (0.50):  retreat from tick 2's FRESH mesh — expected accepted
    //                   (self-recovery: morphing resumes after the reset).
    //
    // Tick 3 is NOT a tiny (~0.002) step like tick 1: `pct_below_025` (the
    // relaxed-floor metric) is an ABSOLUTE property of a mesh's own
    // hole_diameter — empirically it saturates to exactly 1.0 (i.e.
    // permanently SoftFails against the 0.99 relaxed floor, regardless of
    // morph magnitude) for hole_diameter >= ~0.53, and sits at <=0.98 below
    // that. Tick 2's fresh fallback mesh (0.60) sits deep in the saturated
    // zone, so ANY small step from it (e.g. 0.602 or 0.598) lands in that
    // same zone and would ALSO SoftFail — that would test the permanently-
    // stuck edge of the fixture's intrinsic quality envelope, not the
    // "morph-source" chain policy's self-recovery this step targets. A
    // retreat to 0.50 clears the saturation boundary with comfortable
    // margin (confirmed empirically: 0.60→0.50 is `Pass` with
    // `morph_max_ar_factor` ≈ 1.004, nowhere near the 2.0 threshold),
    // cleanly demonstrating that the chain resumes accepting morphs once
    // it steps back into the fixture's normal quality regime.
    let fixture = |hole_diameter: f64| fixtures::plate_with_hole(1.0, hole_diameter, 0.1, 4, 2);
    let params = [0.30_f64, 0.302, 0.60, 0.50];
    // Relaxed pct floor only — same profile as the step-1 smoke test (see
    // that test's comment for the fixture-specific rationale). Reuses the
    // shared `chain_options()` helper (task 2951 amendment,
    // reviewer_comprehensive finding #1) instead of a hand-rolled literal.
    let opts = chain_options();

    let report = runner::run_chain(fixture, &params, &opts);
    assert_eq!(report.ticks.len(), params.len());

    // Sanity: tick 1's tiny step is accepted — pins this chain's ONLY
    // fallback to the deliberate large jump at index 2, which the (d)
    // summary-helper assertions below depend on.
    assert!(
        !report.ticks[1].fell_back,
        "tiny step 0.30 -> 0.302 (index 1) should be accepted, not fall back"
    );

    // (a) The large-jump tick (index 2, 0.302 -> 0.60) falls back — the
    // quality check catches the not-nearest morph.
    assert!(
        report.ticks[2].fell_back,
        "large jump 0.302 -> 0.60 (index 2) should be quality-rejected and \
         fall back to a fresh remesh"
    );

    // (b) The post-fallback mesh IS the fresh remesh: its min_scaled_j
    // equals from_scratch_min_scaled_j (within f64 slop).
    assert!(
        (report.ticks[2].min_scaled_j - report.ticks[2].from_scratch_min_scaled_j).abs() < 1e-9,
        "fallback tick's min_scaled_j ({}) must equal its from_scratch_min_scaled_j \
         ({}) — the fallback mesh IS the fresh remesh",
        report.ticks[2].min_scaled_j,
        report.ticks[2].from_scratch_min_scaled_j
    );

    // (c) The following retreat step (index 3, morphing from tick 2's FRESH
    // mesh back down to 0.50 — see the params comment above for why this
    // isn't a tiny step) is accepted — the chain resumes morphing after the
    // reset (self-recovery).
    assert!(
        !report.ticks[3].fell_back,
        "retreat step 0.60 -> 0.50 (index 3, morphing from the post-fallback \
         fresh mesh) should be accepted (self-recovery after the reset)"
    );

    // (d) Summary helpers return the expected values for this known,
    // fully-enumerated 4-tick chain: exactly 1 fallback (index 2) among the
    // 3 morph ticks (indices 1..=3, i.e. i>=1).
    assert_eq!(
        report.fallback_count(),
        1,
        "expected exactly 1 fallback (index 2) in this known chain, got {}",
        report.fallback_count()
    );
    assert_eq!(
        report.fallback_rate(),
        1.0 / 3.0,
        "expected fallback_rate 1/3 (1 fallback / 3 morph ticks), got {}",
        report.fallback_rate()
    );
    // min_scaled_j_floor() must equal the minimum min_scaled_j recorded
    // across every tick (root included) — checked independently of the
    // physical mesh-quality magnitude, which calibration.rs (task #3451)
    // separately pins per fixture parameter.
    let expected_floor = report
        .ticks
        .iter()
        .map(|t| t.min_scaled_j)
        .fold(f64::INFINITY, f64::min);
    assert_eq!(
        report.min_scaled_j_floor(),
        expected_floor,
        "min_scaled_j_floor() must equal the minimum min_scaled_j across all ticks"
    );
    eprintln!(
        "[chain-degradation step-3 known-chain] fallback_count={} fallback_rate={:.4} \
         min_scaled_j_floor={:.6}",
        report.fallback_count(),
        report.fallback_rate(),
        report.min_scaled_j_floor()
    );
}

// ── Step-5/6: monotonic 50+ tick bounded-degradation ──────────────────────────
//
// The core verification of PRD claim (c): for elasticity morphing, chain
// degradation is BOUNDED — each morph is a fresh BVP, not an iterative
// perturbation — so a long monotonic auto-resolve chain stays within the
// quality envelope without periodic remeshes.

/// Maximum acceptable [`runner::ChainReport::fallback_rate`] for the
/// monotonic chain sweep.
///
/// This is the LOAD-BEARING bound for PRD claim (c): a long monotonic
/// auto-resolve chain should rarely need a remesh reset. Empirically, tiny
/// consecutive steps within this fixture's safe quality regime (see
/// [`CHAIN_MIN_SCALED_J_FLOOR`]'s doc and the step-3 test's doc for the
/// ~0.53 pct-saturation boundary the [0.30, 0.50] sweep range stays clear
/// of) are accepted essentially every tick, so 0.05 (at most ~2 resets in
/// 50+ ticks) leaves comfortable margin while still being a meaningful
/// regression guard — a future change that made elasticity_morph or
/// quality_check materially stricter would show up here as a rate
/// exceeding this bound.
///
/// Brittleness note (reviewer_comprehensive test-robustness finding,
/// amendment round 2): this bound and the [`lcg_jumps`] seed-8 sequence's
/// documented fallback count (see that fn's doc) are empirical pins against
/// the CURRENT `elasticity_morph`/`quality_check` behavior, not a
/// mathematical guarantee. A future morph/quality-check tuning change could
/// flip a monotonic tick to a fallback (breaking this bound) or shift the
/// LCG sequence so its first fallback lands on the final tick (an explicit
/// guard below turns that into a clear failure message rather than a
/// misleading one). Re-run and re-pin both if you change morph-kernel or
/// quality-check internals.
const CHAIN_FALLBACK_RATE_MAX: f64 = 0.05;

/// Documented minimum-scaled-Jacobian floor for the chain sweep tests.
///
/// This is **0.01** (the production `quality_floor_min_scaled_jacobian`
/// default), **not** the task description's illustrative "e.g. > 0.10":
/// the calibration fixtures' intrinsic from-scratch min scaled-J is
/// ≈0.014–0.024 across the plate's swept range (task #3451 empirical
/// pins in `calibration.rs`) — even a **fresh remesh** sits far below
/// 0.10, so 0.10 would be an un-GREENable bound for this fixture, not a
/// signal of morph degradation. 0.01 is achievable and gate-enforced:
/// `quality_check`'s `Pass` verdict requires
/// `global_min_scaled_j >= options.quality_floor_min_scaled_jacobian`
/// (0.01, kept at production default by [`chain_options`] — only the pct
/// floor is relaxed), so every ACCEPTED morph clears 0.01 by construction,
/// and every fresh fallback mesh clears its own from-scratch baseline
/// (>= ~0.017 across this sweep's [0.30, 0.50] range). The load-bearing
/// bounded-degradation signal is the LOW fallback rate sustained over 50+
/// ticks ([`CHAIN_FALLBACK_RATE_MAX`]), not the absolute min-scaled-J
/// value pinned here.
const CHAIN_MIN_SCALED_J_FLOOR: f64 = 0.01;

/// Chain-options profile shared by the monotonic/non-monotonic sweep tests:
/// relaxes `quality_floor_pct_below_025` to 0.99; `quality_floor_min_scaled_jacobian`
/// (0.01) and `quality_aspect_ratio_factor_max` (2.0) stay at production
/// defaults.
///
/// Mirrors `calibration.rs::calibration_sweep_options` — the procedural
/// hex-to-6-tet plate/bracket fixtures have an intrinsic from-scratch
/// `pct_below_025` that rises with the swept parameter (empirically 0.875
/// at plate hole_diameter=0.30 up to a hard saturation at exactly 1.0 for
/// hole_diameter >= ~0.53 — see the step-3 test's doc), so the production
/// 0.01 pct floor would SoftFail on every tick regardless of morph
/// quality, destroying the bounded-degradation signal this harness exists
/// to measure. This is a fixture-specific calibration shim, not a general
/// relaxation — see `calibration_sweep_options`'s doc for the "when NOT to
/// reuse" caveat, which applies identically here.
fn chain_options() -> reify_mesh_morph::MorphOptions {
    reify_mesh_morph::MorphOptions {
        quality_floor_pct_below_025: 0.99,
        ..reify_mesh_morph::MorphOptions::default()
    }
}

/// `n` evenly spaced values from `lo` to `hi` inclusive (`n >= 2`).
///
/// `linspace(0.30, 0.50, 51)` yields `0.30, 0.304, 0.308, ..., 0.50`
/// (Δ ≈ 0.004, 51 values ⇒ 50 morph ticks after the fresh chain root).
fn linspace(lo: f64, hi: f64, n: usize) -> Vec<f64> {
    assert!(n >= 2, "linspace requires n >= 2, got {n}");
    let step = (hi - lo) / (n - 1) as f64;
    (0..n).map(|i| lo + step * i as f64).collect()
}

/// Shared 51-value monotonic-sweep [`runner::ChainReport`] (params
/// `linspace(0.30, 0.50, 51)`, [`chain_options`]) — memoized so it is
/// computed exactly once regardless of which test asks for it first.
///
/// Both the monotonic test (step-6) and the non-monotonic test's comparison
/// baseline (step-8) want the identical sweep. The monotonic sweep is ~50
/// `elasticity_morph` FEA solves — the single largest compute cost in this
/// file (reviewer_comprehensive efficiency finding, amendment round 2) — so
/// computing it once per test (as the non-monotonic test previously did
/// inline) roughly doubled the file's total solve count for no benefit.
/// `OnceLock::get_or_init` is synchronized, so this is safe even though
/// `cargo test` runs `#[test]` fns concurrently on separate threads within
/// this one test binary: whichever test calls this first performs the
/// sweep, the other blocks briefly then reuses the cached result.
/// `run_chain` is a pure function of `(fixture, params, opts)`, so the
/// shared result is identical to what each test would have computed on its
/// own. Deliberately still a real computed `ChainReport` (not a hardcoded
/// constant) so the non-monotonic test's assertion (b) stays honest against
/// any future change to `elasticity_morph`/`quality_check` that shifts the
/// true monotonic rate.
static MONOTONIC_SWEEP_REPORT: OnceLock<runner::ChainReport> = OnceLock::new();

/// Returns the shared monotonic-sweep report, computing it on first call.
/// See [`MONOTONIC_SWEEP_REPORT`]'s doc.
fn monotonic_sweep_report() -> &'static runner::ChainReport {
    MONOTONIC_SWEEP_REPORT.get_or_init(|| {
        let fixture = |hole_diameter: f64| fixtures::plate_with_hole(1.0, hole_diameter, 0.1, 4, 2);
        let params = linspace(0.30, 0.50, 51);
        let opts = chain_options();
        runner::run_chain(fixture, &params, &opts)
    })
}

/// PRD `docs/prds/v0_3/mesh-morphing.md` task #14, claim (c): a long
/// monotonic auto-resolve chain stays within the quality envelope without
/// periodic remeshes.
///
/// Sweeps `plate_with_hole`'s `hole_diameter` over 51 evenly-spaced values
/// in `[0.30, 0.50]` (50 morph ticks after the fresh chain root) — a range
/// chosen (see [`CHAIN_MIN_SCALED_J_FLOOR`] / [`chain_options`] docs) so
/// the fixture's intrinsic from-scratch quality stays comfortably inside
/// the chain-options quality envelope at every tick, isolating the
/// bounded-degradation property of `elasticity_morph` itself (PRD claim
/// (c)) from the fixture's own pct-saturation edge (step-3's test doc).
#[test]
fn monotonic_plate_sweep_50_ticks_keeps_chain_within_quality_envelope() {
    // Shared with the non-monotonic test's comparison baseline (step-8) via
    // `monotonic_sweep_report()`'s `OnceLock` memoization — see that fn's
    // doc for why sharing the computed sweep across both tests is safe.
    let report = monotonic_sweep_report();

    // eprintln! summary so the perf-tracked CI log surfaces the trend
    // (task instruction: "so the perf-tracked CI log surfaces the trend").
    eprintln!(
        "[chain-degradation monotonic-50] ticks={} fallback_count={} fallback_rate={:.4} \
         min_scaled_j_floor={:.6}",
        report.ticks.len(),
        report.fallback_count(),
        report.fallback_rate(),
        report.min_scaled_j_floor()
    );

    // (a) The chain rarely needs a remesh reset — degradation is bounded.
    assert!(
        report.fallback_rate() < CHAIN_FALLBACK_RATE_MAX,
        "monotonic 50-tick plate sweep: fallback_rate={} must be < {} \
         (bounded degradation — a long monotonic chain should not need \
         frequent remesh resets)",
        report.fallback_rate(),
        CHAIN_FALLBACK_RATE_MAX
    );

    // (b) The chain never drifts below the quality-gate floor at any tick.
    assert!(
        report.min_scaled_j_floor() >= CHAIN_MIN_SCALED_J_FLOOR,
        "monotonic 50-tick plate sweep: min_scaled_j_floor={} must be >= {} \
         (see CHAIN_MIN_SCALED_J_FLOOR doc for why 0.01 not the task \
         description's illustrative 0.10)",
        report.min_scaled_j_floor(),
        CHAIN_MIN_SCALED_J_FLOOR
    );
}

// ── Step-7/8: non-monotonic (not-nearest) recovery ────────────────────────────

/// A deterministic, fixed-seed 64-bit LCG (Knuth's MMIX constants) mapped
/// into `[lo, hi]`, producing `n` values that interleave small local steps
/// with occasional full-range jumps — no `Date`/random source, so the
/// sequence (and hence the fallback counts derived from it) is 100%
/// reproducible in CI.
///
/// Every 5th value (index `i` with `i % 5 == 0`, `i >= 1`) is a full-range
/// jump anywhere in `[lo, hi]`; the other values take a small step (up to
/// ±2% of the range) from the previous value, clamped to `[lo, hi]`. For
/// the non-monotonic test's `[0.30, 0.60]` range, a full-range jump has
/// roughly a `(0.60 - ~0.53) / (0.60 - 0.30) ≈ 23%` chance of landing in
/// the plate fixture's pct-saturation zone (`hole_diameter >= ~0.53`, see
/// the step-3 test's doc) and triggering a fallback there — and once a
/// later jump lands back below that boundary, subsequent small steps are
/// accepted again, demonstrating self-recovery.
///
/// The interleave cadence (`JUMP_EVERY`) and small-step magnitude
/// (`SMALL_STEP_FRACTION`) are internal tuning constants, not part of the
/// public signature. `seed = 8` was chosen (task 2951 step-8) by sweeping
/// several seeds/cadences/magnitudes against the plate fixture and picking
/// one with comfortable margin on every step-7 assertion — not a
/// knife-edge fit (fallback_count=6/50, fallback_rate=0.12,
/// min_scaled_j_floor≈0.0142 vs the 0.01 floor).
///
/// `n == 0` returns an empty vec; otherwise index 0 is always `lo` (the
/// chain root, matching the monotonic sweep's start point for a fair
/// side-by-side comparison).
fn lcg_jumps(seed: u64, n: usize, lo: f64, hi: f64) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    // Knuth's MMIX 64-bit LCG constants.
    const A: u64 = 6364136223846793005;
    const C: u64 = 1442695040888963407;
    const JUMP_EVERY: usize = 5;
    const SMALL_STEP_FRACTION: f64 = 0.02;

    let mut state = seed;
    let mut next_unit = move || {
        state = state.wrapping_mul(A).wrapping_add(C);
        // Top 53 bits -> [0, 1) double (standard LCG-to-float technique).
        (state >> 11) as f64 / (1u64 << 53) as f64
    };

    let range = hi - lo;
    let small_delta_max = range * SMALL_STEP_FRACTION;
    let mut out = Vec::with_capacity(n);
    out.push(lo);
    let mut current = lo;
    for i in 1..n {
        let u = next_unit();
        let next = if i % JUMP_EVERY == 0 {
            lo + u * range
        } else {
            let delta = (u * 2.0 - 1.0) * small_delta_max;
            (current + delta).clamp(lo, hi)
        };
        out.push(next);
        current = next;
    }
    out
}

/// Minimum absolute margin — in fallback **count**, not rate — that the
/// non-monotonic sweep's [`runner::ChainReport::fallback_count`] must clear
/// above the monotonic sweep's, for assertion (b) in
/// [`non_monotonic_plate_jumps_trigger_fallbacks_and_chain_self_recovers`].
///
/// (reviewer_comprehensive test-robustness finding, amendment round 3 —
/// round 2's `71f1b5897b` only documented this finding as "no action
/// required"; this constant is the actual code fix.) Both sweeps have the
/// same 50 morph ticks, so a count margin is equivalent in direction to a
/// rate margin but avoids the previous bare
/// `report.fallback_rate() > monotonic_report.fallback_rate()`, which
/// directly coupled two independently-computed *live* quantities: a future
/// morph/quality-check tuning change could nudge the monotonic sweep up
/// towards its own permitted ceiling (`CHAIN_FALLBACK_RATE_MAX` allows up
/// to ~2 of 50) while also nudging the non-monotonic seed-8 sequence's
/// count down (documented at 6, see [`lcg_jumps`]'s doc), flipping a bare
/// `>` on a gap of only one tick — nowhere near the "materially more"
/// property this assertion exists to check. Requiring the non-monotonic
/// count to clear the *live* monotonic count by at least this margin keeps
/// the comparison honest against any future shift in the true monotonic
/// baseline (a hardcoded pin against either sweep's absolute count would
/// not — see `monotonic_sweep_report`'s doc for the identical staleness
/// concern), while no longer flipping on a one-tick nudge: today's actual
/// gap (6 vs 0) clears this margin with comfortable room to spare, and even
/// in the worst *legitimate* monotonic case (2, at `CHAIN_FALLBACK_RATE_MAX`'s
/// own ceiling) the non-monotonic sweep would still need to clear 4.
const CHAIN_NON_MONOTONIC_FALLBACK_MARGIN: usize = 2;

/// PRD `docs/prds/v0_3/mesh-morphing.md` task #14, claim (c): a
/// non-monotonic (not-nearest) chain is caught by the quality check and
/// self-recovers via remesh-fallback.
///
/// Sweeps the same plate fixture over a deterministic fixed-seed LCG
/// sequence ([`lcg_jumps`]) of 51 values in `[0.30, 0.60]`, interleaving
/// small steps with full-range jumps — some of which land in the
/// fixture's pct-saturation zone (`hole_diameter >= ~0.53`, see the
/// step-3 test's doc) and trigger a fallback. Compares against the
/// monotonic sweep (step-6) as a baseline.
#[test]
fn non_monotonic_plate_jumps_trigger_fallbacks_and_chain_self_recovers() {
    let fixture = |hole_diameter: f64| fixtures::plate_with_hole(1.0, hole_diameter, 0.1, 4, 2);
    let opts = chain_options();

    // Comparison baseline: the same monotonic sweep as step-6's test, via
    // the shared `monotonic_sweep_report()` `OnceLock` (reviewer_comprehensive
    // efficiency finding, amendment round 2 — this previously recomputed the
    // full 51-value/50-solve monotonic sweep inline here too, roughly
    // doubling this file's total elasticity_morph solve count). Still a
    // live, actually-computed `ChainReport` (not a hardcoded constant), so
    // assertion (b) below stays honest against any future change to
    // `elasticity_morph`/`quality_check` that shifts the true monotonic
    // rate — see `monotonic_sweep_report`'s doc for why sharing it here is
    // safe and order-independent.
    let monotonic_report = monotonic_sweep_report();

    let params = lcg_jumps(8, 51, 0.30, 0.60);
    let report = runner::run_chain(fixture, &params, &opts);

    // eprintln! the observed non-monotonic fallback_rate for the CI perf log.
    eprintln!(
        "[chain-degradation non-monotonic-50] fallback_count={} fallback_rate={:.4} \
         min_scaled_j_floor={:.6} (monotonic baseline fallback_rate={:.4})",
        report.fallback_count(),
        report.fallback_rate(),
        report.min_scaled_j_floor(),
        monotonic_report.fallback_rate()
    );

    // (a) Fallbacks DO happen — the quality check catches morphs where
    // most-recent is not nearest.
    assert!(
        report.fallback_count() >= 1,
        "non-monotonic plate sweep: expected >= 1 fallback, got {}",
        report.fallback_count()
    );

    // (b) Bad jumps cause MATERIALLY more fallbacks than the tiny monotonic
    // steps — an absolute-count margin against the live monotonic baseline,
    // not a bare `>` between two independently-drifting rates
    // (reviewer_comprehensive test-robustness finding, amendment round 3:
    // see CHAIN_NON_MONOTONIC_FALLBACK_MARGIN's doc).
    assert!(
        report.fallback_count()
            >= monotonic_report.fallback_count() + CHAIN_NON_MONOTONIC_FALLBACK_MARGIN,
        "non-monotonic fallback_count={} must exceed the monotonic baseline={} \
         by at least {} (materially more, not just numerically more) — \
         non-monotonic fallback_rate={:.4}, monotonic fallback_rate={:.4}",
        report.fallback_count(),
        monotonic_report.fallback_count(),
        CHAIN_NON_MONOTONIC_FALLBACK_MARGIN,
        report.fallback_rate(),
        monotonic_report.fallback_rate()
    );

    // (c) The chain self-recovers: quality never collapses, because each
    // fallback resets to a fresh remesh.
    assert!(
        report.min_scaled_j_floor() >= CHAIN_MIN_SCALED_J_FLOOR,
        "non-monotonic plate sweep: min_scaled_j_floor={} must be >= {}",
        report.min_scaled_j_floor(),
        CHAIN_MIN_SCALED_J_FLOOR
    );

    // (d) At least one accepted morph (fell_back==false, i>=1) occurs AFTER
    // the first fallback tick — the chain resumes morphing post-reset.
    let first_fallback_idx = report
        .ticks
        .iter()
        .position(|t| t.fell_back)
        .expect("assertion (a) already confirmed at least one fallback exists");
    // Guard (task 2951 amendment, reviewer_comprehensive finding #2): the
    // slice below needs at least one tick strictly after `first_fallback_idx`
    // to inspect. That holds today (the documented seed-8 LCG sequence has 6
    // fallbacks and the first one is not the final tick), but assert it
    // explicitly rather than let a future fixture/solver tweak that shifted
    // the sequence silently degrade to an empty
    // `report.ticks[first_fallback_idx + 1..]` slice — whose `.any()` would
    // return `false` and fail on the self-recovery message below, obscuring
    // that the real cause is "no ticks remain to check", not a recovery
    // failure.
    assert!(
        first_fallback_idx < report.ticks.len() - 1,
        "first fallback landed at the last tick (index {} of {} total ticks) — \
         no ticks remain after it to check post-reset recovery; adjust the LCG \
         seed/cadence so at least one tick follows the first fallback",
        first_fallback_idx,
        report.ticks.len()
    );
    assert!(
        report.ticks[first_fallback_idx + 1..]
            .iter()
            .any(|t| !t.fell_back),
        "expected at least one accepted morph (fell_back=false) after the first \
         fallback tick (index {}) — the chain should resume morphing post-reset",
        first_fallback_idx
    );
}
