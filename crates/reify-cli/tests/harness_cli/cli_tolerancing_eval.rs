//! End-to-end CLI tests for the §7 tolerancing example CI gate.
//!
//! Gates `examples/tolerancing/std_tolerancing_surface.ri`: the example must
//! compile cleanly, expose the MMC-vs-RFS conformance FLIP as observable Bool
//! value cells, and have all satisfiable constraints pass under `reify check`.
//!
//! A benign compiler Warning (e.g. unused symbol) may appear on stderr —
//! we do NOT assert stderr is empty (mirror of cli_stackup_eval.rs pattern).

use crate::common;

/// Test A: `reify eval examples/tolerancing/std_tolerancing_surface.ri`
/// exits 0 and stdout shows the MMC-vs-RFS conformance FLIP:
///   conforms_mmc = true   (effective zone 0.2mm ≥ 0.15mm under MMC)
///   conforms_rfs = false  (effective zone 0.1mm < 0.15mm under RFS)
///
/// Also asserts presence of key cell-name substrings covering each signal family
/// (ISO grade width, expanded zone, fit max clearance, symmetric upper limit,
/// surface finish bool).  Mostly anchors on cell NAMES + exact Bool text —
/// NOT fragile float formatting (exact numerics are pinned by α/β/γ unit tests).
///
/// Two exceptions where a name-only anchor was shown to be too weak: the
/// nominal_zone family pins exact printed values (see the rationale at those
/// asserts), and the two IT7 cells (`it7_width`, `it7_via_grade`) each PAIR a
/// name anchor with an anti-`undef` assertion, because the eval printer prints
/// undef cells at exit 0 — so a name-only anchor cannot see a regressed cell at
/// all, while an anti-`undef` assertion alone would pass vacuously if the cell
/// stopped being emitted.
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
    // The name anchor alone is NOT sufficient, and this cell is no longer
    // value-agnostic-only.  The eval printer emits `println!("{} = {}", id, v)`
    // for undef cells too and `reify eval` still exits 0 (the root-cause `note:`
    // goes to stderr), so a bare `contains("it7_width")` stays true even when the
    // cell has regressed to `undef` — exactly the blind spot that let a stale
    // grade-first call site survive the 6091 subject-first flip with the whole
    // suite green.  The anti-undef assertion below is what actually sees it.
    //
    // The `!contains(… = undef)` form is used rather than an exact printed float
    // (contrast the soa_zone/runout_zone/prof_zone pins further down): IT7@Ø30–50
    // is 24.969µm, a cube-root result, so its Display rendering is genuinely
    // fragile in the way those clean pass-through zone values are not.
    assert!(
        stdout.contains("it7_width"),
        "stdout should contain 'it7_width' (IT7@Ø30–50 ISO grade cell);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("it7_width = undef"),
        "it7_width must materialise a real ISO 286-1 tolerance, not undef — an undef \
         here means the call site's argument order no longer matches the builtin's \
         decode;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // Same guard for the other route to the same number: it7_via_grade reads
    // ISOToleranceGrade.tolerance_value, i.e. the call site inside the *prelude*
    // (crates/reify-compiler/stdlib/tolerancing.ri) rather than the one in this
    // example.  Measured live (24.98µm, equal to it7_width) once the prelude was
    // migrated, which also settles that a non-`pub` prelude structure does fold
    // its derived let for a user module.  Without this, a stale prelude call site
    // would regress silently exactly as the example's did.
    //
    // Paired with a positive name anchor, exactly like it7_width above: without
    // it the `!contains(… = undef)` half passes VACUOUSLY if the cell is ever
    // renamed, dropped from the example, or stops being emitted by the printer —
    // leaving the prelude call site this guard exists to cover unguarded with the
    // suite still green.
    assert!(
        stdout.contains("it7_via_grade"),
        "stdout should contain 'it7_via_grade' (ISOToleranceGrade.tolerance_value cell);\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !stdout.contains("it7_via_grade = undef"),
        "it7_via_grade must materialise ISOToleranceGrade.tolerance_value, not undef — \
         an undef here means the prelude's iso_it_tolerance call site has drifted from \
         the builtin's decode;\nstdout:\n{stdout}\nstderr:\n{stderr}"
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
