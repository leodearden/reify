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
