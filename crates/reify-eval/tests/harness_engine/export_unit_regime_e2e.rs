//! End-to-end LENGTH-UNIT REGIME pins over the whole DSL→file chain, covering
//! BOTH arms of the export regime:
//!
//! - STEP (task 6186): a `30mm` DSL literal must arrive in the exported STEP as
//!   `30.0` under a `SI_UNIT(.MILLI.,.METRE.)` declaration.
//! - STL (task 6187): the same literals must arrive in the exported binary STL
//!   as millimetre coordinates. STL carries no unit field, so there is no
//!   declaration half — its consumers universally read millimetres, and that
//!   de-facto convention is the promise `write_stl_binary` makes.
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

/// The per-axis AABB of every vertex in a binary STL payload.
///
/// Binary STL is 80 header bytes, a `u32` triangle count at 80..84, then a
/// 50-byte record per triangle: a 12-byte facet normal, 9 f32 vertex
/// coordinates, and a 2-byte attribute count.
///
/// As with [`cartesian_point_aabb`], an extent is the right invariant to
/// measure: it survives everything that legitimately varies — the tessellator's
/// triangle count, facet ordering, and where the box is centred — while
/// remaining a strictly physical quantity, so it fails loudly on a unit-regime
/// error and on nothing else. Facet normals are skipped rather than checked:
/// they are dimensionless unit directions, invariant under the uniform positive
/// metre→millimetre scale, so they carry no regime signal.
///
/// Returns `(min, max, n_triangles)`.
fn binary_stl_aabb(bytes: &[u8]) -> ([f32; 3], [f32; 3], usize) {
    assert!(
        bytes.len() >= 84,
        "a binary STL must carry at least an 80-byte header and a u32 count, got {} bytes",
        bytes.len()
    );
    let n_triangles = u32::from_le_bytes(bytes[80..84].try_into().unwrap()) as usize;
    assert_eq!(
        bytes.len(),
        84 + 50 * n_triangles,
        "binary STL byte length must equal 84 + 50*count"
    );

    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for tri in 0..n_triangles {
        for v in 0..3usize {
            let vbase = 84 + tri * 50 + 12 + v * 12;
            for axis in 0..3usize {
                let raw: [u8; 4] = bytes[vbase + axis * 4..vbase + axis * 4 + 4]
                    .try_into()
                    .unwrap();
                let coord = f32::from_le_bytes(raw);
                min[axis] = min[axis].min(coord);
                max[axis] = max[axis].max(coord);
            }
        }
    }
    (min, max, n_triangles)
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

/// Task 6187 — the STL arm of the same regime. A `box(30mm, 20mm, 10mm)`
/// exported to STL must carry 30 × 20 × 10 in the file, not 0.030 × 0.020 ×
/// 0.010: the user-observable symptom of the defect was a 30 mm part arriving
/// in a slicer as a 0.03 mm speck.
///
/// STL has no unit field, so unlike the STEP sibling there is no DECLARATION
/// half to assert — only the payload. That is precisely why the defect could
/// live undetected: no assertion anywhere in the tree parsed a coordinate out
/// of an exported STL, and every other STL assertion (triangle count, 84+50N
/// byte length, facet normals) is invariant under a coordinate scale.
///
/// **The 1e-3 mm bound is derived, not tuned.** The chain is f32 through one
/// ×1000 multiply, whose floor is 2·30·2^-23 ≈ 7.2e-6 mm (task 6186's own
/// derivation for this identical f32 `Mesh` → ×1000 path, recorded on
/// `write_3mf_declares_millimetres_and_scales_metre_vertices`). The box's faces
/// are planar, so they triangulate on exact corner vertices and tessellation
/// deflection contributes nothing at the extremes. 1e-3 mm therefore keeps
/// ~100× headroom for jitter while sitting four orders below the ~30 mm gap it
/// guards — it cannot pass under the defect.
///
/// The three extents are deliberately DISTINCT, as in the STEP sibling, so this
/// also catches an axis-permuting or single-axis-only scaling regression that a
/// cube could not distinguish.
///
/// Deliberately a composed-chain REGRESSION pin rather than a fresh RED — the
/// writer fix already landed, so it is expected GREEN on arrival. Its value is
/// covering the layers BETWEEN the DSL literal and the writer that no writer
/// unit test can see: the evaluator's `30mm` → SI 0.030 conversion, the
/// `OutputFormat.STL` → `ExportFormat::Stl` mapping, `build_outputs`'
/// `export_with_options` dispatch, and the OCCT actor/FFI boundary. If it ever
/// fails, the defect is in an intermediate layer, not in the bound: debug it
/// rather than relax it.
#[test]
fn dsl_millimetre_literals_round_trip_to_millimetre_stl_coordinates() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping dsl_millimetre_literals_round_trip_to_millimetre_stl_coordinates: \
             OCCT not available"
        );
        return;
    }

    let module = parse_and_compile_with_stdlib(
        r#"structure def D {
    let part = box(30mm, 20mm, 10mm)
    sub s = STLOutput(subject: part, path: "regime.stl")
}"#,
    );

    let kernel = reify_kernel_occt::OcctKernelHandle::spawn();
    let mut engine = reify_eval::Engine::new(
        Box::new(MockConstraintChecker::new()),
        Some(Box::new(kernel)),
    );

    let out_dir = tempfile::tempdir().expect("create a unique temp dir for the e2e export");
    let artifacts = engine.build_outputs(&module, out_dir.path(), None);

    let artifact = artifacts
        .iter()
        .find(|a| a.path.ends_with("regime.stl"))
        .unwrap_or_else(|| {
            panic!(
                "no ExportArtifact for `regime.stl`; produced paths were {:?}",
                artifacts.iter().map(|a| a.path.clone()).collect::<Vec<_>>()
            )
        });

    let (min, max, n_triangles) = binary_stl_aabb(&artifact.bytes);
    assert!(
        n_triangles > 0,
        "expected at least one triangle in the STL export, found none"
    );

    let expected = [30.0_f32, 20.0, 10.0];
    for (axis, name) in ["x", "y", "z"].into_iter().enumerate() {
        let extent = max[axis] - min[axis];
        let want = expected[axis];
        assert!(
            (extent - want).abs() < 1e-3,
            "a `{want}mm` DSL literal must reach the exported STL as {want} mm on {name}, but \
             the vertex AABB extent is {extent} (min {}, max {}) — some layer between the DSL \
             literal and the writer disagrees about metres vs millimetres",
            min[axis],
            max[axis]
        );
    }
}
