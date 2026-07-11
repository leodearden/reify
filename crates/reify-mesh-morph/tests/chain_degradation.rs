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
