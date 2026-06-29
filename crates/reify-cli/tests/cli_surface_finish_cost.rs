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
    // Engine::build() even in OCCT-stub mode.  24 is integer-valued, so tol
    // 1e-9 (exact-arithmetic; no kernel noise).
    assert_cell_value(&stdout, &stderr, "AssemblyBOM.total_finishing_cost", "USD", 24.0, 1e-9);

    // ── B7: area-based coat_cost and coat_mass (kernel-gated) ───────────────
    //
    // CoatedPlate geometry: box(100mm, 100mm, 10mm)
    //   area = 2*(0.1*0.1 + 0.1*0.01 + 0.1*0.01) = 0.024 m^2
    //   coat_cost = 50 USD/m^2 * 0.024 m^2 = 1.2 USD
    //   coat_mass = 3000 kg/m^3 * 0.024 m^2 * 25e-6 m = 0.0018 kg
    //
    // Tolerance 1e-6 for these kernel-realized values (box area is analytic
    // and typically exact to ~1e-12 relative, but the 50× cost multiplier
    // amplifies any noise; 1e-6 comfortably covers OCCT floating-point
    // variation while still catching real value regressions).
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping coat_cost/coat_mass assertions: OCCT unavailable \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    assert_cell_value(&stdout, &stderr, "CoatedPlate.coat_cost", "USD", 1.2, 1e-6);
    assert_cell_value(&stdout, &stderr, "CoatedPlate.coat_mass", "kg", 0.0018, 1e-6);

    // --- no error diagnostics ---
    // surface_finish_cost.ri is a clean file; stderr should contain no
    // "Error:" lines from the engine.
    assert!(
        !stderr.lines().any(|l| l.starts_with("Error:")),
        "reify eval produced unexpected Error diagnostics.\nstderr: {stderr}"
    );
}

/// Extract a `Struct.member = <value> <unit>` line from `stdout`, then assert:
///
/// - the RHS (after `=`) is not `"undef"`
/// - the unit token (second whitespace-delimited token on the RHS) equals
///   `unit` **exactly** — prevents a dimensionally-wrong value (e.g. `USD/m^2`
///   instead of `USD`) from slipping through on a substring match
/// - `|(numeric value) − expected| < tol`
///
/// Pass `tol = 1e-9` for exact-arithmetic values (integer USD totals) and
/// `tol = 1e-6` for kernel-realized geometry values (coat_cost/coat_mass)
/// where OCCT floating-point variation can amplify past pure f64 epsilon.
fn assert_cell_value(
    stdout: &str,
    stderr: &str,
    name: &str,
    unit: &str,
    expected: f64,
    tol: f64,
) {
    let line = stdout
        .lines()
        .find(|l| l.contains(name))
        .unwrap_or_else(|| {
            panic!(
                "expected a '{name}' line in stdout.\nstdout: {stdout}\nstderr: {stderr}"
            )
        });

    let rhs = line
        .split_once('=')
        .map(|(_, r)| r.trim())
        .unwrap_or_else(|| panic!("{name} line has no '=': {line}"));

    assert_ne!(rhs, "undef", "{name} must not be undef.\nline: {line}");

    let mut tokens = rhs.split_whitespace();
    let num_tok = tokens.next().unwrap_or("");
    let unit_tok = tokens.next().unwrap_or("");

    assert_eq!(
        unit_tok, unit,
        "{name} unit should be '{unit}' (exact); got '{unit_tok}'.\nline: {line}"
    );

    let val: f64 = num_tok.parse().unwrap_or_else(|_| {
        panic!(
            "could not parse {name} numeric token as f64: {num_tok:?}\nline: {line}"
        )
    });
    assert!(
        (val - expected).abs() < tol,
        "{name} = {val} {unit_tok}; expected ≈{expected} {unit} (tol {tol}).\nline: {line}"
    );
}
