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

// ── Step-1: chain-runner smoke on a short monotonic plate chain ──────────────

#[test]
fn chain_runner_smoke_on_short_monotonic_plate_chain_accepts_all_ticks() {
    use reify_mesh_morph::MorphOptions;

    // Same plate fixture + relaxed-pct options profile as the task #13
    // calibration sweeps: outer=1.0, thickness=0.1, n_radial=4, n_through=2,
    // hole_diameter is the swept parameter.
    let fixture = |hole_diameter: f64| fixtures::plate_with_hole(1.0, hole_diameter, 0.1, 4, 2);
    let params = [0.30_f64, 0.304, 0.308, 0.312];
    // Relaxed pct floor only — mirrors `calibration_sweep_options()` in
    // tests/calibration.rs (see that fn's doc for the fixture-specific
    // rationale). min_sj/AR stay at production defaults.
    let opts = MorphOptions {
        quality_floor_pct_below_025: 0.99,
        ..MorphOptions::default()
    };

    let report = runner::run_chain(fixture, &params, &opts);

    // (a) One TickRecord per param.
    assert_eq!(
        report.ticks.len(),
        params.len(),
        "run_chain must produce one TickRecord per param, got {} ticks for {} params",
        report.ticks.len(),
        params.len()
    );

    // (b) Every tick's in-use mesh shares connectivity with the root ("morph
    // preserves connectivity"). The runner's TickRecord does not carry the
    // mesh itself, but `tick.param` lets us re-derive the fixture mesh at
    // that param directly. Two structural facts together make this a valid
    // proof that EVERY tick's actual in-use mesh (accepted or fallback) has
    // the root's tet_indices length: (i) the fixture is connectivity-
    // invariant across its swept parameter (fixtures.rs module docs), so a
    // fresh fallback mesh at any param always matches the root's tet_indices
    // length; and (ii) `elasticity_morph` deforms vertices in place and
    // clones tet_indices unchanged, so an accepted tick's mesh has exactly
    // its source's tet_indices length, which chains back to the root by
    // induction.
    let root_tet_len = fixture(params[0]).0.tet_indices().unwrap().len();
    for tick in &report.ticks {
        let (mesh_at_param, _) = fixture(tick.param);
        assert_eq!(
            mesh_at_param.tet_indices().unwrap().len(),
            root_tet_len,
            "tick param={}: fixture tet_indices length must match the root's \
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
    use reify_mesh_morph::MorphOptions;

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
    // that test's comment for the fixture-specific rationale).
    let opts = MorphOptions {
        quality_floor_pct_below_025: 0.99,
        ..MorphOptions::default()
    };

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
    let fixture = |hole_diameter: f64| fixtures::plate_with_hole(1.0, hole_diameter, 0.1, 4, 2);
    let params = linspace(0.30, 0.50, 51);
    let opts = chain_options();

    let report = runner::run_chain(fixture, &params, &opts);

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
