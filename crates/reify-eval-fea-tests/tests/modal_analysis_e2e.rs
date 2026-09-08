//! End-to-end integration tests for the `fn modal_analysis` @optimized →
//! ComputeNode → trampoline pipeline (task ζ, docs/prds/v0_3/modal-analysis.md
//! §10).
//!
//! Steps:
//!   step-13/14 — trampoline registration + seam pin (always-run)
//!   step-15/16 — cantilever first-mode-frequency e2e (release-gated)
//!   step-17/18 — simply-supported first-mode + higher-mode band (release-gated)
//!   task μ     — printer-gantry dogfood: 5-mode structural gate (release-gated)

use reify_core::{DimensionVector, Severity, ValueCellId};
use reify_eval::ComputeFn;
use reify_ir::Value;
use reify_test_support::{make_simple_engine, parse_and_compile_with_stdlib};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Load and compile the cantilever modal smoke fixture.
fn cantilever_source() -> &'static str {
    include_str!("../../../examples/modal/cantilever_beam_modes.ri")
}

/// Load and compile the simply-supported modal smoke fixture.
fn simply_supported_source() -> &'static str {
    include_str!("../../../examples/modal/simply_supported_beam_modes.ri")
}

/// Load and compile the printer-gantry modal dogfood fixture (task μ).
fn printer_gantry_source() -> &'static str {
    include_str!("../../../examples/modal/printer_gantry_modes.ri")
}

/// Load and compile the clamped-clamped vs pinned-pinned BC-kind fixture
/// (task 6663). Lives in this crate's `tests/fixtures/` rather than
/// `examples/modal/` because it is purely this crate's test detail (the 4-tier
/// fixture standard, tests/prd-gate/README.md → "Where fixtures live"), and
/// because `examples/**` is walked recursively by
/// `no_stale_undef_invariant_gate.rs::corpus_files` into a 24-shard eval sweep —
/// a heavy modal example there would add a full modal solve to that sweep.
fn clamped_clamped_source() -> &'static str {
    include_str!("fixtures/clamped_clamped_beam_modes.ri")
}

/// Read a frequency cell (Hz) as `f64`, tolerating the `Real` placeholder
/// (`Mode.frequency : Real`, modal_analysis.ri) or a dimensioned `Scalar`.
fn read_frequency(val: &Value) -> f64 {
    match val {
        Value::Real(r) => *r,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected a frequency Real/Scalar, got: {:?}", other),
    }
}

/// A prismatic beam fixture's section and material — everything
/// `fₙ = (βL)²/(2π)·√(E·I/(ρ·A·L⁴))` needs besides βL.
///
/// One value per FIXTURE, so the numbers a fixture's `.ri` file declares are
/// transcribed into this file exactly once. Before this struct (review
/// suggestion 3) the clamped-clamped fixture's section existed only in prose —
/// in the `.ri` header, in a constants block comment and in the test's own
/// header — while its three analytic references were transcribed decimal
/// literals with nothing linking them back. Editing the fixture's geometry or
/// material left all three bands silently referring to a beam that no longer
/// existed.
struct BeamSection {
    /// Young's modulus, Pa.
    e: f64,
    /// Density, kg/m³.
    rho: f64,
    /// Span, m (the X axis).
    l: f64,
    /// Width, m (the Y axis).
    b: f64,
    /// Height, m — the Z axis, i.e. the bending axis the βL families below
    /// describe (`I = b·h³/12`, deflection in Z).
    h: f64,
}

/// The shared 200 × 10 × 2 mm AISI 1045 steel beam behind the cantilever and
/// simply-supported e2e fixtures (`cantilever_beam_modes.ri`,
/// `simply_supported_beam_modes.ri`). Mirrors `modal_benchmarks.rs`'s
/// `STEEL_E_PA` / `STEEL_DENSITY` and its `BeamFixture { lx: 0.2, ly: 0.01,
/// lz: 0.002 }`.
const STEEL_BEAM_SECTION: BeamSection =
    BeamSection { e: 205.0e9, rho: 7850.0, l: 0.2, b: 0.01, h: 0.002 };

/// The task-6663 dogfood section behind `fixtures/clamped_clamped_beam_modes.ri`
/// — a CFRP rolled tube smeared to a solid section of equivalent EI and
/// mass/length: `span = 800mm`, square `h = 44.588mm` (so `b == h`),
/// `youngs_modulus = 110GPa`, `density = 695.39kg/m^3`.
///
/// These five numbers must match that fixture's `ClampedClampedBeamModes`
/// parameters and its `CFRP_Rolled_Tube` material. They are the ONLY place the
/// section is spelled for assertion purposes: the three bands below name their
/// βL family and nothing else.
const CC_FIXTURE_SECTION: BeamSection =
    BeamSection { e: 110.0e9, rho: 695.39, l: 0.8, b: 0.044588, h: 0.044588 };

/// Analytic Euler–Bernoulli frequency (Hz) for `section`, given the
/// dimensionless eigen-coefficient `beta_l` (β·L).
///
///   fₙ = (βL)² / (2π) · √( E·I / (ρ·A·L⁴) ),  I = b·h³/12,  A = b·h
///
/// βL families: cantilever 1.875104, simply-supported mode `n` `n·π`,
/// clamped-pinned 3.926602, clamped-clamped 4.730041.
fn analytic_beam_frequency_for(beta_l: f64, section: &BeamSection) -> f64 {
    use std::f64::consts::PI;
    let BeamSection { e, rho, l, b, h } = *section;
    let i: f64 = b * h.powi(3) / 12.0;
    let a: f64 = b * h;
    beta_l.powi(2) / (2.0 * PI) * (e * i / (rho * a * l.powi(4))).sqrt()
}

/// [`analytic_beam_frequency_for`] on the shared steel section — the spelling
/// the cantilever and simply-supported e2es have always used.
fn analytic_beam_frequency(beta_l: f64) -> f64 {
    analytic_beam_frequency_for(beta_l, &STEEL_BEAM_SECTION)
}

/// Cantilever P2 first-mode rel-err tolerance — the calibrated honest P2 floor.
/// Mirrors `modal_benchmarks.rs::CANTILEVER_P2_REL_TOL` (the step-4 kernel gate,
/// MEASURED at nx=16, nz=2): the P2 quadratic tets resolve bending curvature and
/// clear the 2% target, distinctly tighter than the P1 10% floor — so meeting it
/// end-to-end proves the fixture runs at element_order = P2 (task 4066).
const CANTILEVER_P2_REL_TOL: f64 = 0.02;

/// Simply-supported P2 rel-err tolerance (per mode) — the calibrated honest P2
/// floor. Mirrors `modal_benchmarks.rs::SS_P2_REL_TOL` (the step-6 kernel gate,
/// MEASURED at nx=24, nz=2): P2 clears 2% on all three bending modes, replacing
/// the prior looser P1 10%/12% bands (task 4066).
const SS_P2_REL_TOL: f64 = 0.02;

// ── task 6663: support-KIND acceptance bands (clamped-clamped vs pinned-pinned) ─
//
// All three constants below are bands over the SAME dogfood section —
// [`CC_FIXTURE_SECTION`], which IS the fixture's declaration rather than a prose
// restatement of it — solved at element_order = P2 on the trampoline's own
// `build_beam_mesh` discretization (nx = round(800/44.588 · 6) = 108, nz = 6).
//
// The three references are COMPUTED from that section by
// `analytic_beam_frequency_for`, so only the βL family is spelled per band
// (amendment, review suggestion 3 — they were transcribed decimal literals, and
// a fixture geometry edit would have left all three pointing at a beam that no
// longer existed). Evaluated, they reproduce the decimal literals they replace
// to within 6e-6 relative — βL = π → 397.3282 Hz (was 397.33),
// βL = 3.926602 → 620.7024 Hz (was 620.702), βL = 4.730041 → 900.6985 Hz (was
// 900.699) — so every band below is arithmetically unchanged. The test prints
// all three, with the section they came from, under `--nocapture`.

/// βL for the pinned-pinned (simply-supported) fundamental — λ² = 9.8696.
const CC_BETA_L_PINNED: f64 = std::f64::consts::PI;

/// βL for the clamped-clamped fundamental — λ² = 22.3733, i.e. `(4.730041/π)²
/// = 2.267×` the pinned-pinned one. That ratio IS task 6663's acceptance.
const CC_BETA_L_FIXED: f64 = 4.730041;

