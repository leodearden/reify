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

/// Read a frequency cell (Hz) as `f64`, tolerating the `Real` placeholder
/// (`Mode.frequency : Real`, modal_analysis.ri) or a dimensioned `Scalar`.
fn read_frequency(val: &Value) -> f64 {
    match val {
        Value::Real(r) => *r,
        Value::Scalar { si_value, .. } => *si_value,
        other => panic!("expected a frequency Real/Scalar, got: {:?}", other),
    }
}

/// Analytic Euler–Bernoulli fundamental frequency (Hz) for the shared
/// 200×10×2 mm AISI 1045 steel beam, given the dimensionless eigen-coefficient
/// `beta_l` (β·L): cantilever uses 1.875, simply-supported mode `n` uses `n·π`.
///
///   fₙ = (βL)² / (2π) · √( E·I / (ρ·A·L⁴) )
///
/// E = 205 GPa, ρ = 7850 kg/m³, L = 0.2 m, b = 0.01 m, h = 0.002 m,
/// I = b·h³/12 (bending about Y, deflection in Z), A = b·h.
fn analytic_beam_frequency(beta_l: f64) -> f64 {
    use std::f64::consts::PI;
    let e: f64 = 205.0e9;
    let rho: f64 = 7850.0;
    let l: f64 = 0.2;
    let b: f64 = 0.01;
    let h: f64 = 0.002;
    let i: f64 = b * h.powi(3) / 12.0;
    let a: f64 = b * h;
    beta_l.powi(2) / (2.0 * PI) * (e * i / (rho * a * l.powi(4))).sqrt()
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
        other => panic!("expected ModalResult StructureInstance/Map, got: {:?}", other),
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
        Value::Scalar { si_value, dimension } => {
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
// PINS BOTH end faces (x_min and x_max). Five observable signals:
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
// BC realization (build_dirichlet_bcs → simply_supported_pin_pin_bcs): the two
// FixedSupports targeting x_min AND x_max select the pin-pin branch — pin ONLY
// the transverse Z DOF on both end faces (the bending rotation dw/dx stays free,
// carried by the axial u(z)) + minimal axial/lateral anchors at the two end-face
// neutral-axis nodes (z = h/2). This yields the (nπ)² simply-supported family
// rather than the fixed-fixed family the all-DOF clamp would produce. Selection
// is by coordinate, so it catches the P2 edge-midpoint nodes once the trampoline
// promotes the mesh.
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
// x_max), requesting the first 5 natural frequencies. This is the 4th fixture
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
// real, positive, and distinct. No analytic tolerance is asserted — the mesh
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
