mod common;

/// End-to-end CLI regression for task 4145: `reify eval` surfaces geometry-query
/// value cells (`mass`, `centroid`) for a `Physical` structure.
///
/// Before the kernel-backed build() path was gated into cmd_eval, both cells
/// printed "undef" because `Engine::new(None) + eval()` never runs the
/// `run_post_processes`/`post_process_geometry_queries` geometry pipeline.
///
/// Exit status is asserted unconditionally (exit 0 for a clean file regardless
/// of OCCT availability). Mass/centroid value assertions are gated on
/// `reify_kernel_occt::OCCT_AVAILABLE` so stub-mode CI runners skip cleanly.
#[test]
fn eval_spec_shape_physical_surfaces_mass_and_centroid() {
    let path = common::example_path("spec-shape-physical.ri");

    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    // Exit 0 unconditionally — this is a clean file with no Error diagnostics.
    assert!(
        status.success(),
        "reify eval spec-shape-physical.ri should exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Geometry assertions only when the OCCT kernel is compiled in.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping mass/centroid assertions: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // --- mass ---
    // Expect a line like "Bracket.mass = 0.0471 kg" (or similar SI display).
    // The analytic value is V * density = 6e-6 m³ * 7850 kg·m⁻³ = 0.0471 kg;
    // the committed golden pins 4.710000e-2 [kg] via the same build() path.
    // We allow a small band [0.046, 0.048] to accommodate f64 display rounding.
    let mass_line = stdout
        .lines()
        .find(|l| l.contains("Bracket.mass"))
        .unwrap_or_else(|| {
            panic!("expected a 'Bracket.mass' line in stdout.\nstdout: {stdout}\nstderr: {stderr}")
        });

    // RHS is everything after the '='
    let mass_rhs = mass_line
        .split_once('=')
        .map(|(_, rhs)| rhs.trim())
        .unwrap_or_else(|| panic!("Bracket.mass line has no '=': {mass_line}"));

    assert_ne!(
        mass_rhs, "undef",
        "Bracket.mass must not be undef (kernel path must have fired).\nline: {mass_line}"
    );

    assert!(
        mass_rhs.contains("kg"),
        "Bracket.mass RHS must contain 'kg' (MASS dimension).\nline: {mass_line}"
    );

    // The leading token of the RHS should be the numeric SI value.
    // Value::Display for a Scalar emits "<si_value> <dimension>".
    let numeric_token = mass_rhs.split_whitespace().next().unwrap_or("");
    let mass_kg: f64 = numeric_token.parse().unwrap_or_else(|_| {
        panic!(
            "could not parse Bracket.mass numeric token as f64: {numeric_token:?}\nline: {mass_line}"
        )
    });
    assert!(
        (0.046..=0.048).contains(&mass_kg),
        "Bracket.mass = {mass_kg} kg is outside the expected band [0.046, 0.048] kg.\nline: {mass_line}"
    );

    // --- centroid ---
    // Expect a line like "Bracket.centroid = point(0, 0, 0)" (from the golden).
    // We only pin non-undef and the "point(" prefix — not exact coordinates —
    // because cmd_eval renders via raw Value::Display which may have f64 jitter.
    let centroid_line = stdout
        .lines()
        .find(|l| l.contains("Bracket.centroid"))
        .unwrap_or_else(|| {
            panic!(
                "expected a 'Bracket.centroid' line in stdout.\nstdout: {stdout}\nstderr: {stderr}"
            )
        });

    let centroid_rhs = centroid_line
        .split_once('=')
        .map(|(_, rhs)| rhs.trim())
        .unwrap_or_else(|| panic!("Bracket.centroid line has no '=': {centroid_line}"));

    assert_ne!(
        centroid_rhs, "undef",
        "Bracket.centroid must not be undef.\nline: {centroid_line}"
    );

    assert!(
        centroid_rhs.starts_with("point("),
        "Bracket.centroid RHS must start with 'point('.\nline: {centroid_line}"
    );

    // --- no error diagnostics ---
    // spec-shape-physical.ri is a clean file; stderr should contain no
    // "Error:" lines from the engine.
    assert!(
        !stderr.lines().any(|l| l.starts_with("Error:")),
        "reify eval produced unexpected Error diagnostics.\nstderr: {stderr}"
    );
}

/// End-to-end CLI regression for task 4168 (geom-transform ε): `arbitrary_pattern`'s
/// LIST form — a `List<Transform<3>>` carrying a full per-instance rigid transform
/// (rotation + translation) — must be honored end-to-end through `reify eval`,
/// additively alongside the existing scalar-triple (translation-only) form
/// (superseding the phantom-partial task 323).
///
/// examples/pattern_arbitrary_transforms.ri unions `box(2mm,2mm,10mm)` with a
/// single Y-90-rotated copy (zero translation) via a `Physical`-conforming
/// structure (mirrors spec-shape-physical.ri), so `reify eval` surfaces
/// `mass = volume(geometry) * material.density` as a real `Scalar<Mass>`.
///
/// Exit status is asserted unconditionally (exit 0 for a clean file regardless
/// of OCCT availability). The non-undef / numeric-band mass assertions are
/// gated on `reify_kernel_occt::OCCT_AVAILABLE`, mirroring
/// `eval_spec_shape_physical_surfaces_mass_and_centroid` (without OCCT,
/// `volume(geometry)` cannot be dispatched, so `mass` is legitimately undef —
/// same status as `Rigid.moment_of_inertia`, see structural_physical.ri).
/// The band is chosen to fall strictly between the rotation-honored union
/// value (~5.652e-4 kg) and the value a rotation-dropped regression would
/// produce (~3.14e-4 kg, i.e. the single un-rotated box — task 323's
/// translation-only behaviour), so this test would catch the per-instance
/// rotation silently being dropped anywhere in the compiler/eval/kernel chain.
///
/// RED (pre-step-10): examples/pattern_arbitrary_transforms.ri does not exist
/// yet, so `run_subcommand` invokes `reify eval` on a nonexistent path, which
/// fails to parse/compile and exits non-zero — failing the unconditional
/// `status.success()` assertion below.
#[test]
fn eval_pattern_arbitrary_transforms_honors_rotation() {
    let path = common::example_path("pattern_arbitrary_transforms.ri");

    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    // Exit 0 unconditionally — this is a clean file with no Error diagnostics.
    assert!(
        status.success(),
        "reify eval pattern_arbitrary_transforms.ri should exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Geometry assertions only when the OCCT kernel is compiled in.
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping mass assertions: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // Expect a line like "ArbitraryTransformBracket.mass = 5.652000e-4 kg".
    let mass_line = stdout
        .lines()
        .find(|l| l.contains("ArbitraryTransformBracket.mass"))
        .unwrap_or_else(|| {
            panic!(
                "expected an 'ArbitraryTransformBracket.mass' line in stdout.\nstdout: {stdout}\nstderr: {stderr}"
            )
        });

    let mass_rhs = mass_line
        .split_once('=')
        .map(|(_, rhs)| rhs.trim())
        .unwrap_or_else(|| panic!("ArbitraryTransformBracket.mass line has no '=': {mass_line}"));

    assert_ne!(
        mass_rhs, "undef",
        "ArbitraryTransformBracket.mass must not be undef (kernel path must have fired).\nline: {mass_line}"
    );

    assert!(
        mass_rhs.contains("kg"),
        "ArbitraryTransformBracket.mass RHS must contain 'kg' (MASS dimension).\nline: {mass_line}"
    );

    let numeric_token = mass_rhs.split_whitespace().next().unwrap_or("");
    let mass_kg: f64 = numeric_token.parse().unwrap_or_else(|_| {
        panic!(
            "could not parse ArbitraryTransformBracket.mass numeric token as f64: {numeric_token:?}\nline: {mass_line}"
        )
    });

    // Rotation-honored union: 7.2e-8 m^3 * 7850 kg/m^3 = 5.652e-4 kg. This band
    // comfortably contains that value while excluding the rotation-dropped
    // value (~3.14e-4 kg, the single un-rotated box).
    assert!(
        (0.00050..=0.00062).contains(&mass_kg),
        "ArbitraryTransformBracket.mass = {mass_kg} kg is outside the rotation-honored band \
         [0.00050, 0.00062] kg — expected ~5.652e-4 kg for box(2,2,10) unioned with its \
         Y-90-rotated copy. A value near 3.14e-4 kg would indicate the per-instance rotation \
         was dropped (task 323's translation-only regression).\nline: {mass_line}"
    );

    // --- no error diagnostics ---
    assert!(
        !stderr.lines().any(|l| l.starts_with("Error:")),
        "reify eval produced unexpected Error diagnostics.\nstderr: {stderr}"
    );
}

/// Back-compat guard: the existing scalar-triple (translation-only) form of
/// `arbitrary_pattern` — exercised by examples/pattern_composition.ri's
/// `PatternComposed.shifted` — must continue to evaluate cleanly (exit 0) now
/// that task 4168 has additively wired in the LIST form alongside it.
#[test]
fn eval_pattern_composition_triple_form_still_exits_zero() {
    let path = common::example_path("pattern_composition.ri");

    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval pattern_composition.ri should still exit 0 (triple-form back-compat).\nstdout: {stdout}\nstderr: {stderr}"
    );
}