/// βL for the clamped-pinned (propped-cantilever) fundamental — the MIXED pair
/// `[FixedSupport("x_min"), PinnedSupport("x_max")]`, task 6663's scope
/// extension.
///
/// λ² = 15.4182, strictly between the pinned-pinned 9.8696 and the
/// clamped-clamped 22.3733, and the CP/PP frequency ratio the closed form
/// returns (1.562191) matches 15.4182/9.8696 to six digits — the cross-check
/// that this βL is the right root and not a transcription slip.
const CC_BETA_L_PROPPED: f64 = 3.926602;

/// Pinned-pinned band, read on the VERTICAL (Z-dominant) fundamental.
/// **MEASURED at this exact mesh**, in release:
///
/// ```text
/// [modal bc-kind]   result_pinned mode 0: f=391.0495 Hz, participation_z=1.350404e-4
/// [modal bc-kind]   result_pinned mode 1: f=395.2177 Hz, participation_z=8.939962e-1
/// [modal bc-kind]   result_pinned vertical (Z-dominant) family: [395.2177, 1558.3373]
/// [modal bc-kind] f1z_pinned=395.2177 Hz (analytic 397.330, err -0.53%) [raw f1 = 391.0495 Hz]
/// ```
///
/// −0.53% leaves 2.47% of margin on a measured reference. Deliberately the SAME
/// 3% construction as the two bands below, so all three are read the same way.
///
/// # Why the vertical member and not `first_frequency`
///
/// The raw fundamental 391.0495 Hz is the LATERAL (Y-bending) mode — the square
/// section makes the two directions near-degenerate here (a 1.1% split), so
/// comparing the raw value to the pinned-pinned BENDING analytic only worked
/// because the pair happens to sit close together. Every band in this test now
/// reads the vertical family, the same one signals (f)/(g) use.
///
/// # What this band is, and is NOT
///
/// It is an ACCURACY statement about the vertical mode the pinned-pinned
/// analytic describes: 3% around 397.33 Hz. It is deliberately NOT the
/// bit-preservation guard, and an earlier revision of this comment claimed it
/// was — asserting that the raw fundamental "reproduces the dogfood round's
/// 391.049 Hz to four decimals" when the test only checked that value finite and
/// positive. At 3% on a different mode, the pinned realization could have
/// drifted by 2.5% (and the raw lateral mode by any amount) with this band still
/// green.
///
/// # Where bit-preservation IS asserted (amendment, review suggestion 2)
///
/// Not here, and no longer anywhere in this file. The claim — task 6663 leaves
/// `simply_supported_pin_pin_bcs` untouched, so the pin-pin Dirichlet set is
/// unchanged — is STRUCTURAL, and is asserted structurally and exactly at the
/// unit level, on the DOF sets themselves:
///
///   * `modal_ops::tests::build_dirichlet_bcs_discriminates_support_kind`
///     case (i) — two `PinnedSupport`s still select the pin-pin set;
///   * `modal_ops::tests::simply_supported_pin_pin_bcs_places_minimal_anchors` —
///     Z on every end-face node plus exactly the three neutral-axis anchors;
///   * `modal_ops::tests::build_dirichlet_bcs_pin_pin_special_case_does_not_discard_other_faces`
///     — the special case adds to, rather than replaces, the other faces.
///
/// A previous revision ALSO pinned the pinned configuration's raw
/// `first_frequency` here, at 391.0495 Hz ± 0.5%, and called that the
/// bit-preservation guard. It was not one: that number depends on
/// `build_beam_mesh`'s derived nx, the P2 promotion, the element assembly and
/// the shift-invert tolerance as much as on the Dirichlet set, so any legitimate
/// improvement in any of them would have failed it under the message "the pin-pin
/// Dirichlet set must be unchanged" — a confidently wrong diagnosis, and exactly
/// the mis-attribution class the rest of these comments warn about. It was also
/// the file's only band read off a RAW mode index rather than a dominant-axis
/// family, i.e. it re-adopted the index fragility everything else here exists to
/// avoid (the pinned pair is 1.07% apart). The numeric side of the same property
/// is covered by `e2e_simply_supported_modes_match_analytic`'s untouched 2%
/// bands.
const CC_FIXTURE_PINNED_REL_TOL: f64 = 0.03;

/// Clamped-clamped band, read on the VERTICAL (Z-dominant) fundamental.
/// **MEASURED at this exact mesh**, in release:
///
/// ```text
/// [modal bc-kind]   result_fixed mode 0: f=887.5474 Hz, participation_z=7.637559e-1
/// [modal bc-kind]   result_fixed mode 1: f=890.8998 Hz, participation_z=7.521734e-4
/// [modal bc-kind]   result_fixed vertical (Z-dominant) family: [887.5474, 2384.6260]
/// [modal bc-kind] f1z_fixed =887.5474 Hz (analytic 900.699, err -1.46%) [raw f1 = 887.5474 Hz]
/// ```
///
/// (`cargo test --release -p reify-eval-fea-tests
/// e2e_two_fixed_supports_are_clamped_clamped_not_simply_supported -- --nocapture`)
///
/// This configuration is the one where the vertical member IS the raw
/// fundamental — 887.5474 either way, with the lateral mode 0.4% ABOVE it — so
/// moving this band onto the vertical family changed the assertion's basis
/// without changing its value. That thin 0.4% split is precisely why reading the
/// family rather than mode 0 matters: a small solver or mesh perturbation could
/// reorder the pair, and the classification survives that where an index does not.
///
/// 3% around the analytic value therefore leaves 1.54% of margin on a measured
/// reference — deliberately the SAME construction as
/// [`CC_FIXTURE_PINNED_REL_TOL`] and [`CC_FIXTURE_PROPPED_REL_TOL`], so all
/// three bands are read the same way. It excludes the defect value (391.05 Hz,
/// the pinned answer) by 57%, i.e. by 19 band-widths.
///
/// This REPLACES a derived 10% band whose bounds came from an nx=16 probe plus a
/// Rayleigh-monotonicity argument (which does not strictly apply — an
/// nx=108/nz=6 mesh is not a nested refinement of nx=16) and a doubled scaled
/// deviation, and which would have first executed at the release merge gate
/// because this test is `#[cfg_attr(debug_assertions, ignore)]`. The measurement
/// retires that reasoning, and shows it was pessimistic in the interesting
/// direction: the derived floor was ≈ 836 Hz (scaling the pinned −1.58%
/// deviation by the (βL)² ratio 22.373/9.870 = 2.267 → ≈ −3.6%, then doubling),
/// but the clamped section actually deviates −1.46% — essentially the same as
/// the pinned case, not 2.3× worse. The section is stubby (L/h ≈ 18), so shear
/// and rotary inertia are NOT negligible here; the measured sign is consistent
/// with that (the 3-D FE model carries shear, the Euler–Bernoulli reference does
/// not, so the FE value sits below analytic), and the magnitude says the effect
/// is ~1.5% at this mesh rather than the several percent the derivation guarded
/// against.
const CC_FIXTURE_FIXED_REL_TOL: f64 = 0.03;

/// Clamped-pinned (propped-cantilever) band. **MEASURED at this exact mesh**, in
/// release:
///
/// ```text
/// [modal bc-kind]   result_propped mode 0: f=141.6988 Hz, participation_z=1.436489e-6
/// [modal bc-kind]   result_propped mode 1: f=616.9229 Hz, participation_z=8.143401e-1
/// [modal bc-kind]   result_propped mode 2: f=877.5937 Hz, participation_z=1.130448e-5
/// [modal bc-kind]   result_propped mode 3: f=1955.2129 Hz, participation_z=7.980694e-3
/// [modal bc-kind]   result_propped vertical (Z-dominant) family: [616.9229, 1955.2129]
/// [modal bc-kind] f1z_propped=616.9229 Hz (analytic 620.702, err -0.61%)
/// ```
///
/// (`cargo test --release -p reify-eval-fea-tests
/// e2e_two_fixed_supports_are_clamped_clamped_not_simply_supported -- --nocapture`)
///
/// Deliberately the SAME 3% construction as the two headline bands above, so all
/// three are read the same way, and anchored the same way: on a first-hand
/// release measurement of THIS fixture at THIS mesh, not on a prediction. The
/// measured −0.61% leaves 2.39% of margin — comparable to the pinned band's
/// 2.47% and more than the clamped band's 1.54%, because the propped mode is
/// well resolved at this discretization, not because the band was loosened.
///
/// What this band is for: under the pre-6663 defect all three configurations
/// returned the bit-identical pinned answer, so a band that merely excluded
/// 391.05 Hz would already be satisfied by the clamped-clamped 887.55 Hz. This
/// one is two-sided and narrow enough to exclude BOTH siblings — the pinned
/// vertical 395.22 Hz is −36% from the analytic and the clamped 887.55 Hz is
/// +43%, i.e. each is more than 12 band-widths away — so it pins the propped
/// cantilever on its OWN analytic rather than on "not the one wrong answer we
/// happened to name".
///
/// Read against the VERTICAL (Z-dominant) fundamental, not `first_frequency`;
/// the test's signal (f) documents the measurement that forces that choice, and
/// signal (h) asserts it (the raw fundamental 141.70 Hz is a lateral
/// clamped-free cantilever mode, 4.4× below the propped bending mode). Since
/// task 6663's amendment the other two bands read that family too.
const CC_FIXTURE_PROPPED_REL_TOL: f64 = 0.03;

