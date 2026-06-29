mod common;

/// End-to-end CLI regression for task 4890 (γ): `reify eval` surfaces the
/// flat finishing-cost roll-up (`AssemblyBOM.total_finishing_cost = 24 USD`)
/// and the geometry-realized area coating values
/// (`CoatedPlate.coat_cost ≈ 1.2 USD`, `CoatedPlate.coat_mass ≈ 0.0018 kg`)
/// from `examples/surface_finish_cost.ri`.
///
/// PRD: docs/prds/v0_6/surface-finish-functional.md task γ, boundaries B6+B7.
///
/// Exit status is asserted unconditionally (exit 0 for a clean file regardless
/// of OCCT availability).  The flat total (B6, 24 USD) is deterministic and
/// geometry-independent — asserted unconditionally.  The area-based values
/// (B7, coat_cost/coat_mass) depend on area() which is kernel-gated — asserted
/// only when `reify_kernel_occt::OCCT_AVAILABLE`.
#[test]
fn eval_surface_finish_cost_rolls_up_total_finishing_cost_and_coat_values() {
    let path = common::example_path("surface_finish_cost.ri");

    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    // Exit 0 unconditionally — this is a clean file with no Error diagnostics.
    assert!(
        status.success(),
        "reify eval surface_finish_cost.ri should exit 0.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // ── B6: flat total_finishing_cost (deterministic, geometry-independent) ──
    //
    // Plate.finishing_cost = 12 USD (anodize) + 4 USD (temper) = 16 USD
    // Bracket.finishing_cost = 8 USD (powder coat) + 0 USD (default treatment) = 8 USD
    // AssemblyBOM.total_finishing_cost = 16 + 8 = 24 USD
    //
    // The .sum roll-up has no geometry dependency and resolves through
    // Engine::build() even in OCCT-stub mode.
    let total_line = stdout
        .lines()
        .find(|l| l.contains("AssemblyBOM.total_finishing_cost"))
        .unwrap_or_else(|| {
            panic!(
                "expected an 'AssemblyBOM.total_finishing_cost' line in stdout.\n\
                 stdout: {stdout}\nstderr: {stderr}"
            )
        });

    let total_rhs = total_line
        .split_once('=')
        .map(|(_, rhs)| rhs.trim())
        .unwrap_or_else(|| {
            panic!("AssemblyBOM.total_finishing_cost line has no '=': {total_line}")
        });

    assert_ne!(
        total_rhs, "undef",
        "AssemblyBOM.total_finishing_cost must not be undef.\nline: {total_line}"
    );

    assert!(
        total_rhs.contains("USD"),
        "AssemblyBOM.total_finishing_cost RHS must contain 'USD' (MONEY dimension).\n\
         line: {total_line}"
    );

    // Parse the leading numeric token and assert it equals 24.0 exactly
    // (integer-valued; no f64 noise at 24).
    let total_token = total_rhs.split_whitespace().next().unwrap_or("");
    let total_val: f64 = total_token.parse().unwrap_or_else(|_| {
        panic!(
            "could not parse AssemblyBOM.total_finishing_cost numeric token as f64: \
             {total_token:?}\nline: {total_line}"
        )
    });
    assert!(
        (total_val - 24.0).abs() < 1e-9,
        "AssemblyBOM.total_finishing_cost = {total_val} USD; expected 24.0 USD.\n\
         line: {total_line}"
    );

    // ── B7: area-based coat_cost and coat_mass (kernel-gated) ───────────────
    //
    // CoatedPlate geometry: box(100mm, 100mm, 10mm)
    //   area = 2*(0.1*0.1 + 0.1*0.01 + 0.1*0.01) = 2*(0.01 + 0.001 + 0.001)
    //        = 2 * 0.012 = 0.024 m^2
    //   coat_cost = 50 USD/m^2 * 0.024 m^2 = 1.2 USD
    //   coat_mass = 3000 kg/m^3 * 0.024 m^2 * 25e-6 m = 0.0018 kg
    // (f64 representation noise present → compare with abs tolerance 1e-9)
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping coat_cost/coat_mass assertions: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // --- coat_cost ---
    let coat_cost_line = stdout
        .lines()
        .find(|l| l.contains("CoatedPlate.coat_cost"))
        .unwrap_or_else(|| {
            panic!(
                "expected a 'CoatedPlate.coat_cost' line in stdout.\n\
                 stdout: {stdout}\nstderr: {stderr}"
            )
        });

    let coat_cost_rhs = coat_cost_line
        .split_once('=')
        .map(|(_, rhs)| rhs.trim())
        .unwrap_or_else(|| panic!("CoatedPlate.coat_cost line has no '=': {coat_cost_line}"));

    assert_ne!(
        coat_cost_rhs, "undef",
        "CoatedPlate.coat_cost must not be undef (kernel path must have fired).\n\
         line: {coat_cost_line}"
    );

    assert!(
        coat_cost_rhs.contains("USD"),
        "CoatedPlate.coat_cost RHS must contain 'USD' (MONEY dimension).\n\
         line: {coat_cost_line}"
    );

    let coat_cost_token = coat_cost_rhs.split_whitespace().next().unwrap_or("");
    let coat_cost_val: f64 = coat_cost_token.parse().unwrap_or_else(|_| {
        panic!(
            "could not parse CoatedPlate.coat_cost numeric token as f64: \
             {coat_cost_token:?}\nline: {coat_cost_line}"
        )
    });
    assert!(
        (coat_cost_val - 1.2).abs() < 1e-9,
        "CoatedPlate.coat_cost = {coat_cost_val} USD; expected ≈1.2 USD (tol 1e-9).\n\
         line: {coat_cost_line}"
    );

    // --- coat_mass ---
    let coat_mass_line = stdout
        .lines()
        .find(|l| l.contains("CoatedPlate.coat_mass"))
        .unwrap_or_else(|| {
            panic!(
                "expected a 'CoatedPlate.coat_mass' line in stdout.\n\
                 stdout: {stdout}\nstderr: {stderr}"
            )
        });

    let coat_mass_rhs = coat_mass_line
        .split_once('=')
        .map(|(_, rhs)| rhs.trim())
        .unwrap_or_else(|| panic!("CoatedPlate.coat_mass line has no '=': {coat_mass_line}"));

    assert_ne!(
        coat_mass_rhs, "undef",
        "CoatedPlate.coat_mass must not be undef (kernel path must have fired).\n\
         line: {coat_mass_line}"
    );

    assert!(
        coat_mass_rhs.contains("kg"),
        "CoatedPlate.coat_mass RHS must contain 'kg' (MASS dimension).\n\
         line: {coat_mass_line}"
    );

    let coat_mass_token = coat_mass_rhs.split_whitespace().next().unwrap_or("");
    let coat_mass_val: f64 = coat_mass_token.parse().unwrap_or_else(|_| {
        panic!(
            "could not parse CoatedPlate.coat_mass numeric token as f64: \
             {coat_mass_token:?}\nline: {coat_mass_line}"
        )
    });
    assert!(
        (coat_mass_val - 0.0018).abs() < 1e-9,
        "CoatedPlate.coat_mass = {coat_mass_val} kg; expected ≈0.0018 kg (tol 1e-9).\n\
         line: {coat_mass_line}"
    );

    // --- no error diagnostics ---
    // surface_finish_cost.ri is a clean file; stderr should contain no
    // "Error:" lines from the engine.
    assert!(
        !stderr.lines().any(|l| l.starts_with("Error:")),
        "reify eval produced unexpected Error diagnostics.\nstderr: {stderr}"
    );
}
