//! End-to-end CLI tests for the §7 tolerancing example CI gate.
//!
//! Gates `examples/tolerancing/std_tolerancing_surface.ri`: the example must
//! compile cleanly, expose the MMC-vs-RFS conformance FLIP as observable Bool
//! value cells, and have all satisfiable constraints pass under `reify check`.
//!
//! A benign compiler Warning (e.g. unused symbol) may appear on stderr —
//! we do NOT assert stderr is empty (mirror of cli_stackup_eval.rs pattern).

use crate::common;

/// Assert that `cell` is BOTH present in eval stdout AND not printed as `undef`.
///
/// The two halves are individually insufficient in opposite directions, which is
/// why they are only ever applied as a pair:
///
/// - `contains(cell)` alone stays true against a cell that has regressed to
///   `undef`. The eval printer emits `println!("{} = {}", id, v)` for undef cells
///   too and `reify eval` still exits 0 (the root-cause `note:` goes to stderr),
///   so the cell NAME appears either way. That is the exact blind spot that let a
///   stale grade-first `iso_it_tolerance` call site survive the #6091
///   subject-first flip with the whole suite green.
/// - `!contains("<cell> = undef")` alone passes VACUOUSLY if the cell is renamed,
///   dropped from the example, or stops being emitted by the printer — leaving
///   the call site the guard exists to cover unguarded, suite still green.
///
/// This is the file's weaker tier, for derived cells whose printed value is NOT a
/// clean pass-through. Clean values get an exact `contains("<cell> = <value> m")`
/// pin instead (the nominal_zone / Location / Orientation families below), which
/// subsumes both halves.
///
/// Which tier a cell belongs in is MEASURED, not assumed — measured 2026-08-25 by
/// running `reify eval` on this example:
///   it7_width, it7_via_grade  0.000024979887994163098 m  (cube-root result)
///   fit_maxc                  0.0002499999999999985 m    (not 0.00025)
///   expanded_zone_mmc         0.0002 m                   (clean)
///   sym_upper                 0.0101 m                   (clean)
/// So `fit_maxc` reads as a clean 0.25mm in the example's own comment yet prints
/// with visible float noise; it belongs in this tier, not the exact-pin tier.
fn assert_cell_present_and_defined(stdout: &str, stderr: &str, cell: &str, why: &str) {
    assert!(
        stdout.contains(cell),
        "stdout should contain '{cell}' ({why});\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stdout.contains(&format!("{cell} = undef")),
        "{cell} must materialise a real value, not undef ({why}) — an undef here means \
         a call site or member access feeding it has regressed;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

/// Test A: `reify eval examples/tolerancing/std_tolerancing_surface.ri`
/// exits 0 and stdout shows the MMC-vs-RFS conformance FLIP:
///   conforms_mmc = true   (effective zone 0.2mm ≥ 0.15mm under MMC)
///   conforms_rfs = false  (effective zone 0.1mm < 0.15mm under RFS)
///
/// Also covers each signal family (ISO grade width, expanded zone, fit max
/// clearance, symmetric upper limit, surface finish bool).
///
/// NO assertion in this test is a bare name anchor. A name-only
/// `contains("<cell>")` is blind to a cell that regressed to `undef`, because the
/// eval printer prints undef cells and `reify eval` still exits 0 — the blind spot
/// that let a stale grade-first `iso_it_tolerance` call site survive the #6091
/// flip with the suite green. Every cell below is therefore in exactly one of two
/// tiers, chosen by MEASURING its printed value (see
/// `assert_cell_present_and_defined`):
///
/// - exact value pin — `conforms_mmc`/`conforms_rfs`/`finish_ok` (Bool text) and
///   the clean pass-through scalars: the nominal_zone family, the
///   Location/Orientation callouts, `expanded_zone_mmc`, `sym_upper`.
/// - present-and-defined pair — cells whose printed value carries float noise and
///   whose exact numerics are pinned by the α/β/γ unit tests instead:
///   `it7_width`, `it7_via_grade` (cube-root results) and `fit_maxc`.
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
    // Present-and-defined tier: IT7@Ø30–50 is a cube-root result (measured
    // 0.000024979887994163098 m), so its Display rendering is genuinely fragile in
    // the way the clean pass-through pins further down are not.  Its exact numeric
    // is pinned by α's unit test instead.
    assert_cell_present_and_defined(
        &stdout,
        &stderr,
        "it7_width",
        "IT7@Ø30–50 ISO grade cell — an undef here means THIS example's call site \
         no longer matches the builtin's argument decode",
    );
    // The other route to the same number: it7_via_grade reads
    // ISOToleranceGrade.tolerance_value, i.e. the call site inside the *prelude*
    // (crates/reify-compiler/stdlib/tolerancing.ri) rather than the one in this
    // example — a distinct call site that the it7_width guard does not reach.
    // Measured live (equal to it7_width to the last digit) once the prelude was
    // migrated, which also settles that a non-`pub` prelude structure does fold
    // its derived let for a user module.
    assert_cell_present_and_defined(
        &stdout,
        &stderr,
        "it7_via_grade",
        "ISOToleranceGrade.tolerance_value cell — an undef here means the PRELUDE's \
         iso_it_tolerance call site has drifted from the builtin's decode",
    );

    // ── Effective tolerance zone cell ─────────────────────────────────────────
    // Exact-pin tier: efz(0.1mm, MMC, 0.1mm) = 0.2mm, a clean sum (measured
    // 0.0002 m exactly).  This is the scalar behind the `conforms_mmc = true`
    // flip pinned above, so pinning it turns that Bool from an assertion about an
    // opaque comparison into one whose operand is nailed down too.
    assert!(
        stdout.contains("expanded_zone_mmc = 0.0002 m"),
        "stdout should contain 'expanded_zone_mmc = 0.0002 m' (efz(0.1mm, MMC, 0.1mm) zone size under MMC, not undef);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // ── Fit max clearance (nested DimensionalTolerance in Fit struct) ─────────
    // Present-and-defined tier, and the one cell here where that is a MEASURED
    // call rather than an obvious one: the example annotates fit_maxc as a clean
    // "0.25mm = 2.5e-4 m", but it actually prints 0.0002499999999999985 — the
    // limit arithmetic inside Fit.max_clearance does not land on the nearest
    // double to 0.00025.  Pinning that literal would be precisely the fragility
    // this file's two-tier convention exists to avoid.
    assert_cell_present_and_defined(
        &stdout,
        &stderr,
        "fit_maxc",
        "Fit.max_clearance derived let — reads through a nested DimensionalTolerance, \
         so an undef here means a member access or prelude call site regressed",
    );

    // ── Symmetric tolerance upper_limit (DimensionalTolerance derived let) ────
    // Exact-pin tier: 10mm + 0.1mm = 10.1mm, clean (measured 0.0101 m exactly).
    assert!(
        stdout.contains("sym_upper = 0.0101 m"),
        "stdout should contain 'sym_upper = 0.0101 m' (symmetric_tolerance upper_limit 10.1mm, not undef);\nstdout:\n{stdout}\nstderr:\n{stderr}"
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
    // VALUE-pinning anchors (mirror the conforms_mmc / finish_ok style, NOT the
    // value-agnostic it7_width style): each nominal_zone must materialise its real
    // scalar, so we pin the exact printed value.  A name-only `contains("soa_zone")`
    // substring would still pass if nominal_zone regressed to `undef` — the eval
    // printer prints the cell name either way — which is the very thing these
    // exercises claim to cover.  These are zero-departure nominal zones, so
    // efz(tol, condition, 0mm) == tol exactly (clean pass-through, no float drift),
    // printed by the eval engine in metres:
    //   soa_zone    = 0.05mm → 0.00005 m  — StraightnessOfAxis (FOS axis form variant)
    //   runout_zone = 0.02mm → 0.00002 m  — CircularRunout with a required datum_refs
    //   prof_zone   = 0.03mm → 0.00003 m  — ProfileOfSurfaceRelated with a required datum_refs
    assert!(
        stdout.contains("soa_zone = 0.00005 m"),
        "stdout should contain 'soa_zone = 0.00005 m' (StraightnessOfAxis.nominal_zone = 0.05mm, not undef);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("runout_zone = 0.00002 m"),
        "stdout should contain 'runout_zone = 0.00002 m' (CircularRunout.nominal_zone = 0.02mm w/ datum_refs, not undef);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("prof_zone = 0.00003 m"),
        "stdout should contain 'prof_zone = 0.00003 m' (ProfileOfSurfaceRelated.nominal_zone = 0.03mm w/ datum_refs, not undef);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // ── Value-pins for Location/Orientation callouts (by-name binder guard) ──
    // The eval-engine named-argument binder now binds strictly by parameter name
    // (task-4522), so beyond-trait params such as `zone_shape` no longer need to
    // precede `material_condition` in the declaration order. These pins verify that
    // the by-name binder correctly routes the arguments and keeps nominal_zone
    // materialising a real scalar.
    //
    //   pos_zone = efz(0.1mm, MMC, 0mm) = 0.1mm = 0.0001 m
    //     Position with explicit MMC — proves the beyond-trait zone_shape param
    //     does not corrupt nominal_zone when material_condition is explicit.
    //   par_zone = efz(0.04mm, RFS, 0mm) = 0.04mm = 0.00004 m
    //     Parallelism with IMPLICIT material_condition (RFS default) — the critical
    //     case: the old positional binder would misbind material_condition to undef
    //     when zone_shape followed it; the by-name binder handles this correctly.
    assert!(
        stdout.contains("pos_zone = 0.0001 m"),
        "stdout should contain 'pos_zone = 0.0001 m' (Position.nominal_zone = 0.1mm under MMC, not undef);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("par_zone = 0.00004 m"),
        "stdout should contain 'par_zone = 0.00004 m' (Parallelism.nominal_zone = 0.04mm under RFS default, not undef);\nstdout:\n{stdout}\nstderr:\n{stderr}"
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
