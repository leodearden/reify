//! CLI integration tests for virtual/resultant condition and bolt-pattern clearance
//! (task 4478 ε, GD&T geometric-zones PRD docs/prds/v0_6/gdt-geometric-zones-and-containment.md).
//!
//! ## Tests
//!
//! ### Ungated (step-3 RED → step-4 GREEN)
//! `eval_vc_boundary_solid_example_succeeds`: evaluates `vc_boundary_solid.ri` without a
//! kernel (pure scalar + zone_cylinder). Asserts cell-name anchors (`vc`, `rc`,
//! `vc_boundary`) and the observable scalar Bool `vc_positive = true`.
//!
//! ### OCCT-gated (step-5 RED → step-6 GREEN)
//! `build_vc_bolt_pattern_clearance_satisfied`: the conformant bolt-pattern example
//! (hole radius 5.2 mm > VC radius 5.075 mm) — `min_clearance > 0mm` is Satisfied.
//!
//! ### OCCT-gated (step-7 RED → step-8 GREEN)
//! `build_vc_bolt_pattern_interference_violated`: the under-clearanced variant
//! (hole radius 5.0 mm < VC radius 5.075 mm) — `min_clearance > 0mm` is Violated.

mod common;

// ─── Ungated: vc_boundary_solid eval gate ────────────────────────────────────

/// `reify eval examples/tolerancing/vc_boundary_solid.ri` must exit 0, emit
/// no Error-severity diagnostics on stderr, and print the scalar + zone cell-name
/// anchors including the observable `vc_positive = true` Bool anchor.
///
/// This gate is OCCT-independent: vc/rc are pure Length arithmetic, and
/// zone_cylinder produces a Geometry cell that may print as a summary or name
/// only — the Bool anchor is the kernel-free observable.
///
/// RED (step-3): examples/tolerancing/vc_boundary_solid.ri does not exist yet
/// (file-not-found → non-zero exit). GREEN after step-4 creates it.
#[test]
fn eval_vc_boundary_solid_example_succeeds() {
    let path = common::example_path("tolerancing/vc_boundary_solid.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval vc_boundary_solid.ri should exit 0;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // No Error-severity diagnostics on stderr — a benign Warning is OK.
    assert!(
        !stderr.contains("Error:"),
        "stderr should contain no Error diagnostics;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // ── Bool anchor: vc > 0mm (always true for a positive VC) ────────────────
    assert!(
        stdout.contains("vc_positive = true"),
        "stdout should contain 'vc_positive = true' (vc > 0mm is always true);\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // ── Scalar cell-name anchors ──────────────────────────────────────────────
    assert!(
        stdout.contains("vc"),
        "stdout should contain 'vc' (virtual_condition result);\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("rc"),
        "stdout should contain 'rc' (resultant_condition result);\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // ── Geometry cell-name anchor for the VC boundary solid ──────────────────
    assert!(
        stdout.contains("vc_boundary"),
        "stdout should contain 'vc_boundary' (zone_cylinder at VC diameter);\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

// ─── OCCT-gated: conformant bolt-pattern clearance ───────────────────────────

/// `reify build examples/tolerancing/vc_bolt_pattern_clearance.ri` must report
/// "All constraints satisfied" (exit 0) when OCCT is available.
///
/// The hole radius (5.2 mm) exceeds the VC radius (5.075 mm), so radial
/// clearance ≈ 0.125 mm > 0 mm → the `min_clearance(s, id_vc, id_mating) > 0mm`
/// constraint is Satisfied.
///
/// Stub mode (no OCCT): skip with eprintln! — no kernel means no constraint
/// verdict (Indeterminate, not Satisfied), so the assertion would be vacuous.
///
/// RED (step-5): examples/tolerancing/vc_bolt_pattern_clearance.ri does not exist
/// yet. GREEN after step-6 creates it.
#[test]
fn build_vc_bolt_pattern_clearance_satisfied() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping build_vc_bolt_pattern_clearance_satisfied: \
             OCCT unavailable (cfg(has_occt) not set)"
        );
        return;
    }

    let out = common::run_build_at(&common::example_path(
        "tolerancing/vc_bolt_pattern_clearance.ri",
    ));

    assert!(
        out.status.success(),
        "reify build vc_bolt_pattern_clearance.ri should exit 0 (all constraints satisfied);\n\
         stdout:\n{}\nstderr:\n{}",
        out.stdout,
        out.stderr
    );
    assert!(
        out.stdout.contains("All constraints satisfied"),
        "stdout should contain 'All constraints satisfied' (hole r=5.2mm > VC r=5.075mm → clr≈0.125mm);\n\
         stdout:\n{}\nstderr:\n{}",
        out.stdout,
        out.stderr
    );
    assert!(
        !out.stdout.contains("VIOLATED"),
        "stdout must not contain 'VIOLATED' (conformant variant — clearance > 0);\n\
         stdout:\n{}\nstderr:\n{}",
        out.stdout,
        out.stderr
    );
}

// ─── OCCT-gated: under-clearanced bolt-pattern interference ──────────────────

/// `reify build examples/tolerancing/vc_bolt_pattern_interference.ri` must report
/// "VIOLATED" and exit non-zero when OCCT is available.
///
/// The hole radius (5.0 mm) is LESS than the VC radius (5.075 mm), so the VC
/// boundary overlaps the hole wall → min_clearance collapses to 0 → the
/// `min_clearance(s, id_vc, id_mating) > 0mm` constraint is Violated.
///
/// Together with `build_vc_bolt_pattern_clearance_satisfied` this is the B8
/// "both verdicts in CI" requirement.
///
/// Stub mode (no OCCT): skip with eprintln!
///
/// RED (step-7): examples/tolerancing/vc_bolt_pattern_interference.ri does not
/// exist yet. GREEN after step-8 creates it.
#[test]
fn build_vc_bolt_pattern_interference_violated() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping build_vc_bolt_pattern_interference_violated: \
             OCCT unavailable (cfg(has_occt) not set)"
        );
        return;
    }

    let out = common::run_build_at(&common::example_path(
        "tolerancing/vc_bolt_pattern_interference.ri",
    ));

    assert!(
        !out.status.success(),
        "reify build vc_bolt_pattern_interference.ri should exit non-zero \
         (hole r=5.0mm < VC r=5.075mm → clearance=0 → Violated);\n\
         stdout:\n{}\nstderr:\n{}",
        out.stdout,
        out.stderr
    );
    assert!(
        out.stdout.contains("VIOLATED"),
        "stdout should contain 'VIOLATED' (VC boundary overlaps hole wall → min_clearance=0);\n\
         stdout:\n{}\nstderr:\n{}",
        out.stdout,
        out.stderr
    );
    assert!(
        out.stdout.contains("Some constraints violated"),
        "stdout should contain 'Some constraints violated';\n\
         stdout:\n{}\nstderr:\n{}",
        out.stdout,
        out.stderr
    );
}
