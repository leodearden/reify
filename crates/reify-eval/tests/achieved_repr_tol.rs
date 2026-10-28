//! End-to-end tests for `Engine::achieved_repr_tol` — the sampled max
//! facet-chord deviation metric per realized output subject
//! (Determinacy β, task 4198).
//!
//! Tests the full recording pipeline:
//!   parse → compile → `tessellate_realizations` → `engine.achieved_repr_tol(occ)`
//!
//! All kernel-backed assertions are guarded by
//! `reify_kernel_occt::OCCT_AVAILABLE` and skip cleanly when OCCT is not
//! present.
//!
//! # Invariants under test
//!
//! - **B1** (planar box): `achieved_repr_tol` ≤ 1e-5 m.
//! - **B2** (curved sphere, end-to-end monotone): coarse deviation > fine
//!   deviation strictly.
//! - **B3** (honest absence): unknown occurrence name → `None`;
//!   a never-realized subject is never `0.0`.

use reify_core::ModulePath;

// ── helpers ──────────────────────────────────────────────────────────────────

/// Compile a DSL source string through the Reify compiler, asserting no
/// error-severity diagnostics. Returns the `CompiledModule`.
fn compile_no_errors(source: &str, path_name: &str) -> reify_compiler::CompiledModule {
    use reify_core::Severity;
    let parsed = reify_syntax::parse(source, ModulePath::single(path_name));
    assert!(
        parsed.errors.is_empty(),
        "parse errors in {path_name}: {:?}",
        parsed.errors
    );
    let compiled = reify_compiler::compile(&parsed);
    let errors: Vec<_> = compiled
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "compile errors in {path_name}: {:#?}",
        errors
    );
    compiled
}

/// Build a fresh `Engine` backed by a real OCCT kernel.
///
/// Registers `OcctKernelHandle` directly as the `Box<dyn GeometryKernel>`
/// (mirroring the production path from `reify_kernel_occt::register::factory`,
/// which inserts `OcctKernelHandle` directly into the engine's kernel map).
///
/// **Why not `SingleKernelHolder`**: `SingleKernelHolder` is a wrapper that
/// delegates most `GeometryKernel` methods to its inner kernel, but it does
/// NOT override optional capability methods (e.g. `measure_mesh_deviation`)
/// that default to `None` in the trait. Using `OcctKernelHandle` directly
/// ensures the `measure_mesh_deviation` override is reachable through the
/// `&dyn GeometryKernel` call site in `surface_subtree`.
fn make_occt_engine() -> reify_eval::Engine {
    let checker = reify_constraints::SimpleConstraintChecker;
    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    reify_eval::Engine::new(reify_eval::SolverPosture::SolverFree, Box::new(checker), Some(Box::new(kernel)))
}

// ── B2 end-to-end: sphere coarse > fine ─────────────────────────────────────

/// B2 end-to-end: tessellating a 1 m-radius sphere at a COARSE precision
/// (50 mm) gives a strictly higher `achieved_repr_tol` than at FINE precision
/// (0.5 mm).
///
/// The `#precision` pragma sets `module.default_tolerance`, which drives the
/// per-realization tessellation budget via `compute_tessellation_budgets`.
///
/// # Anti-circularity note
///
/// `measure_mesh_deviation` receives no tolerance argument — it cannot echo
/// the configured deflection (PRD §8.3 / task CRITICAL). The measured values
/// here reflect actual facet-chord error, not the configured budget.
#[test]
fn b2_sphere_coarse_greater_than_fine() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping b2_sphere_coarse_greater_than_fine: OCCT not available");
        return;
    }

    // Coarse: 50 mm deflection (5e-2 m)
    let coarse_src = r#"
#precision(50mm)
structure Sphere {
    let r = sphere(1000mm)
}
"#;
    // Fine: 0.5 mm deflection (5e-4 m)
    let fine_src = r#"
#precision(0.5mm)
structure Sphere {
    let r = sphere(1000mm)
}
"#;

    let coarse_compiled = compile_no_errors(coarse_src, "sphere_coarse");
    let fine_compiled = compile_no_errors(fine_src, "sphere_fine");

    // --- Coarse engine ---
    let mut coarse_engine = make_occt_engine();
    coarse_engine.set_capture_repr_tol(true);
    coarse_engine.tessellate_realizations(&coarse_compiled);
    let coarse_dev = coarse_engine
        .achieved_repr_tol("Sphere#realization[0]")
        .expect(
            "B2: coarse sphere should have Some achieved_repr_tol after tessellate_realizations",
        );

    // --- Fine engine ---
    let mut fine_engine = make_occt_engine();
    fine_engine.set_capture_repr_tol(true);
    fine_engine.tessellate_realizations(&fine_compiled);
    let fine_dev = fine_engine
        .achieved_repr_tol("Sphere#realization[0]")
        .expect("B2: fine sphere should have Some achieved_repr_tol after tessellate_realizations");

    // B3-numeric: both values must be finite and ≥ 0.
    assert!(
        coarse_dev.is_finite() && coarse_dev >= 0.0,
        "B3-numeric: coarse deviation must be finite ≥ 0, got {coarse_dev}"
    );
    assert!(
        fine_dev.is_finite() && fine_dev >= 0.0,
        "B3-numeric: fine deviation must be finite ≥ 0, got {fine_dev}"
    );

    // B2: OCCT linear-deflection chord bound ⇒ coarser tessellation ⇒ larger
    // deviation. The deflections here are 100× apart, so the inequality is
    // robust regardless of sphere radius.
    assert!(
        coarse_dev > fine_dev,
        "B2 end-to-end: coarse sphere achieved_repr_tol ({coarse_dev:.3e} m) must \
         be strictly greater than fine ({fine_dev:.3e} m)"
    );
}

