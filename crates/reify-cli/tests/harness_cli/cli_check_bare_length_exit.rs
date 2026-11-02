//! `reify check`'s exit on a bare number in a geometry LENGTH slot, under
//! #5403's general `Severity::Error` gate (PRD
//! docs/prds/v0_6/check-diagnostic-truthfulness.md, decision D3): exit 1 on
//! every constructor, whichever layer rejects it.
//!
//! The layer shows on stdout. No verdict line means the compiler stopped the
//! form before evaluation; `All constraints satisfied.` means the form was
//! evaluated to a verdict and then rejected by the post-verdict gate.
//!
//! These rows pin the "Which command catches it" section of
//! crates/reify-mcp/src/tools/chunks/units.md. A row that changes layer moves
//! that section and its SYNC ledger with it.
//!
//! The 7-argument `mirror` pivot is pinned by
//! cli_check.rs::check_rejects_bare_scalar_mirror_origin_before_reaching_build,
//! not here.

use std::process::ExitStatus;

use reify_test_support::LENGTH_SLOT_DIAGNOSTIC_MARKER;

use crate::common;

/// The line `finish_check` prints when no constraint failed.
const CLEAN_SUMMARY: &str = "All constraints satisfied.";

/// Which layer rejects the bare form.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Layer {
    /// A compile-time LENGTH slot: `check` returns before evaluating anything.
    Compiler,
    /// No compile-time slot: realization rejects it after `check`'s verdict.
    Eval,
}

/// A constructor called with bare numbers, its dimensioned migration, and the
/// LENGTH slots the bare call must be blamed for.
struct Row {
    bare: &'static str,
    dimensioned: &'static str,
    layer: Layer,
    blamed: &'static [&'static str],
}

#[rustfmt::skip]
const ROWS: &[Row] = &[
    Row { bare: "box(20, 20, 10)", dimensioned: "box(20mm, 20mm, 10mm)", layer: Layer::Compiler, blamed: &["size_x", "size_y", "size_z"] },
    Row { bare: "translate(g, 0, 0, 5)", dimensioned: "translate(g, 0mm, 0mm, 5mm)", layer: Layer::Compiler, blamed: &["dx", "dy", "dz"] },
    Row { bare: "fillet(box(10mm, 10mm, 10mm), 1)", dimensioned: "fillet(box(10mm, 10mm, 10mm), 1mm)", layer: Layer::Compiler, blamed: &["radius"] },
    Row { bare: "rotate_around(g, 0, 0, 0, 0, 0, 1, 45deg)", dimensioned: "rotate_around(g, 0mm, 0mm, 0mm, 0, 0, 1, 45deg)", layer: Layer::Compiler, blamed: &["px", "py", "pz"] },
    Row { bare: "helix(10, 2, 50)", dimensioned: "helix(10mm, 2mm, 50mm)", layer: Layer::Eval, blamed: &["radius", "pitch", "height"] },
    Row { bare: "polygon(0, 0, 10, 0, 5, 10)", dimensioned: "polygon(0mm, 0mm, 10mm, 0mm, 5mm, 10mm)", layer: Layer::Eval, blamed: &["x1", "y1", "x2", "y2", "x3", "y3"] },
    Row { bare: "arc(0, 0, 0, 10, 0deg, 90deg, 0, 0, 1)", dimensioned: "arc(0mm, 0mm, 0mm, 10mm, 0deg, 90deg, 0, 0, 1)", layer: Layer::Eval, blamed: &["cx", "cy", "cz", "radius"] },
    Row { bare: "line_segment(0, 0, 0, 0, 200, 0)", dimensioned: "line_segment(0mm, 0mm, 0mm, 0mm, 200mm, 0mm)", layer: Layer::Eval, blamed: &["x1", "y1", "z1", "x2", "y2", "z2"] },
    Row { bare: "interp(0, 0, 0, 10, 5, 0)", dimensioned: "interp(0mm, 0mm, 0mm, 10mm, 5mm, 0mm)", layer: Layer::Eval, blamed: &["x1", "y1", "z1", "x2", "y2", "z2"] },
    Row { bare: "bezier(0, 0, 0, 10, 5, 0)", dimensioned: "bezier(0mm, 0mm, 0mm, 10mm, 5mm, 0mm)", layer: Layer::Eval, blamed: &["x1", "y1", "z1", "x2", "y2", "z2"] },
    Row { bare: "nurbs(1, 2, 0, 0, 0, 10, 0, 0, 1, 1, 0, 0, 1, 1)", dimensioned: "nurbs(1, 2, 0mm, 0mm, 0mm, 10mm, 0mm, 0mm, 1, 1, 0, 0, 1, 1)", layer: Layer::Eval, blamed: &["x1", "y1", "z1", "x2", "y2", "z2"] },
];