/// The headline acceptance signal: clamping both end faces must be a genuinely
/// DIFFERENT structure from pinning both. The analytic ratio is a pure BC ratio,
/// (4.730041)²/π² = 22.373/9.870 = 2.267, and the expected measured ratio is in
/// [2.14, 2.30]. A 2.0 floor is insensitive to every modelling uncertainty in the
/// two bands above. Under the defect this ratio read exactly 1.0 — the two BC sets
/// were bit-identical.
///
/// MEASURED in release, WITHIN the vertical family (887.5474 / 395.2177):
/// **2.2457** — 0.94% below the analytic BC ratio, and the cleanest evidence
/// that the two realizations now differ by exactly the physics and not by a
/// modelling artifact (the two solves share one section, one mesh and one
/// material, so everything but the BC set cancels).
///
/// Taken within the vertical family rather than over the raw fundamentals, for
/// the reason [`CC_FIXTURE_PINNED_REL_TOL`] gives: the pinned raw fundamental is
/// the LATERAL mode, so the raw ratio (2.2697) compared a lateral mode against a
/// vertical one and landed near the analytic by coincidence of a near-degenerate
/// section. Both readings clear the 2.0 floor by a wide margin; only one of them
/// is comparing like with like.
const CC_FIXTURE_MIN_FIXED_PINNED_RATIO: f64 = 2.0;

// ── step-13: RED — trampoline registration + seam pin ────────────────────────
//
// Compile-time seam pin: coerce
//   `reify_eval::modal_ops::solve_modal_analysis_trampoline`
// to `ComputeFn`, pinning the cross-crate trampoline signature. Compile success
// is the signal (no runtime assertion). Paired with a runtime check that
// `register_compute_fns` installs the trampoline under "modal::free_vibration".
//
// RED until step-14 adds `solve_modal_analysis_trampoline` + its registration:
// the seam pin references a symbol that does not yet exist (compile-fail RED),
// mirroring buckling_smoke.rs's step-1 seam pin.

#[allow(dead_code)]
fn _seam_pin() {
    let _f: ComputeFn = reify_eval::modal_ops::solve_modal_analysis_trampoline;
}

/// Step-13: `register_compute_fns` installs the modal trampoline under the key.
///
/// Constructs `make_simple_engine()`, calls
/// `reify_eval::compute_targets::register_compute_fns(&mut engine)`, and asserts
/// `engine.compute_dispatch("modal::free_vibration").is_some()`.
///
/// Expected to fail (compile error) until step-14 creates the trampoline and
/// registers it.
#[test]
fn register_compute_fns_installs_modal_free_vibration() {
    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    assert!(
        engine.compute_dispatch("modal::free_vibration").is_some(),
        "register_compute_fns must install a trampoline under 'modal::free_vibration'"
    );
}

// ── step-15 / step-11: cantilever first-mode-frequency e2e (P2 2% band) ───────
//
// Four observable signals on the cantilever fixture (examples/modal/
// cantilever_beam_modes.ri):
//   (a) no Error-severity diagnostics after parse + eval
//   (b) a ComputeNode with target == "modal::free_vibration" in the graph
//   (c) the `result` cell is a non-Undef StructureInstance/Map
//   (d) the first-mode frequency `f1` is within CANTILEVER_P2_REL_TOL (2%) of the
//       analytic Euler–Bernoulli cantilever fundamental f₁ = (1.875²/2π)·
//       √(EI/ρAL⁴) ≈ 41.3 Hz — the P2-tet bending-lock-free band (task 4066),
//       distinctly tighter than the prior P1 10% floor, so it can only be met
//       once the fixture runs at element_order = ElementOrder.P2.
//
// Gated like buckling: the modal solve assembles K + M and runs a generalized
// eigensolve — heavy in debug. The registration pin above runs always; this e2e
// gate runs release-only.
//
// ── step-11 RED → step-12 GREEN ──────────────────────────────────────────────
//
// BC realization (build_dirichlet_bcs): the single FixedSupport(target:"x_min")
// clamps ALL THREE translational DOFs on every root-face (x ≈ 0) node — the
// cantilever clamped-free configuration (catching P2 edge-midpoints by
// coordinate once the trampoline promotes the mesh).
//
// RED (step-11): the fixture is still P1 (no `element_order` field), so the P1
// constant-strain solve biases the fundamental high — MEASURED f1 ≈ 44.715 Hz vs
// analytic 41.271 Hz → +8.34% (P1 tets lock in bending → overestimate K → f ∝ √K
// high), far outside the 2% P2 band, so the (d) assertion FAILS.
//
// GREEN (step-12): the fixture is re-authored to element_order = ElementOrder.P2
// and the trampoline assembles K/M on the coarse example-practical P2 mesh
// (matching the modal_benchmarks.rs cantilever gate, which clears 2% at nx=16,
// nz=2). The quadratic tets resolve bending curvature, driving f1 under the
// calibrated CANTILEVER_P2_REL_TOL (2%) floor — proving P2 is engaged end-to-end.

