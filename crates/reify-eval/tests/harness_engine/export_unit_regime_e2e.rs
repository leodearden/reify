//! Task 6186 — end-to-end LENGTH-UNIT REGIME pin over the whole DSL→file
//! chain: a `30mm` DSL literal must arrive in the exported STEP as `30.0` under
//! a `SI_UNIT(.MILLI.,.METRE.)` declaration.
//!
//! Reify's regime is: model space is SI METRES, declaration-carrying export
//! formats emit MILLIMETRES, so the unit a file DECLARES and the coordinates it
//! CARRIES agree. The unit tests for the two writers
//! (`reify-kernel-occt`'s `export_step_declares_millimetres_and_scales_metre_
//! coordinates` and `reify-ir`'s
//! `write_3mf_declares_millimetres_and_scales_metre_vertices`) pin each writer
//! from inside its own crate. Neither can see the layers BETWEEN the DSL
//! literal and the writer — the evaluator's `30mm` → SI 0.030 conversion, the
//! kernel router, the FFI boundary — so neither would catch a metre/millimetre
//! disagreement reintroduced there.
//!
//! This test drives the composed chain a user actually drives: a `.ri` source
//! written in millimetre literals (the language spec defines `5mm` as a Length
//! evaluating to SI metres), compiled, built through the real OCCT kernel, and
//! measured out of the produced bytes. It is deliberately a regression pin
//! rather than a fresh RED — the writer fixes already landed, so it is expected
//! GREEN on arrival. If it ever fails, the defect is in an intermediate scaling
//! layer, not in the bound: debug it rather than relax it.
//!
//! It also closes a standing coverage gap: before task 6186, not one test in
//! the tree parsed a float out of exported STEP text — every STEP assertion was
//! header presence, schema name, or entity count, all invariant under a
//! coordinate scale, which is exactly why a 1000× mislabel could live in the
//! tree undetected.
//!
//! Guarded on [`reify_kernel_occt::OCCT_AVAILABLE`] (the runtime flag) because
//! downstream crates cannot see `reify-kernel-occt`'s compile-time `has_occt`
//! cfg.

use reify_test_support::{MockConstraintChecker, parse_and_compile_with_stdlib};

/// The per-axis AABB of every 3D `CARTESIAN_POINT` in a STEP file.
///
/// STEP wraps long lines, so all ASCII whitespace is dropped before parsing;
/// the entity body is then `CARTESIAN_POINT('',(x,y,z))` and the coordinate
/// list is its first parenthesised group. An extent is invariant under
/// everything that legitimately varies — OCCT's float formatting, entity
/// ordering, box centring, and the AP203/AP214/AP242 schema selection — while
/// still being a strictly physical quantity, so it fails loudly on a unit
/// mislabel and on nothing else.
///
/// Returns `(min, max, n_points)`.
fn cartesian_point_aabb(step_text: &str) -> ([f64; 3], [f64; 3], usize) {
    let stripped: String = step_text
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();

    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    let mut n_points = 0usize;
    for tail in stripped.split("CARTESIAN_POINT(").skip(1) {
        let Some(open) = tail.find('(') else { continue };
        let Some(close) = tail[open + 1..].find(')') else {
            continue;
        };
        let coords: Vec<f64> = tail[open + 1..open + 1 + close]
            .split(',')
            .filter_map(|s| s.parse::<f64>().ok())
            .collect();
        if coords.len() != 3 {
            // Not a 3D point (or an unparsable body) — ignore it.
            continue;
        }
        n_points += 1;
        for axis in 0..3 {
            min[axis] = min[axis].min(coords[axis]);
            max[axis] = max[axis].max(coords[axis]);
        }
    }
    (min, max, n_points)
}

/// A `box(30mm, 20mm, 10mm)` exported to STEP must declare millimetres AND
/// carry millimetre coordinates: 30 × 20 × 10 in the file, not 0.030 × 0.020
/// × 0.010.
///
/// The 1e-6 mm bound is derived, not tuned: the whole chain is f64 through one
/// exactly-representable ×1000 multiply (≤1 ulp ≈ 3.6e-15 mm at 30) plus OCCT's
/// ≥12-significant-digit decimal round-trip (≤5e-11 mm) — roughly four orders
/// of margin, and still four orders tighter than the 0.030-vs-30.0 gap it
/// guards, so it cannot pass under the defect it pins.
///
/// The three extents are deliberately DISTINCT so the test also catches an
/// axis-permuting or single-axis-only scaling regression, which a cube could
/// not distinguish.
#[test]
fn dsl_millimetre_literals_round_trip_to_millimetre_step_coordinates() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping dsl_millimetre_literals_round_trip_to_millimetre_step_coordinates: \
             OCCT not available"
        );
        return;
    }

    // Millimetre DSL literals — the language spec defines `5mm` as a Length
    // whose SI value is 0.005 m, so this box is 0.030 × 0.020 × 0.010 in
    // reify model space by the time it reaches the kernel.
    let module = parse_and_compile_with_stdlib(
        r#"structure def D {
    let part = box(30mm, 20mm, 10mm)
    sub s = STEPOutput(subject: part, path: "regime.step")
}"#,
    );

    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    let mut engine = reify_eval::Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );

    // Unique per-run temp dir (auto-removed on drop); kept bound through the
    // assertions so it outlives the build call.
    let out_dir = tempfile::tempdir().expect("create a unique temp dir for the e2e export");
    let artifacts = engine.build_outputs(&module, out_dir.path(), None);

    let artifact = artifacts
        .iter()
        .find(|a| a.path.ends_with("regime.step"))
        .unwrap_or_else(|| {
            panic!(
                "no ExportArtifact for `regime.step`; produced paths were {:?}",
                artifacts.iter().map(|a| a.path.clone()).collect::<Vec<_>>()
            )
        });

    let step_text =
        String::from_utf8(artifact.bytes.clone()).expect("STEP bytes must be valid UTF-8");
    assert!(
        !step_text.is_empty(),
        "the STEP export must have written bytes"
    );

    // (1) DECLARATION half — the file says millimetres.
    let stripped: String = step_text
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    assert!(
        stripped.contains("SI_UNIT(.MILLI.,.METRE.)"),
        "the STEP export must declare millimetres via SI_UNIT(.MILLI.,.METRE.)"
    );

    // (2) PAYLOAD half — and carries millimetres, matching the DSL literals.
    let (min, max, n_points) = cartesian_point_aabb(&step_text);
    assert!(
        n_points > 0,
        "expected at least one 3D CARTESIAN_POINT in the STEP export, found none"
    );

    let expected = [30.0_f64, 20.0, 10.0];
    for (axis, name) in ["x", "y", "z"].into_iter().enumerate() {
        let extent = max[axis] - min[axis];
        let want = expected[axis];
        assert!(
            (extent - want).abs() < 1e-6,
            "a `{want}mm` DSL literal must reach the millimetre-declared STEP file as {want} mm \
             on {name}, but the CARTESIAN_POINT AABB extent is {extent} (min {}, max {}) — some \
             layer between the DSL literal and the writer disagrees about metres vs millimetres",
            min[axis],
            max[axis]
        );
    }
}
