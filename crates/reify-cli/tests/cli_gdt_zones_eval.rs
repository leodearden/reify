//! CLI gate for examples/tolerancing/gdt_zones.ri — GD&T zone constructors.
//!
//! Gates `examples/tolerancing/gdt_zones.ri`: the example must compile and
//! evaluate cleanly (exit 0, no Error diagnostics), and stdout must contain
//! the zone let-cell name anchors plus the observable Bool anchor cell.
//!
//! Mirrors cli_tolerancing_eval.rs: cell-name + Bool anchors only — exact
//! numerics (zone volumes, axis lengths) are pinned in the Rust kernel oracle
//! tests (zone_constructors_e2e.rs), not here.
//!
//! Initially RED (step-7): examples/tolerancing/gdt_zones.ri does not yet
//! exist, so `reify eval` exits non-zero (file-not-found). GREEN after
//! step-8 creates the example file.

mod common;

/// `reify eval examples/tolerancing/gdt_zones.ri` must exit 0, emit no
/// Error-severity diagnostics on stderr, and print the zone cell-name anchors
/// plus the observable `zone_ok = true` Bool anchor on stdout.
#[test]
fn eval_gdt_zones_example_succeeds() {
    let path = common::example_path("tolerancing/gdt_zones.ri");
    let (status, stdout, stderr) = common::run_subcommand("eval", &path);

    assert!(
        status.success(),
        "reify eval gdt_zones.ri should exit 0;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // No Error-severity diagnostics on stderr — a benign Warning is OK
    // (mirrors the cli_tolerancing_eval.rs "we do NOT assert stderr is empty" convention).
    assert!(
        !stderr.contains("Error:"),
        "stderr should contain no Error diagnostics;\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // ── Bool anchor: non-geometry cell computed from a plain Length ──
    // `zone_ok = w > 0mm` where w is the zone width (positive by construction).
    assert!(
        stdout.contains("zone_ok = true"),
        "stdout should contain 'zone_ok = true' (w > 0mm = true for the declared zone width);\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );

    // ── Cell-name anchors for each zone constructor ──
    // Geometry cells may print as `<name> = <geometry-summary>` or just `<name>`;
    // we pin the cell NAME only, not the geometry value (floats live in oracle tests).
    assert!(
        stdout.contains("cyl_zone"),
        "stdout should contain 'cyl_zone' (zone_cylinder result);\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("ann_zone"),
        "stdout should contain 'ann_zone' (zone_annulus result);\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("prof_zone"),
        "stdout should contain 'prof_zone' (zone_profile result);\n\
         stdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