/// Run `reify check` on a temp module whose only questionable member is
/// `let probe = <form>`.
///
/// A temp module rather than a `tests/fixtures/` file because the bare forms
/// are deliberately uncompilable, and every `.ri` under `crates/**` is swept by
/// the corpus walkers.
fn check_form(form: &str) -> (ExitStatus, String, String) {
    let dir = tempfile::tempdir().expect("failed to create temp dir");
    // The stem must match the `module` declaration, or `check` reports
    // E_MODULE_PATH_MISMATCH instead.
    let path = dir.path().join("bare_length_probe.ri");
    std::fs::write(
        &path,
        format!(
            r#"module bare_length_probe

structure def BareLengthProbe {{
    param w : Length = 20mm
    let g = box(10mm, 10mm, 10mm)
    let probe = {form}
    constraint w > 1mm
}}
"#
        ),
    )
    .expect("failed to write temp module");
    common::run_with_args(&["check", path.to_str().expect("temp path is UTF-8")])
}

/// The bare form exits 1, blames every LENGTH slot, and stops in `row.layer`;
/// its dimensioned twin, in the identical wrapper, exits 0 with a clean stderr.
fn assert_layer_rejects(row: &Row) {
    let bare = row.bare;
    if row.layer == Layer::Eval && !reify_kernel_occt::OCCT_AVAILABLE {
        // The eval-layer rejection comes from the kernel-backed realize pass,
        // which is unmeasured in a stub build.
        eprintln!(
            "skipping eval-layer bare-length exit assertions for {bare}: OCCT \
             unavailable (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    let (status, stdout, stderr) = check_form(bare);
    assert_eq!(
        status.code(),
        Some(1),
        "a bare number in a LENGTH slot must make `reify check` exit exactly 1 \
         for {bare}.\nstdout: {stdout}\nstderr: {stderr}"
    );
    let op = bare.split_once('(').map_or(bare, |(op, _)| op);
    for slot in row.blamed {
        let needle = format!("{op}: {slot}{LENGTH_SLOT_DIAGNOSTIC_MARKER}");
        assert!(
            stderr.contains(&needle),
            "every bare LENGTH slot of {bare} must be blamed; `{needle}` is \
             missing.\nstdout: {stdout}\nstderr: {stderr}"
        );
    }
    match row.layer {
        Layer::Compiler => assert!(
            !stdout.contains(CLEAN_SUMMARY),
            "{bare} must be stopped by the compiler, before evaluation prints a \
             verdict.\nstdout: {stdout}\nstderr: {stderr}"
        ),
        Layer::Eval => assert!(
            stdout.contains(CLEAN_SUMMARY),
            "{bare} must evaluate to a clean verdict, so that only the post-verdict \
             Severity::Error gate can produce exit 1.\nstdout: {stdout}\nstderr: {stderr}"
        ),
    }

    let dimensioned = row.dimensioned;
    let (status, stdout, stderr) = check_form(dimensioned);
    assert_eq!(
        status.code(),
        Some(0),
        "the dimensioned twin of {bare}, {dimensioned}, must pass `reify check`, \
         or the bare form's exit 1 is not attributable to its units.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains(CLEAN_SUMMARY),
        "the dimensioned twin of {bare}, {dimensioned}, must reach a clean \
         verdict.\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        !stderr.lines().any(|line| line.starts_with("error:")),
        "the dimensioned twin of {bare}, {dimensioned}, must report no error.\n\
         stdout: {stdout}\nstderr: {stderr}"
    );
}

fn assert_every_row_of(layer: Layer) {
    let rows: Vec<&Row> = ROWS.iter().filter(|row| row.layer == layer).collect();
    assert!(
        !rows.is_empty(),
        "no {layer:?}-layer row: this test would pass vacuously"
    );
    for row in rows {
        assert_layer_rejects(row);
    }
}

#[test]
fn check_exits_1_before_its_verdict_on_a_bare_compiler_checked_length() {
    assert_every_row_of(Layer::Compiler);
}

#[test]
fn check_exits_1_after_its_verdict_on_a_bare_eval_checked_length() {
    assert_every_row_of(Layer::Eval);
}