/// Cantilever: first-mode frequency within CANTILEVER_P2_REL_TOL (2%) of the
/// analytic value — the P2-tet band (RED until step-12 re-authors the fixture to
/// element_order = ElementOrder.P2; see the step-11/12 note above).
#[cfg_attr(debug_assertions, ignore = "heavy modal solve; release-only")]
#[test]
fn e2e_cantilever_first_mode_within_two_percent() {
    let source = cantilever_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // (a) No Error-severity diagnostics.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    // (b) A ComputeNode with target == "modal::free_vibration" must be present.
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let has_compute_node = snapshot
        .graph
        .compute_nodes
        .iter()
        .any(|(_, data)| data.target == "modal::free_vibration");
    assert!(
        has_compute_node,
        "expected a ComputeNode with target==\"modal::free_vibration\"; found targets: {:?}",
        snapshot
            .graph
            .compute_nodes
            .iter()
            .map(|(_, d)| d.target.as_str())
            .collect::<Vec<_>>()
    );

    // (c) The `result` cell must hold a non-Undef StructureInstance/Map.
    let result_cell = ValueCellId::new("CantileverBeamModes", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell CantileverBeamModes.result not found in eval result"));
    assert!(
        matches!(result_val, Value::StructureInstance(_) | Value::Map(_)),
        "expected result to be StructureInstance or Map (NOT Undef), got: {:?}",
        result_val
    );

    // (d) `f1` within the P2 2% band of the analytic cantilever fundamental
    //     (βL = 1.875). RED at P1 (~8.34%); GREEN once the fixture runs at P2.
    let f1_cell = ValueCellId::new("CantileverBeamModes", "f1");
    let f1 = read_frequency(
        eval_result
            .values
            .get(&f1_cell)
            .unwrap_or_else(|| panic!("cell CantileverBeamModes.f1 not found in eval result")),
    );
    assert!(
        f1.is_finite() && f1 > 0.0,
        "f1 must be finite and positive, got: {}",
        f1
    );

    let f1_analytic = analytic_beam_frequency(1.875);
    let rel_err = (f1 - f1_analytic).abs() / f1_analytic;
    assert!(
        rel_err < CANTILEVER_P2_REL_TOL,
        "cantilever f1 = {:.3} Hz, analytic = {:.3} Hz, rel_err = {:.2}% > {:.2}% (P2 band)",
        f1,
        f1_analytic,
        rel_err * 100.0,
        CANTILEVER_P2_REL_TOL * 100.0
    );
}

// ── step-5 (task 4548): Mode.frequency is a dimensioned Scalar<Frequency> ─────
//
// `Mode.frequency` tightens from the `Real` PLACEHOLDER to `Frequency`
// (modal_analysis.ri:189; task 4548). This e2e gate matches the PRODUCED
// `frequency` field variant EXPLICITLY — deliberately NOT through the tolerant
// `read_frequency` / `as_f64` helpers (which accept Real OR Scalar) — so it
// pins the modal producer to construct a dimensioned `Value::Scalar`, not a
// bare `Value::Real`.
//
// RED (step-5): modal_ops.rs builds `("frequency", Value::Real(f))` for each
// mode, so the explicit `Value::Scalar { FREQUENCY }` match FAILS.
// GREEN (step-6): the producer builds `Value::Scalar { si_value: f,
// dimension: FREQUENCY }`, and this assertion passes. The runtime assertion
// also transitively pins first_frequency / mode_frequency to flow a
// Frequency-typed value.
//
// Heavy modal solve (assembles K + M, generalized eigensolve) — release-gated
// like the cantilever / simply-supported e2e tests.

/// Each produced `Mode.frequency` must be a dimensioned `Value::Scalar` of
/// dimension `FREQUENCY` (Hz = s⁻¹), pinning the modal producer to the
/// tightened `Mode.frequency : Frequency` surface type (task 4548).
#[cfg_attr(debug_assertions, ignore = "heavy modal solve; release-only")]
#[test]
fn e2e_mode_frequency_is_dimensioned_scalar() {
    let source = cantilever_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);
    let eval_result = engine.eval(&compiled);

    // The run must succeed (no Error diagnostics) to produce modes.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    // The `result` cell holds the ModalResult (StructureInstance/Map) with a
    // `modes` list of Mode structure-instances.
    let result_cell = ValueCellId::new("CantileverBeamModes", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell CantileverBeamModes.result not found in eval result"));

    let modes = match result_val {
        Value::StructureInstance(d) => d.fields.get(&"modes".to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String("modes".to_string())).cloned(),
        other => panic!(
            "expected ModalResult StructureInstance/Map, got: {:?}",
            other
        ),
    }
    .expect("ModalResult must expose a `modes` field");

    let mode_list = match modes {
        Value::List(items) => items,
        other => panic!("expected `modes` to be a List, got: {:?}", other),
    };
    assert!(
        !mode_list.is_empty(),
        "modal run must produce at least one mode"
    );

    // Read the first mode's `frequency` field and match its variant EXPLICITLY.
    // The tolerant `read_frequency` / `as_f64` helpers are intentionally avoided
    // here so the test fails while the producer still emits `Value::Real`.
    let freq = match &mode_list[0] {
        Value::StructureInstance(d) => d.fields.get(&"frequency".to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String("frequency".to_string())).cloned(),
        other => panic!("expected a Mode StructureInstance/Map, got: {:?}", other),
    }
    .expect("Mode must expose a `frequency` field");

    match &freq {
        Value::Scalar {
            si_value,
            dimension,
        } => {
            assert_eq!(
                *dimension,
                DimensionVector::FREQUENCY,
                "Mode.frequency must carry dimension FREQUENCY (Hz = s⁻¹), got {:?}",
                dimension
            );
            assert!(
                si_value.is_finite() && *si_value > 0.0,
                "Mode.frequency si_value must be finite and positive, got {}",
                si_value
            );
        }
        other => panic!(
            "Mode.frequency must be a dimensioned `Value::Scalar {{ FREQUENCY }}` \
             (tightened from the `Real` PLACEHOLDER; task 4548), got: {:?}",
            other
        ),
    }
}

// ── step-17 / step-11: simply-supported first-mode + higher modes (P2 2%) ─────
//
// The simply-supported fixture (examples/modal/simply_supported_beam_modes.ri)
// PINS BOTH end faces (x_min and x_max) with two `PinnedSupport`s. Five
// observable signals:
//   (a) no Error-severity diagnostics after parse + eval
//   (b) a ComputeNode with target == "modal::free_vibration" in the graph
//   (c) the `result` cell is a non-Undef StructureInstance/Map
//   (d) the FIRST-mode frequency f1 is within SS_P2_REL_TOL (2%) of the analytic
//       Euler–Bernoulli simply-supported fundamental f₁ = (π²/2π)·√(EI/ρAL⁴)
//       ≈ 115.9 Hz — the P2 band, anchored on the fundamental (the headline
//       signal).
//   (e) f2, f3 are present, finite, positive, strictly sorted ascending, and
//       each within SS_P2_REL_TOL (2%) of their analytic (nπ)² values
//       (f₂ ≈ 463.4 Hz, f₃ ≈ 1042.8 Hz) — the P2 band resolves all three
//       bending modes uniformly, replacing the looser P1 higher-mode floor.
//
// Release-gated like the cantilever e2e (heavy generalized eigensolve). The
// registration pin runs always; this e2e gate runs release-only.
//
// ── step-11 RED → step-12 GREEN ──────────────────────────────────────────────
//
// BC realization (build_dirichlet_bcs → simply_supported_pin_pin_bcs): BOTH
// beam-axis end faces are named AND every end-face support is a PinnedSupport,
// which selects the pin-pin branch — pin ONLY the transverse Z DOF on both end
// faces (the bending rotation dw/dx stays free, carried by the axial u(z)) +
// minimal axial/lateral anchors at the two end-face neutral-axis nodes
// (z = h/2). This yields the (nπ)² simply-supported family rather than the
// fixed-fixed family the all-DOF clamp would produce. Selection is by
// coordinate, so it catches the P2 edge-midpoint nodes once the trampoline
// promotes the mesh.
//
// Task 6663 re-aimed that discriminator: it used to fire on the target face
// NAMES alone, so the fixture's then-two-`FixedSupport`s took the pin-pin branch
// and a genuinely clamped-clamped beam was unreachable. The realization is now
// per-face and kind-aware, and the fixture spells `PinnedSupport`. The 2% bands
// below are unchanged and are the guard that the pin-pin numbers are
// bit-preserved across that change.
//
// RED (step-11): the fixture is still P1, so the constant-strain solve biases
// every bending mode high — MEASURED f1 = 125.752 Hz / f2 = 501.595 Hz /
// f3 = 1117.190 Hz vs analytic 115.862 / 463.448 / 1042.759 → +8.54% / +8.23% /
// +7.14% (P1 tets lock in bending, f ∝ √K). All three exceed the 2% P2 band, so
// (d)/(e) FAIL.
//
// GREEN (step-12): the fixture is re-authored to element_order = ElementOrder.P2
// and the trampoline assembles K/M on the coarse example-practical P2 mesh
// (matching the modal_benchmarks.rs SS gate, which clears 2% on all three modes
// at nx=24, nz=2). The quadratic tets drive f1/f2/f3 under SS_P2_REL_TOL (2%).

/// Read each mode's `(frequency_hz, participation_mass)` from a ModalResult
/// value — a measurement aid (printed under `--nocapture`) for telling vertical
/// bending modes (high participation along the z reference direction) apart from
/// lateral / torsional modes (≈ 0 z-participation) in the simply-supported
/// spectrum.
fn modes_freq_participation(result: &Value) -> Vec<(f64, f64)> {
    let modes = match result {
        Value::StructureInstance(d) => d.fields.get(&"modes".to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String("modes".to_string())).cloned(),
        _ => None,
    };
    let as_f64 = |v: Option<&Value>| -> f64 {
        match v {
            Some(Value::Real(r)) => *r,
            Some(Value::Scalar { si_value, .. }) => *si_value,
            _ => f64::NAN,
        }
    };
    match modes {
        Some(Value::List(items)) => items
            .iter()
            .map(|m| match m {
                Value::StructureInstance(d) => (
                    as_f64(d.fields.get(&"frequency".to_string())),
                    as_f64(d.fields.get(&"participation_mass".to_string())),
                ),
                _ => (f64::NAN, f64::NAN),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Frequencies (Hz, ascending) of the VERTICAL (Z-dominant) bending modes in a
/// `ModalResult`, selected by eigenvector dominant-axis classification: a mode
/// is Z-dominant when its shape energy along Z (`Σ_node φ_z²`) is ≥ the energy
/// along both X and Y.
///
/// This is the e2e mirror of the kernel gate's selection
/// (`modal_benchmarks.rs::axis_energy_fractions` →
/// `simply_supported_beam_p2_modal_within_two_percent`). It is required because
/// the wide-thin section (b = 10 mm, h = 2 mm) places the lateral Y-bending mode
/// (≈ 579 Hz) BETWEEN vertical modes 2 (≈ 463 Hz) and 3 (≈ 1043 Hz) in the raw
/// frequency-sorted spectrum, so the raw mode index does NOT map 1:1 to the
/// vertical (nπ)² family — `mode_frequency(result, 2)` is the lateral mode, not
/// vertical mode 3. Dominant-axis classification recovers the vertical family
/// (including the even vertical mode 2, whose net participation_mass is ≈ 0 by
/// antisymmetry but whose shape energy is unambiguously Z-aligned).
///
/// `Mode.shape` is `List<Vector([Real;3])>` (one per-node displacement;
/// modal_ops::mode_shape_value). Modes are producer-ordered ascending by
/// frequency, so the returned vector is ascending.
fn z_dominant_frequencies(result: &Value) -> Vec<f64> {
    let modes = match result {
        Value::StructureInstance(d) => d.fields.get(&"modes".to_string()).cloned(),
        Value::Map(m) => m.get(&Value::String("modes".to_string())).cloned(),
        _ => None,
    };
    let mode_list = match modes {
        Some(Value::List(items)) => items,
        _ => return Vec::new(),
    };
    let mut out = Vec::new();
    for mode in &mode_list {
        let fields = match mode {
            Value::StructureInstance(d) => d,
            _ => continue,
        };
        let freq = match fields.fields.get(&"frequency".to_string()) {
            Some(Value::Real(r)) => *r,
            Some(Value::Scalar { si_value, .. }) => *si_value,
            _ => continue,
        };
        let shape = match fields.fields.get(&"shape".to_string()) {
            Some(Value::List(nodes)) => nodes,
            _ => continue,
        };
        let mut energy = [0.0f64; 3];
        for node in shape {
            if let Value::Vector(comps) = node {
                for (a, slot) in energy.iter_mut().enumerate() {
                    if let Some(Value::Real(c)) = comps.get(a) {
                        *slot += c * c;
                    }
                }
            }
        }
        // Z-dominant iff the Z shape-energy ties-or-exceeds both X and Y. The `>=`
        // is deliberate: an exact energy tie is counted as Z-dominant (conservative
        // inclusion), so a borderline mode is never silently dropped from the
        // vertical family the caller asserts `len() >= 3` on before indexing.
        if energy[2] >= energy[0] && energy[2] >= energy[1] {
            out.push(freq);
        }
    }
    out
}

/// Simply-supported: first-mode frequency within 10% of the analytic value; the
/// higher modes (2-3) present, sorted, and within a measured band (step-18).
#[cfg_attr(debug_assertions, ignore = "heavy modal solve; release-only")]
#[test]
fn e2e_simply_supported_modes_match_analytic() {
    use std::f64::consts::PI;

    let source = simply_supported_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // (a) No Error-severity diagnostics.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    // (b) A ComputeNode with target == "modal::free_vibration" must be present.
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let has_compute_node = snapshot
        .graph
        .compute_nodes
        .iter()
        .any(|(_, data)| data.target == "modal::free_vibration");
    assert!(
        has_compute_node,
        "expected a ComputeNode with target==\"modal::free_vibration\"; found targets: {:?}",
        snapshot
            .graph
            .compute_nodes
            .iter()
            .map(|(_, d)| d.target.as_str())
            .collect::<Vec<_>>()
    );

    // (c) The `result` cell must hold a non-Undef StructureInstance/Map.
    let result_cell = ValueCellId::new("SimplySupportedBeamModes", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell SimplySupportedBeamModes.result not found in eval result"));
    assert!(
        matches!(result_val, Value::StructureInstance(_) | Value::Map(_)),
        "expected result to be StructureInstance or Map (NOT Undef), got: {:?}",
        result_val
    );

    // Read f1 / f2 / f3.
    let read_cell = |name: &str| -> f64 {
        read_frequency(
            eval_result
                .values
                .get(&ValueCellId::new("SimplySupportedBeamModes", name))
                .unwrap_or_else(|| {
                    panic!("cell SimplySupportedBeamModes.{name} not found in eval result")
                }),
        )
    };
    let f1 = read_cell("f1");
    let f2 = read_cell("f2");
    let f3 = read_cell("f3");

    // Analytic simply-supported Euler–Bernoulli modes (βL = nπ).
    let f1_analytic = analytic_beam_frequency(PI);
    let f2_analytic = analytic_beam_frequency(2.0 * PI);
    let f3_analytic = analytic_beam_frequency(3.0 * PI);

    // Measurement diagnostics (visible with `--nocapture`); the modes list with
    // per-mode z-participation distinguishes vertical-bending modes from
    // lateral / torsional ones in the spectrum.
    eprintln!(
        "[modal ss] f1={:.3} Hz (analytic {:.3}, err {:+.2}%)",
        f1,
        f1_analytic,
        (f1 - f1_analytic) / f1_analytic * 100.0
    );
    eprintln!(
        "[modal ss] f2={:.3} Hz (analytic {:.3}, err {:+.2}%)",
        f2,
        f2_analytic,
        (f2 - f2_analytic) / f2_analytic * 100.0
    );
    eprintln!(
        "[modal ss] f3={:.3} Hz (analytic {:.3}, err {:+.2}%)",
        f3,
        f3_analytic,
        (f3 - f3_analytic) / f3_analytic * 100.0
    );
    for (i, (f, p)) in modes_freq_participation(result_val).iter().enumerate() {
        eprintln!("[modal ss]   mode {i}: f={f:.3} Hz, participation_mass(z)={p:.6e}");
    }

    // (d) f1 within the P2 2% band of the analytic simply-supported fundamental
    //     (βL = π). f1 = first_frequency(result) is the lowest mode, which is
    //     unambiguously the vertical (Z-bending) fundamental. RED at P1 (~8.54%);
    //     GREEN once the fixture runs at P2.
    assert!(
        f1.is_finite() && f1 > 0.0,
        "f1 must be finite and positive, got: {}",
        f1
    );
    let f1_err = (f1 - f1_analytic).abs() / f1_analytic;
    assert!(
        f1_err < SS_P2_REL_TOL,
        "ss f1 = {:.3} Hz, analytic = {:.3} Hz, rel_err = {:.2}% > {:.2}% (P2 band)",
        f1,
        f1_analytic,
        f1_err * 100.0,
        SS_P2_REL_TOL * 100.0
    );

    // (e) The fixture's raw-index f2/f3 cells (mode_frequency(result, 1/2)) are
    //     present, finite, positive, and strictly ascending — proving the stdlib
    //     accessor is wired end-to-end. NOTE: the raw frequency-sorted spectrum
    //     interleaves the lateral Y-bending mode (≈ 579 Hz) between vertical
    //     modes 2 and 3, so f3 (= mode index 2) is the LATERAL mode here, not
    //     vertical mode 3. The rigorous per-mode 2% accuracy gate over the three
    //     VERTICAL bending modes is asserted in (f) by dominant-axis
    //     classification, mirroring the kernel gate
    //     modal_benchmarks.rs::simply_supported_beam_p2_modal_within_two_percent.
    for (name, f) in [("f2", f2), ("f3", f3)] {
        assert!(
            f.is_finite() && f > 0.0,
            "{} must be finite and positive, got: {}",
            name,
            f
        );
    }
    assert!(
        f1 < f2 && f2 < f3,
        "frequencies must be strictly ascending: f1={} f2={} f3={}",
        f1,
        f2,
        f3
    );

    // (f) The three VERTICAL (Z-dominant) bending modes each within the P2 2%
    //     band of their analytic (nπ)² values (f₁ ≈ 115.9, f₂ ≈ 463.4,
    //     f₃ ≈ 1042.8 Hz). The lateral Y-bending mode intrudes between vertical
    //     modes 2 and 3 in the raw spectrum (see (e)), so the vertical family is
    //     selected by eigenvector dominant-axis classification (shape energy
    //     along Z ≥ along X and Y) over the result's modes list — exactly as the
    //     kernel benchmark does. P2 resolves all three uniformly; RED at P1
    //     (~+8.5% / +8.2% / +7.1%, before the fixture runs at element_order = P2).
    let vertical = z_dominant_frequencies(result_val);
    eprintln!("[modal ss] vertical (Z-dominant) family: {:?}", vertical);
    assert!(
        vertical.len() >= 3,
        "need ≥3 vertical (Z-dominant) bending modes in the spectrum, found {}: {:?}",
        vertical.len(),
        vertical
    );
    assert!(
        vertical[0] < vertical[1] && vertical[1] < vertical[2],
        "vertical frequencies must be strictly ascending: {:?}",
        &vertical[..3]
    );
    for (i, (&f, &f_analytic)) in vertical
        .iter()
        .zip([f1_analytic, f2_analytic, f3_analytic].iter())
        .take(3)
        .enumerate()
    {
        let err = (f - f_analytic).abs() / f_analytic;
        assert!(
            err < SS_P2_REL_TOL,
            "ss vertical mode {} = {:.3} Hz, analytic = {:.3} Hz, rel_err = {:.2}% > {:.2}% (P2 band)",
            i + 1,
            f,
            f_analytic,
            err * 100.0,
            SS_P2_REL_TOL * 100.0
        );
    }
}

// ── task μ: printer-gantry dogfood — 5-mode structural gate ──────────────────
//
// The printer-gantry fixture (examples/modal/printer_gantry_modes.ri) models a
// 500×60×40 mm Aluminium_6061_T6 crossbeam pinned at both ends (x_min and
// x_max) with two `PinnedSupport`s — a crossbeam that RESTS ON its end mounts —
// requesting the first 5 natural frequencies. This is the 4th fixture
// in the modal_analysis_e2e CI gate (PRD docs/prds/v0_3/modal-analysis.md §1).
//
// The user-observable signal is "runs end-to-end and prints the first 5 modes
// of the printer-build gantry." PRD §1 specifies NO analytic accuracy bound for
// the gantry (unlike the cantilever/SS 2% bands), so this test asserts
// STRUCTURAL properties only:
//   (a) no Error-severity diagnostics after parse + eval
//   (b) a ComputeNode with target == "modal::free_vibration" in the graph
//   (c) the `result` cell is a non-Undef StructureInstance/Map
//   (d) cells f1..f5 are each finite, positive, and strictly ascending
//       (the asymmetric 60×40 mm cross-section keeps the vertical/lateral
//       bending families non-degenerate, so strict ordering holds robustly)
//
// The two-mount (x_min + x_max) pin-pin realization in the trampoline removes
// all 6 rigid-body modes so K_free is non-singular and the 5 lowest modes are
// real, positive, and distinct. That realization is selected because both end
// faces are named by PINNED supports (task 6663 made the discriminator read the
// support KIND per face; before it, the fixture's two `FixedSupport`s reached
// the same branch by face name alone). No analytic tolerance is asserted — the mesh
// density is not validated for this cross-section, so any threshold would be
// a guessed/unvalidated number (the false-premise trap).
//
// Release-gated like the other e2e solves (heavy generalized eigensolve).
// The registration pin (_seam_pin, step-13) runs always.
//
// The fixture (examples/modal/printer_gantry_modes.ri) was created in the same
// diff, so this test is GREEN as landed (include_str! compile-fail is resolved).

/// Printer gantry: first 5 modes finite, positive, strictly ascending —
/// the dogfood structural gate (task μ, PRD §1).
#[cfg_attr(debug_assertions, ignore = "heavy modal solve; release-only")]
#[test]
fn e2e_printer_gantry_prints_five_modes() {
    let source = printer_gantry_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // (a) No Error-severity diagnostics.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    // (b) A ComputeNode with target == "modal::free_vibration" must be present.
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let has_compute_node = snapshot
        .graph
        .compute_nodes
        .iter()
        .any(|(_, data)| data.target == "modal::free_vibration");
    assert!(
        has_compute_node,
        "expected a ComputeNode with target==\"modal::free_vibration\"; found targets: {:?}",
        snapshot
            .graph
            .compute_nodes
            .iter()
            .map(|(_, d)| d.target.as_str())
            .collect::<Vec<_>>()
    );

    // (c) The `result` cell must hold a non-Undef StructureInstance/Map.
    let result_cell = ValueCellId::new("PrinterGantryModes", "result");
    let result_val = eval_result
        .values
        .get(&result_cell)
        .unwrap_or_else(|| panic!("cell PrinterGantryModes.result not found in eval result"));
    assert!(
        matches!(result_val, Value::StructureInstance(_) | Value::Map(_)),
        "expected result to be StructureInstance or Map (NOT Undef), got: {:?}",
        result_val
    );

    // (d) f1..f5 are each finite, positive, and strictly ascending.
    //     An asymmetric cross-section (width=60mm ≠ height=40mm) keeps the
    //     vertical/lateral bending families non-degenerate so the strict
    //     ordering holds without a knife-edge tie on degenerate modes.
    let read_cell = |name: &str| -> f64 {
        read_frequency(
            eval_result
                .values
                .get(&ValueCellId::new("PrinterGantryModes", name))
                .unwrap_or_else(|| {
                    panic!("cell PrinterGantryModes.{name} not found in eval result")
                }),
        )
    };
    let f1 = read_cell("f1");
    let f2 = read_cell("f2");
    let f3 = read_cell("f3");
    let f4 = read_cell("f4");
    let f5 = read_cell("f5");

    eprintln!(
        "[modal gantry] f1={:.3} f2={:.3} f3={:.3} f4={:.3} f5={:.3} Hz",
        f1, f2, f3, f4, f5
    );

    for (name, f) in [("f1", f1), ("f2", f2), ("f3", f3), ("f4", f4), ("f5", f5)] {
        assert!(
            f.is_finite() && f > 0.0,
            "{name} must be finite and positive, got: {f}"
        );
    }

    // Rigid-body-leak guard: the pin-pin BCs (x_min + x_max) must remove all
    // 6 rigid-body modes so K_free is non-singular.  A leaked rigid-body mode
    // would surface near zero (e.g. ~1e-3 Hz) and still pass `f > 0.0`.
    // A real structural fundamental for this gantry geometry is in the
    // hundreds-of-Hz range; the 1 Hz floor separates a genuine structural
    // mode from a spurious residual.  This is NOT an analytic accuracy bound —
    // it only guards against incomplete BC removal.
    assert!(
        f1 > 1.0,
        "f1 rigid-body-leak guard: expected > 1.0 Hz (structural mode), got {f1:.6} Hz \
         — suggests a leaked near-zero rigid-body mode from incomplete pin-pin BC removal"
    );

    assert!(
        f1 < f2 && f2 < f3 && f3 < f4 && f4 < f5,
        "gantry frequencies must be strictly ascending: f1={f1} f2={f2} f3={f3} f4={f4} f5={f5}"
    );
}

// ── task 6663: support KIND must drive the BC realization ────────────────────
//
// The headline acceptance test. One fixture
// (tests/fixtures/clamped_clamped_beam_modes.ri), one eval, three modal solves
// over the SAME 800 × 44.588 × 44.588 mm section differing ONLY in support kind:
//   • `[FixedSupport("x_min"), FixedSupport("x_max")]`   → clamped-clamped
//   • `[PinnedSupport("x_min"), PinnedSupport("x_max")]` → pinned-pinned
//   • `[FixedSupport("x_min"), PinnedSupport("x_max")]`  → propped cantilever
//
// Every band is read on the VERTICAL (Z-dominant) family, selected by
// eigenvector dominant-axis energy — never on the raw `first_frequency`. The
// square section makes each configuration's vertical and lateral bending
// families near-degenerate (pinned 391.05 lateral / 395.22 vertical; fixed
// 887.55 vertical / 890.90 lateral) and the MIXED one splits them 4.4×, so the
// raw fundamental is the lateral mode in at least two of the three. The fixture
// requests `n_modes: 4` so that selection has headroom rather than picking out
// of a window holding exactly one near-degenerate pair (see the fixture's own
// "WHY n_modes: 4" note). MEASURED headroom at 4 modes: every configuration's
// Z-dominant family now has TWO members, not one — pinned [395.22, 1558.34],
// fixed [887.55, 2384.63], propped [616.92, 1955.21] — and the four low modes
// are bit-identical to the `n_modes: 2` run, so the extra window costs nothing
// numerically (the shift-invert Krylov window is 64 either way). The raw cells
// are still read and guarded finite — that keeps the `first_frequency` builtin
// exercised — and (h) asserts on one.
//
// Signals asserted:
//   (a) no Error-severity diagnostics after parse + eval
//   (b) a ComputeNode with target == "modal::free_vibration" in the graph
//   (c) f1z_pinned within CC_FIXTURE_PINNED_REL_TOL of the SS analytic 397.33 Hz
//       — an ACCURACY band on the pinned vertical mode. NOT the bit-preservation
//       guard, which is (i); see CC_FIXTURE_PINNED_REL_TOL's own doc for why the
//       two were conflated and what that cost
//   (d) f1z_fixed within CC_FIXTURE_FIXED_REL_TOL of the CC analytic 900.699 Hz
//   (e) f1z_fixed / f1z_pinned ≥ CC_FIXTURE_MIN_FIXED_PINNED_RATIO — the task's
//       literal "they must DIFFER" acceptance
//   (f) the mixed pair's vertical fundamental within CC_FIXTURE_PROPPED_REL_TOL
//       of the CP analytic 620.702 Hz — the task's SCOPE EXTENSION, that
//       honoring the kind PER FACE makes the mixed pair a genuine propped
//       cantilever on its OWN analytic
//   (g) the ordering within that family: pinned < propped < fixed, strictly —
//       the BC-stiffness ordering λ²: 9.8696 < 15.4182 < 22.3733, which no
//       single band can express
//   (h) the mixed pair's RAW fundamental sits strictly below its vertical one —
//       the measurable form of "a lateral mode intrudes here", which is what
//       forces the whole test onto the vertical family
//
// NOT asserted here (amendment, review suggestion 2): bit-preservation of the
// pin-pin Dirichlet set. A former signal (i) pinned the pinned configuration's
// RAW `first_frequency` to 391.0495 Hz ± 0.5% and claimed to be that guard; it
// was not, because that number also depends on the derived mesh, the P2
// promotion, the assembly and the shift-invert tolerance, so any legitimate
// improvement in any of them would have reported "the pin-pin Dirichlet set must
// be unchanged". The property IS asserted — structurally and exactly, on the DOF
// sets — by the unit tests `CC_FIXTURE_PINNED_REL_TOL`'s doc names.
//
// Why (f)–(h) live in THIS test rather than a sibling: all three solves come
// from one eval of one fixture, so a sibling test would re-run the two heavy
// solves (c)/(d) already cover just to reach the third. The name still describes
// the headline clause; (f)–(h) are the mixed-pair extension riding the same eval.
//
// RED before the fix: `build_dirichlet_bcs` discriminated on target face NAMES
// only and never read the support kind, so ALL THREE solves returned the
// bit-identical pinned answer (391.049 Hz measured). (d) missed its band by 2.3×;
// (e) read 1.0; (f) missed its band by 36%; (g) read pinned == propped.
//
// Release-gated like every other heavy modal e2e in this file.
//
// GATE COST (amendment, review suggestion 6): **MEASURED 21.7 s** in release on
// this branch (`test result: ok … finished in 21.70s`, this test alone,
// sequential). One eval drives THREE P2 shift-invert modal solves over the same
// 1526-node P1 / ~10k-node P2 mesh — the pinned, fixed and mixed configurations
// — and 21.7 s is the total for all three plus parse, compile and eval. Recorded
// here for the same reason the kernel benchmarks record theirs next to their
// meshes: this test is release-only, so its whole cost lands on the merge gate,
// and a future mesh or `n_modes` bump should be costed against a measurement
// rather than guessed. Sibling reference point: the clamped-clamped kernel
// benchmark (`reify-solver-elastic/tests/modal_benchmarks.rs`) measures 38.8 s
// on its own, for ONE dense QZ solve at n_free = 1269 — the shift-invert path
// this test takes is why three larger solves come in cheaper than one dense one.
// Both make a clamped-clamped accuracy statement against βL = 4.730041 and
// neither subsumes the other; `CLAMPED_P2_REL_TOL`'s doc there gives the three
// independent axes (solver path, fixture slenderness regime, layer) and is the
// place to argue from if either is ever proposed for removal.

/// Two `FixedSupport`s must give the clamped-clamped answer, not the
/// pinned-pinned one — and a mixed `[Fixed, Pinned]` pair must give the
/// propped-cantilever answer, distinct from both (task 6663).
#[cfg_attr(debug_assertions, ignore = "heavy modal solve; release-only")]
#[test]
fn e2e_two_fixed_supports_are_clamped_clamped_not_simply_supported() {
    let source = clamped_clamped_source();
    let compiled = parse_and_compile_with_stdlib(source);

    let mut engine = make_simple_engine();
    reify_eval::compute_targets::register_compute_fns(&mut engine);

    let eval_result = engine.eval(&compiled);

    // (a) No Error-severity diagnostics.
    let errors: Vec<_> = eval_result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "expected no Error diagnostics, got: {:?}",
        errors
    );

    // (b) A ComputeNode with target == "modal::free_vibration" must be present.
    let snapshot = engine
        .eval_state()
        .expect("eval_state must be Some after eval()")
        .snapshot
        .clone();
    let has_compute_node = snapshot
        .graph
        .compute_nodes
        .iter()
        .any(|(_, data)| data.target == "modal::free_vibration");
    assert!(
        has_compute_node,
        "expected a ComputeNode with target==\"modal::free_vibration\"; found targets: {:?}",
        snapshot
            .graph
            .compute_nodes
            .iter()
            .map(|(_, d)| d.target.as_str())
            .collect::<Vec<_>>()
    );

    // Every `result` cell must hold a non-Undef StructureInstance/Map.
    for name in ["result_fixed", "result_pinned", "result_propped"] {
        let val = eval_result
            .values
            .get(&ValueCellId::new("ClampedClampedBeamModes", name))
            .unwrap_or_else(|| {
                panic!("cell ClampedClampedBeamModes.{name} not found in eval result")
            });
        assert!(
            matches!(val, Value::StructureInstance(_) | Value::Map(_)),
            "expected {name} to be StructureInstance or Map (NOT Undef), got: {val:?}"
        );
    }

    let read_cell = |name: &str| -> f64 {
        read_frequency(
            eval_result
                .values
                .get(&ValueCellId::new("ClampedClampedBeamModes", name))
                .unwrap_or_else(|| {
                    panic!("cell ClampedClampedBeamModes.{name} not found in eval result")
                }),
        )
    };
    let f1_fixed = read_cell("f1_fixed");
    let f1_pinned = read_cell("f1_pinned");
    let f1_propped = read_cell("f1_propped");

    // The VERTICAL (Z-dominant) bending family of each configuration, selected by
    // eigenvector dominant-axis classification — the same selection the
    // simply-supported e2e above and the kernel benchmark already use, and for
    // the same reason: the raw frequency-sorted spectrum interleaves LATERAL
    // (Y-bending) modes, so a raw mode index does not map onto the (βL)² family
    // the analytic references describe. Here the square section makes the two
    // directions near-degenerate for the two symmetric configurations, but NOT
    // for the mixed one — see the (f)/(g) commentary below.
    let vertical_family = |name: &str| -> Vec<f64> {
        let val = eval_result
            .values
            .get(&ValueCellId::new("ClampedClampedBeamModes", name))
            .unwrap_or_else(|| {
                panic!("cell ClampedClampedBeamModes.{name} not found in eval result")
            });
        for (i, (f, p)) in modes_freq_participation(val).iter().enumerate() {
            eprintln!("[modal bc-kind]   {name} mode {i}: f={f:.4} Hz, participation_z={p:.6e}");
        }
        let vertical = z_dominant_frequencies(val);
        eprintln!("[modal bc-kind]   {name} vertical (Z-dominant) family: {vertical:?}");
        assert!(
            !vertical.is_empty(),
            "{name} has no Z-dominant (vertical bending) mode in its {}-mode spectrum: {:?}",
            modes_freq_participation(val).len(),
            modes_freq_participation(val)
        );
        vertical
    };

    // Selected ONCE per configuration — the closure prints the full spectrum as
    // a side effect, so calling it twice would double the log.
    let vertical_pinned = vertical_family("result_pinned");
    let vertical_fixed = vertical_family("result_fixed");
    let vertical_propped = vertical_family("result_propped");
    let f1z_pinned = vertical_pinned[0];
    let f1z_fixed = vertical_fixed[0];
    let f1z_propped = vertical_propped[0];

    // The three Euler–Bernoulli references, COMPUTED from the fixture's own
    // section (amendment, review suggestion 3) rather than transcribed as
    // decimal literals. `CC_FIXTURE_SECTION` is the single place this file
    // spells the `.ri` fixture's geometry and material, so each band below names
    // only its βL family; edit the fixture and these move with it instead of
    // silently referring to a beam that no longer exists.
    let cc_pinned_analytic_hz = analytic_beam_frequency_for(CC_BETA_L_PINNED, &CC_FIXTURE_SECTION);
    let cc_fixed_analytic_hz = analytic_beam_frequency_for(CC_BETA_L_FIXED, &CC_FIXTURE_SECTION);
    let cc_propped_analytic_hz =
        analytic_beam_frequency_for(CC_BETA_L_PROPPED, &CC_FIXTURE_SECTION);
    eprintln!(
        "[modal bc-kind] analytic references from CC_FIXTURE_SECTION \
         (L={:.4} m, b={:.6} m, h={:.6} m, E={:.4e} Pa, ρ={:.2} kg/m³): \
         pinned {:.4} Hz, propped {:.4} Hz, fixed {:.4} Hz",
        CC_FIXTURE_SECTION.l,
        CC_FIXTURE_SECTION.b,
        CC_FIXTURE_SECTION.h,
        CC_FIXTURE_SECTION.e,
        CC_FIXTURE_SECTION.rho,
        cc_pinned_analytic_hz,
        cc_propped_analytic_hz,
        cc_fixed_analytic_hz,
    );

    eprintln!(
        "[modal bc-kind] f1z_pinned={:.4} Hz (analytic {:.3}, err {:+.2}%) [raw f1 = {:.4} Hz]",
        f1z_pinned,
        cc_pinned_analytic_hz,
        (f1z_pinned - cc_pinned_analytic_hz) / cc_pinned_analytic_hz * 100.0,
        f1_pinned
    );
    eprintln!(
        "[modal bc-kind] f1z_fixed ={:.4} Hz (analytic {:.3}, err {:+.2}%) [raw f1 = {:.4} Hz]",
        f1z_fixed,
        cc_fixed_analytic_hz,
        (f1z_fixed - cc_fixed_analytic_hz) / cc_fixed_analytic_hz * 100.0,
        f1_fixed
    );
    // NOT compared to an analytic here: the mixed configuration's RAW fundamental
    // is a lateral mode, not the propped bending mode — see (f)/(h).
    eprintln!("[modal bc-kind] f1_propped (raw fundamental) = {f1_propped:.4} Hz");
    eprintln!(
        "[modal bc-kind] ratio f1z_fixed/f1z_pinned = {:.4} (analytic 2.267)",
        f1z_fixed / f1z_pinned
    );

    // The raw `first_frequency` cells are still read and still guarded — that
    // keeps the DSL builtin exercised end-to-end — but the BANDS below are read
    // on the vertical family, for the reason (f) documents.
    for (name, f) in [
        ("f1_fixed", f1_fixed),
        ("f1_pinned", f1_pinned),
        ("f1_propped", f1_propped),
    ] {
        assert!(
            f.is_finite() && f > 0.0,
            "{name} must be finite and positive, got: {f}"
        );
    }

    // (c) Pinned-pinned is unchanged by this task — the accuracy band.
    //
    // Read on the VERTICAL family, not on `first_frequency`. The square section
    // makes this configuration's two bending directions near-degenerate (391.05
    // lateral / 395.22 vertical), and the raw fundamental is the LATERAL one —
    // so comparing it to the pinned-pinned BENDING analytic only worked because
    // the pair happens to sit 1.1% apart. Every band in this test now reads the
    // same family the (f)/(g) clauses do.
    let pinned_err =
        (f1z_pinned - cc_pinned_analytic_hz).abs() / cc_pinned_analytic_hz;
    assert!(
        pinned_err < CC_FIXTURE_PINNED_REL_TOL,
        "f1z_pinned = {:.4} Hz, analytic simply-supported = {:.3} Hz, rel_err = {:.2}% > {:.2}% \
         — the pinned-pinned realization must be unchanged by the support-kind fix",
        f1z_pinned,
        cc_pinned_analytic_hz,
        pinned_err * 100.0,
        CC_FIXTURE_PINNED_REL_TOL * 100.0
    );

    // (d) Clamped-clamped lands on the clamped-clamped analytic, not the pinned
    //     one. Same family as (c), for the same reason (887.55 vertical / 890.90
    //     lateral is a 0.4% split — thinner still than the pinned pair's).
    let fixed_err = (f1z_fixed - cc_fixed_analytic_hz).abs() / cc_fixed_analytic_hz;
    assert!(
        fixed_err < CC_FIXTURE_FIXED_REL_TOL,
        "f1z_fixed = {:.4} Hz, analytic clamped-clamped = {:.3} Hz, rel_err = {:.2}% > {:.2}% \
         — two FixedSupports must clamp BOTH end faces, not degrade to the pinned-pinned answer \
         ({:.3} Hz)",
        f1z_fixed,
        cc_fixed_analytic_hz,
        fixed_err * 100.0,
        CC_FIXTURE_FIXED_REL_TOL * 100.0,
        cc_pinned_analytic_hz
    );

    // (e) The two configurations must genuinely DIFFER — the task's acceptance.
    //     Taken WITHIN the vertical family so the ratio is one physical quantity
    //     under two BC sets (the same argument (g) makes); under the defect both
    //     families were bit-identical, so it read 1.0 either way.
    let ratio = f1z_fixed / f1z_pinned;
    assert!(
        ratio >= CC_FIXTURE_MIN_FIXED_PINNED_RATIO,
        "f1z_fixed/f1z_pinned = {:.4} (f1z_fixed={:.4} Hz, f1z_pinned={:.4} Hz), expected ≥ {:.2} \
         (analytic BC ratio 2.267) — a ratio near 1.0 means the support KIND was ignored and \
         both solves realized the same Dirichlet set",
        ratio,
        f1z_fixed,
        f1z_pinned,
        CC_FIXTURE_MIN_FIXED_PINNED_RATIO
    );

    // (f) The MIXED pair lands on the clamped-pinned analytic — the scope
    // extension. Honoring the kind PER FACE means x_min clamps all three
    // translational DOFs while x_max pins Z only, i.e. a propped cantilever with
    // its own (βL)² = 15.4182, not a collapse onto either sibling.
    //
    // Asserted on the VERTICAL (Z-dominant) family, not on `first_frequency`,
    // and MEASUREMENT says why: the mixed configuration's raw fundamental is
    // 141.6988 Hz with participation_z = 1.4e-6 — a LATERAL (Y-bending)
    // cantilever mode, matching the analytic Y-cantilever 141.53 Hz to +0.12%.
    // It is a real mode of a correctly realized structure: `PinTransverse` pins
    // Z across x_max (the beam idealization `simply_supported_pin_pin_bcs`
    // documents), so the lateral direction sees a clamped root and a free tip and
    // is genuinely soft. The two symmetric configurations hide this because a
    // square section makes their two directions near-degenerate (pinned: 391.05
    // lateral / 395.22 vertical; fixed: 887.55 vertical / 890.90 lateral), while
    // the mixed one splits them 4.4×. Selecting the vertical family is the same
    // move — and the same helper — the simply-supported e2e above already makes
    // for the same reason.
    eprintln!(
        "[modal bc-kind] f1z_propped={:.4} Hz (analytic {:.3}, err {:+.2}%)",
        f1z_propped,
        cc_propped_analytic_hz,
        (f1z_propped - cc_propped_analytic_hz) / cc_propped_analytic_hz * 100.0
    );
    let propped_err =
        (f1z_propped - cc_propped_analytic_hz).abs() / cc_propped_analytic_hz;
    assert!(
        propped_err < CC_FIXTURE_PROPPED_REL_TOL,
        "f1z_propped = {:.4} Hz, analytic clamped-pinned = {:.3} Hz, rel_err = {:.2}% > {:.2}% \
         — [FixedSupport(x_min), PinnedSupport(x_max)] must be a genuine propped cantilever, \
         not the pinned-pinned ({:.3} Hz) or clamped-clamped ({:.3} Hz) answer",
        f1z_propped,
        cc_propped_analytic_hz,
        propped_err * 100.0,
        CC_FIXTURE_PROPPED_REL_TOL * 100.0,
        cc_pinned_analytic_hz,
        cc_fixed_analytic_hz
    );

    // (g) BC stiffness must ORDER strictly: pin-pin < propped < clamp-clamp,
    // mirroring λ² = 9.8696 < 15.4182 < 22.3733. Compared WITHIN the vertical
    // family, so the three numbers are the same physical quantity under three
    // BC sets; the solves share one section, one mesh and one material, so
    // everything but the Dirichlet realization cancels. A tie anywhere means two
    // configurations realized the same set — the defect, reached through
    // whichever pair happens to collide. (MEASURED: 395.22 < 616.92 < 887.55.)
    assert!(
        f1z_pinned < f1z_propped && f1z_propped < f1z_fixed,
        "expected strict BC-stiffness ordering pinned < propped < fixed within the vertical \
         family, got f1z_pinned={:.4} Hz, f1z_propped={:.4} Hz, f1z_fixed={:.4} Hz — clamping \
         one end of a simply-supported beam must stiffen it, and replacing the far clamp with \
         a prop must soften it",
        f1z_pinned,
        f1z_propped,
        f1z_fixed
    );

    // (h) Guard the selection itself: the mixed configuration's raw fundamental
    // must be STRICTLY below its vertical fundamental. That is the measurable
    // form of "the lateral mode intrudes here", so a future reader who reverts
    // (f) to `first_frequency` gets a failure that explains itself rather than a
    // band that quietly stops meaning what it says. No magic number — the two
    // are read from the same solve.
    assert!(
        f1_propped < f1z_propped,
        "expected the mixed configuration's raw fundamental ({:.4} Hz) to sit strictly below \
         its vertical fundamental ({:.4} Hz) — the lateral cantilever mode is why (f) selects \
         the Z-dominant family instead of first_frequency; if these have converged, re-derive \
         the selection rather than assuming first_frequency is now the bending mode",
        f1_propped,
        f1z_propped
    );
}
