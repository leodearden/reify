//! The user-facing positional order of the box constructors binds to the
//! kernel's axis-explicit `GeometryOp::Box` slots: `box(width, depth, height)`
//! sets `size_x`, `size_y`, `size_z` (the X, Y, Z extents), as documented in
//! the geometry reference chunk.
//!
//! Every fixture uses three DISTINCT per-axis values, so any transposition
//! anywhere between the parser and the kernel boundary fails. Runs against the
//! mock kernel, so it needs no native kernel and always executes.

use reify_core::Severity;
use reify_ir::GeometryOp;
use reify_test_support::{build_against_mock_kernel, parse_and_compile};

const TOL: f64 = 1e-12;

/// Build `source` and return the `(size_x, size_y, size_z)` SI extents of every
/// `Box` op that reached the kernel, after asserting the build had no errors.
#[track_caller]
fn built_box_extents(source: &str) -> Vec<(f64, f64, f64)> {
    let (diagnostics, ops) = build_against_mock_kernel(parse_and_compile(source));
    let errors: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "build errors for {source:?}: {errors:?}");

    let si = |v: &reify_ir::Value| {
        v.as_f64()
            .unwrap_or_else(|| panic!("box slot must carry a numeric SI value; got {v:?}"))
    };
    ops.iter()
        .filter_map(|op| match op {
            GeometryOp::Box {
                size_x,
                size_y,
                size_z,
            } => Some((si(size_x), si(size_y), si(size_z))),
            _ => None,
        })
        .collect()
}

fn approx_eq(a: (f64, f64, f64), b: (f64, f64, f64)) -> bool {
    (a.0 - b.0).abs() < TOL && (a.1 - b.1).abs() < TOL && (a.2 - b.2).abs() < TOL
}

#[test]
fn box_positional_width_depth_height_bind_to_size_x_y_z() {
    for ctor in ["box", "box_centered"] {
        let source = format!("structure def S {{ let body = {ctor}(10mm, 20mm, 30mm) }}");
        let boxes = built_box_extents(&source);
        assert_eq!(
            boxes.len(),
            1,
            "{ctor}: expected exactly one Box op; got {boxes:?}"
        );
        assert!(
            approx_eq(boxes[0], (0.01, 0.02, 0.03)),
            "{ctor}(10mm, 20mm, 30mm) must reach the kernel as \
             size_x = 0.01, size_y = 0.02, size_z = 0.03; got {:?}",
            boxes[0]
        );
    }
}

/// `rounded_box(width, depth, height, r)` lowers to two overlapping boxes, one
/// shrunk by `2r` along Y and one along X; `height` is the Z extent of both.
#[test]
fn rounded_box_height_drives_size_z_of_both_bodies() {
    let boxes =
        built_box_extents("structure def S { let body = rounded_box(40mm, 30mm, 10mm, 2mm) }");
    assert_eq!(
        boxes.len(),
        2,
        "expected exactly two Box ops; got {boxes:?}"
    );

    let expected = [(0.04, 0.026, 0.01), (0.036, 0.030, 0.01)];
    for want in expected {
        let matches = boxes.iter().filter(|got| approx_eq(**got, want)).count();
        assert_eq!(
            matches, 1,
            "expected exactly one Box with (size_x, size_y, size_z) = {want:?}; got {boxes:?}"
        );
    }
}