// ── B1 end-to-end: box ≤ 1e-5 m ─────────────────────────────────────────────

/// B1 end-to-end: a 1 m³ axis-aligned box achieves near-zero facet-chord
/// deviation because all faces are planar. Interior sample points are exact
/// convex combinations of coplanar f32 vertices; projected distance =
/// pure f32 quantization (~1e-6 m at unit scale).
#[test]
fn b1_box_achieved_tol_near_zero() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping b1_box_achieved_tol_near_zero: OCCT not available");
        return;
    }

    // Use a 10 mm deflection so tessellation runs; the exact deflection
    // value does not affect the near-zero result (planar faces always give ~0).
    let src = r#"
#precision(10mm)
structure Box {
    let r = box(1000mm, 1000mm, 1000mm)
}
"#;
    let compiled = compile_no_errors(src, "box_b1");
    let mut engine = make_occt_engine();
    engine.set_capture_repr_tol(true);
    engine.tessellate_realizations(&compiled);

    let dev = engine
        .achieved_repr_tol("Box#realization[0]")
        .expect("B1: box should have Some achieved_repr_tol after tessellate_realizations");

    assert!(
        dev >= 0.0,
        "B3-numeric: box deviation must be ≥ 0, got {dev}"
    );
    assert!(
        dev.is_finite(),
        "B3-numeric: box deviation must be finite, got {dev}"
    );
    assert!(
        dev <= 1e-5,
        "B1 end-to-end: box (planar faces) must have achieved_repr_tol ≤ 1e-5 m, \
         got {dev:.3e} m"
    );
}

// ── B3: unknown occurrence → None ────────────────────────────────────────────

/// B3 end-to-end: a never-realized or unknown occurrence name yields `None`.
///
/// This test is **GREEN immediately** (pre-2 leaves the map empty, so every
/// key lookup returns `None`) and must remain GREEN after step-6 (recording
/// only populates entries for successfully tessellated subjects).
#[test]
fn b3_unknown_occurrence_yields_none() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!("skipping b3_unknown_occurrence_yields_none: OCCT not available");
        return;
    }

    let src = r#"
structure S {
    let r = sphere(500mm)
}
"#;
    let compiled = compile_no_errors(src, "b3_absent");
    let mut engine = make_occt_engine();
    engine.tessellate_realizations(&compiled);

    // A name that was never realized — must return None (honest absence, B3).
    assert!(
        engine.achieved_repr_tol("not_a_real_occurrence").is_none(),
        "B3: completely unknown occurrence must return None"
    );
    // An out-of-range realization index — also None.
    assert!(
        engine.achieved_repr_tol("S#realization[999]").is_none(),
        "B3: out-of-range realization index must return None"
    );
}

// ── enumeration: every recorded deviation, key order, bit-exact ─────────────

/// `achieved_repr_tol_by_occurrence` is the enumerating view of the same map
/// the point lookup reads — what `reify check --verbose` prints. It walks
/// occurrences in ascending key order, hands back each value bit-identical to
/// what was recorded, and yields nothing when nothing was measured (B3 honest
/// absence). OCCT-independent: the map is injected through the
/// test-instrumentation seam.
#[test]
fn by_occurrence_enumerates_every_recorded_deviation_in_key_order_bit_exact() {
    let make_engine = || {
        reify_eval::Engine::new(
            reify_eval::SolverPosture::SolverFree,
            Box::new(reify_constraints::SimpleConstraintChecker),
            None,
        )
    };
    let injected = std::collections::BTreeMap::from([
        ("Zeta#realization[0]".to_string(), 1.19549876543e-4_f64),
        ("Alpha#realization[1]".to_string(), 6.2e-4_f64),
    ]);

    let mut engine = make_engine();
    engine.set_achieved_repr_tol_for_test(injected.clone());
    let enumerated: Vec<(String, f64)> = engine
        .achieved_repr_tol_by_occurrence()
        .map(|(occurrence, deviation)| (occurrence.to_string(), deviation))
        .collect();

    let keys: Vec<&str> = enumerated.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["Alpha#realization[1]", "Zeta#realization[0]"]);
    for (occurrence, deviation) in &enumerated {
        assert_eq!(
            deviation.to_bits(),
            injected[occurrence].to_bits(),
            "{occurrence}: enumerated value must be bit-identical to the recorded one"
        );
        assert_eq!(
            engine.achieved_repr_tol(occurrence).map(f64::to_bits),
            Some(deviation.to_bits()),
            "{occurrence}: enumeration must agree with the point lookup"
        );
    }

    let unmeasured = make_engine();
    assert_eq!(
        unmeasured.achieved_repr_tol_by_occurrence().count(),
        0,
        "B3: an engine that measured nothing must enumerate nothing"
    );
}
