//! End-to-end CLI tests for the §7 tolerancing example CI gate.
//!
//! Gates `examples/tolerancing/std_tolerancing_surface.ri`: the example must
//! compile cleanly, expose the MMC-vs-RFS conformance FLIP as observable Bool
//! value cells, and have all satisfiable constraints pass under `reify check`.
//!
//! A benign compiler Warning (e.g. unused symbol) may appear on stderr —
//! we do NOT assert stderr is empty (mirror of cli_stackup_eval.rs pattern).

mod common;

/// Test A: `reify eval examples/tolerancing/std_tolerancing_surface.ri`
/// exits 0 and stdout shows the MMC-vs-RFS conformance FLIP:
///   conforms_mmc = true   (effective zone 0.2mm ≥ 0.15mm under MMC)
///   conforms_rfs = false  (effective zone 0.1mm < 0.15mm under RFS)
///
/// Also asserts presence of key cell-name substrings covering each signal family
/// (ISO grade width, expanded zone, fit max clearance, symmetric upper limit,
/// surface finish bool).  Anchors on cell NAMES + exact Bool text only —
/// NOT fragile float formatting (exact numerics are pinned by α/β/γ unit tests).
#[test]
fn eval_std_tolerancing_surface_example_succeeds() {
    let path = common::example_path("tolerancing/std_tolerancing_surface.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval std_tolerancing_surface.ri should exit 0;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // ── The headline observable signal: MMC-vs-RFS conformance FLIP ──────────
    assert!(
        stdout.contains("conforms_mmc = true"),
        "stdout should contain 'conforms_mmc = true' (MMC zone 0.2mm ≥ 0.15mm);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("conforms_rfs = false"),
        "stdout should contain 'conforms_rfs = false' (RFS zone 0.1mm < 0.15mm);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // ── ISO tolerance grade (iso_it_tolerance builtin) ────────────────────────
    assert!(
        stdout.contains("it7_width"),
        "stdout should contain 'it7_width' (IT7@Ø30–50 ISO grade cell);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // ── Effective tolerance zone cell ─────────────────────────────────────────
    assert!(
        stdout.contains("expanded_zone_mmc"),
        "stdout should contain 'expanded_zone_mmc' (zone size under MMC);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // ── Fit max clearance (nested DimensionalTolerance in Fit struct) ─────────
    assert!(
        stdout.contains("fit_maxc"),
        "stdout should contain 'fit_maxc' (Fit.max_clearance derived let);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // ── Symmetric tolerance upper_limit (DimensionalTolerance derived let) ────
    assert!(
        stdout.contains("sym_upper"),
        "stdout should contain 'sym_upper' (symmetric_tolerance upper_limit);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // ── Surface finish bool cell (sf.value > 0mm inline expression) ──────────
    // finish_ok is produced by `sf.value > 0mm` (not require_finish); the inline
    // expression is used because the eval engine propagates Undef through free function
    // calls with Geometry args.  require_finish() is regression-locked in tolerancing_tests.rs.
    assert!(
        stdout.contains("finish_ok = true"),
        "stdout should contain 'finish_ok = true' (sf.value > 0mm: 1.6µm > 0mm → true);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // ── α new-type exercises: nominal_zone reads off the new GD&T types ────────
    // Value-agnostic name-substring anchors (same style as it7_width / fit_maxc):
    // nominal_zone materialises a real scalar for each, so the cell prints.
    //   soa_zone    — StraightnessOfAxis (FOS axis form variant, MMC-eligible)
    //   runout_zone — CircularRunout with a required datum_refs
    //   prof_zone   — ProfileOfSurfaceRelated with a required datum_refs
    assert!(
        stdout.contains("soa_zone"),
        "stdout should contain 'soa_zone' (StraightnessOfAxis.nominal_zone);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("runout_zone"),
        "stdout should contain 'runout_zone' (CircularRunout.nominal_zone w/ datum_refs);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("prof_zone"),
        "stdout should contain 'prof_zone' (ProfileOfSurfaceRelated.nominal_zone w/ datum_refs);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

/// Test B: `reify check examples/tolerancing/std_tolerancing_surface.ri`
/// exits 0 — all satisfiable constraints pass (Conforms MMC zone 0.2mm ≥ 0.15mm
/// + require_finish 1.6µm > 0mm).
///
/// `reify check` prints "All constraints satisfied." on stdout and exits 0 when
/// every constraint is satisfied; "Some constraints violated." + exit non-zero
/// when any constraint is violated (verified via main.rs cmd_check).
#[test]
fn check_std_tolerancing_surface_example_succeeds() {
    let path = common::example_path("tolerancing/std_tolerancing_surface.ri");
    let (status, stdout, stderr) = common::run_subcommand("check", &path);

    assert!(
        status.success(),
        "reify check std_tolerancing_surface.ri should exit 0 (all constraints satisfied);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // Positive assertion: constraints were actually evaluated and all passed.
    // Without this, a silent "no constraints registered" regression would still
    // exit 0 and the negative assertion below would be vacuously true.
    assert!(
        stdout.contains("All constraints satisfied."),
        "stdout should contain 'All constraints satisfied.' (confirms constraints were evaluated);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("Some constraints violated"),
        "stdout should NOT contain 'Some constraints violated';\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
