//! Integration tests for the sweep_guided operation via the public
//! OcctKernel API.
//!
//! These tests exercise GeometryOp::SweepGuided through
//! OcctKernel::execute(), verifying both the happy path
//! (non-degenerate shape, finite bounding box) and the semantic
//! distinction from plain Sweep (the guide wire changes the result,
//! so centroids differ).
//!
//! Notes:
//! - The profile is a closed circular wire (Arc from 0 to 2π), so the
//!   resulting pipe-shell is a closed tube with positive volume.
//! - The guide wire is intentionally non-parallel to the path spine so
//!   that MakePipeShell's SetMode(aux, false) has visible effect.
//!
//! Coverage note: `sweep_guided_produces_valid_shape` and
//! `sweep_guided_orientation_differs_from_plain_sweep` build their profile
//! from `GeometryOp::Arc`, i.e. a closed circular **wire**. No DSL program can
//! reach that input — the compiler requires `sweep_guided`'s first argument
//! to be a `Surface`, and `circle(r)` lowers to `CircleProfile` →
//! `make_circle_face` → a `TopoDS_Face`. Since `BRepFill_Section` (used
//! internally by `BRepOffsetAPI_MakePipeShell`) accepts only a wire or a
//! vertex, a wire profile is the one shape that happened to work, so the
//! wire-only tests were green while every DSL call failed. The
//! `*_face_profile*` tests below close that hole by driving the op with the
//! face the DSL actually produces.

#![cfg(has_occt)]

use crate::common;
use reify_kernel_occt::OcctKernel;
use reify_ir::{GeometryError, GeometryHandleId, GeometryOp, GeometryQuery, Value};

/// Build a closed circular wire profile at z=0 of the given radius.
fn make_circle_profile(kernel: &mut OcctKernel, radius: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Arc {
            center: [0.0, 0.0, 0.0],
            radius,
            start_angle: 0.0,
            end_angle: 2.0 * std::f64::consts::PI,
            axis: [0.0, 0.0, 1.0],
        })
        .expect("Arc (full circle) creation should succeed")
        .id
}

/// Build a straight line-segment path along +Z from (0,0,0) to
/// (0,0,`length`).
fn make_straight_path(kernel: &mut OcctKernel, length: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::LineSegment {
            x1: 0.0,
            y1: 0.0,
            z1: 0.0,
            x2: 0.0,
            y2: 0.0,
            z2: length,
        })
        .expect("LineSegment (path) creation should succeed")
        .id
}

/// Build an auxiliary-spine guide wire clearly offset from the main
/// spine. MakePipeShell's auxiliary spine must not be coincident with
/// the spine at any parameter (otherwise OCCT reports
/// "gp_Vec::Normalized() - vector has zero norm") so both endpoints
/// must be offset in X. `dx_start`/`dx_end` vary the offset along the
/// parameter so the section orientation is non-constant and the guide
/// meaningfully biases the result.
fn make_offset_guide(
    kernel: &mut OcctKernel,
    dx_start: f64,
    dx_end: f64,
    length: f64,
) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::LineSegment {
            x1: dx_start,
            y1: 0.0,
            z1: 0.0,
            x2: dx_end,
            y2: 0.0,
            z2: length,
        })
        .expect("LineSegment (guide) creation should succeed")
        .id
}

/// Query the bounding box of `id`, assert every component is finite, and
/// return the (x_span, y_span, z_span) extents.
///
/// Thin wrapper over the shared [`common::bbox_of`] parser (task 5893), which
/// replaced the file-local parsers this task's earlier revision had grown.
/// Kept as a named helper only so the extent assertions below read as one call.
fn bbox_spans(kernel: &OcctKernel, id: GeometryHandleId) -> (f64, f64, f64) {
    let bbox = common::bbox_of(kernel.query(&GeometryQuery::BoundingBox(id)));
    assert!(
        bbox.all_finite(),
        "bbox components must all be finite, got {bbox:?}"
    );
    bbox.spans()
}

/// Parse a JSON-encoded centroid string `{"x":…,"y":…,"z":…}` into (x, y, z).
fn parse_centroid(s: &str) -> (f64, f64, f64) {
    let inner = s.trim_start_matches('{').trim_end_matches('}');
    let mut x = f64::NAN;
    let mut y = f64::NAN;
    let mut z = f64::NAN;
    for pair in inner.split(',') {
        let mut parts = pair.splitn(2, ':');
        let key = parts.next().unwrap().trim().trim_matches('"');
        let val: f64 = parts.next().unwrap().trim().parse().unwrap();
        match key {
            "x" => x = val,
            "y" => y = val,
            "z" => z = val,
            _ => {}
        }
    }
    (x, y, z)
}

#[test]
fn sweep_guided_produces_valid_shape() {
    let mut kernel = OcctKernel::new();
    let profile = make_circle_profile(&mut kernel, 0.02);
    let path = make_straight_path(&mut kernel, 0.1);
    let guide = make_offset_guide(&mut kernel, 0.05, 0.03, 0.1);

    let result = kernel
        .execute(&GeometryOp::SweepGuided {
            profile,
            path,
            guide,
        })
        .expect("SweepGuided should succeed");

    // The pipe-shell should have a finite, non-degenerate bounding box.
    let bbox = common::bbox_of(kernel.query(&GeometryQuery::BoundingBox(result.id)));
    assert!(
        bbox.all_finite(),
        "bbox components must all be finite, got {bbox:?}"
    );
}

/// A `CircleProfile` face — the only profile a DSL `sweep_guided()` call can
/// produce — must be accepted. `BRepFill_Section` takes a wire or a vertex
/// only, so the face has to be reduced to its outer wire before `Add`.
///
/// This is the *shape-level* half of the face-profile contract: the result is
/// topologically closed and its extents match the swept disk. The magnitude
/// half (analytic volume, parity with plain `Sweep`) lives in
/// `sweep_guided_face_profile_yields_solid_volume` below; the two are
/// complementary — a closed shape of the wrong size passes here and fails
/// there, and vice versa.
#[test]
fn sweep_guided_accepts_circle_face_profile() {
    let mut kernel = OcctKernel::new();
    let profile = kernel
        .execute(&GeometryOp::CircleProfile {
            radius: Value::Real(0.02),
        })
        .expect("CircleProfile (20mm) should build")
        .id;
    let path = make_straight_path(&mut kernel, 0.1);
    let guide = make_offset_guide(&mut kernel, 0.05, 0.03, 0.1);

    let result = match kernel.execute(&GeometryOp::SweepGuided {
        profile,
        path,
        guide,
    }) {
        Ok(r) => r,
        Err(e) => panic!("SweepGuided with a CircleProfile (face) must succeed, got: {e}"),
    };

    // A face profile must yield a *closed* result — an un-capped shell has
    // free edges at both ends and reports false here. This distinguishes the
    // solidified face path from the raw-shell wire path without relying on a
    // volume magnitude.
    match kernel
        .query(&GeometryQuery::IsClosed(result.id))
        .expect("IsClosed query should succeed")
    {
        Value::Bool(closed) => assert!(
            closed,
            "a face profile must sweep to a closed (capped) result, got an open shell"
        ),
        other => panic!("expected IsClosed Bool, got {:?}", other),
    }

    // Extents. The swept tube must be at least as wide as its own section
    // (2r = 0.04) on both transverse axes, and must follow the 0.1 m spine in
    // Z. The upper bounds are loose sanity rails, not pins: the guide swings
    // the sections about the spine, so the X extent measures ~0.075 here
    // rather than 0.04 — the point of these assertions is to catch a
    // collapsed or exploded result, not to fix the guide's law.
    let (x_span, y_span, z_span) = bbox_spans(&kernel, result.id);
    for (axis, span) in [("x", x_span), ("y", y_span)] {
        assert!(
            (0.039..0.25).contains(&span),
            "{axis}-span must be at least the section diameter (2r = 0.04) and \
             not exploded, got {span}"
        );
    }
    assert!(
        (0.09..0.15).contains(&z_span),
        "z-span should track the 0.1 m spine, got {z_span}"
    );
}

/// A face profile must sweep to an *enclosed solid*, at parity with plain
/// `sweep()` — the declared stdlib signature is
/// `sweep_guided(profile: Surface, path: Curve, guide: Curve) -> Solid`.
///
/// Tolerance basis: this is a straight 0.1 m spine, so the exact answer is
/// analytic — V = π·r²·L = 1.2566370614e-4 m³ for r = 0.02. A standalone OCCT
/// probe measured the guided sweep at 1.25663673e-4 (rel. err. 2.6e-7) and
/// `MakePipe(face)` at 1.25663706e-4 (rel. err. 4.4e-9). The 1e-3 relative
/// tolerance below therefore carries ~4 orders of margin over the observed
/// discretisation error; it is derived from the geometry, not tuned to fit.
#[test]
fn sweep_guided_face_profile_yields_solid_volume() {
    let mut kernel = OcctKernel::new();
    let profile = kernel
        .execute(&GeometryOp::CircleProfile {
            radius: Value::Real(0.02),
        })
        .expect("CircleProfile (20mm) should build")
        .id;
    let path = make_straight_path(&mut kernel, 0.1);
    let guide = make_offset_guide(&mut kernel, 0.05, 0.03, 0.1);

    let guided = kernel
        .execute(&GeometryOp::SweepGuided {
            profile,
            path,
            guide,
        })
        .expect("SweepGuided with a face profile should succeed");
    let guided_volume = match kernel
        .query(&GeometryQuery::Volume(guided.id))
        .expect("Volume query should succeed")
    {
        Value::Real(v) => v,
        other => panic!("expected Volume Real, got {:?}", other),
    };

    // (a) against the analytic volume of the swept disk.
    let analytic = std::f64::consts::PI * 0.02 * 0.02 * 0.1;
    let rel_err = (guided_volume - analytic).abs() / analytic;
    assert!(
        rel_err < 1e-3,
        "guided sweep of a face profile must enclose the analytic volume \
         {analytic}, got {guided_volume} (relative error {rel_err}); an open \
         shell measures ~8.4e-5 here"
    );

    // (b) parity with plain Sweep, which OCCT already solidifies for a face
    // profile. Fresh inputs — MakePipeShell consumes the ones above.
    let profile_plain = kernel
        .execute(&GeometryOp::CircleProfile {
            radius: Value::Real(0.02),
        })
        .expect("CircleProfile (20mm) should build")
        .id;
    let path_plain = make_straight_path(&mut kernel, 0.1);
    let plain = kernel
        .execute(&GeometryOp::Sweep {
            profile: profile_plain,
            path: path_plain,
        })
        .expect("plain Sweep should succeed");
    let plain_volume = match kernel
        .query(&GeometryQuery::Volume(plain.id))
        .expect("Volume query should succeed")
    {
        Value::Real(v) => v,
        other => panic!("expected Volume Real, got {:?}", other),
    };
    let parity_err = (guided_volume - plain_volume).abs() / plain_volume;
    assert!(
        parity_err < 1e-3,
        "guided sweep volume {guided_volume} must match plain sweep volume \
         {plain_volume} for the same face profile (relative error {parity_err})"
    );
}

/// Run a `SweepGuided` that must be rejected, and assert the diagnostic is
/// Reify-authored: it names `expected_type`, attributes `make_pipe_shell`, and
/// leaks neither OCCT's opaque `BRepFill_Section` wording nor the "unexpected"
/// framing `wrap_occt_call` reserves for genuinely unforeseen exceptions.
fn assert_profile_rejected(
    kernel: &mut OcctKernel,
    profile: GeometryHandleId,
    expected_type: &str,
) -> String {
    let path = make_straight_path(kernel, 0.1);
    let guide = make_offset_guide(kernel, 0.05, 0.03, 0.1);

    match kernel.execute(&GeometryOp::SweepGuided {
        profile,
        path,
        guide,
    }) {
        Err(GeometryError::OperationFailed(msg)) => {
            let lower = msg.to_lowercase();
            assert!(
                lower.contains(&expected_type.to_lowercase()),
                "error must name the offending shape type ('{expected_type}'), got: {msg}"
            );
            assert!(
                msg.contains("make_pipe_shell"),
                "error must attribute the failing op, got: {msg}"
            );
            assert!(
                !msg.contains("BRepFill_Section"),
                "error must be a Reify diagnostic, not OCCT's opaque internal \
                 wording, got: {msg}"
            );
            assert!(
                !lower.contains("unexpected"),
                "a known contract violation must not be framed as an unexpected \
                 OCCT exception, got: {msg}"
            );
            msg
        }
        Ok(_) => panic!("expected OperationFailed for a {expected_type} profile, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

/// Every profile type `BRepFill_Section` genuinely cannot take must be
/// rejected with a Reify-authored diagnostic naming the offending shape type.
///
/// Covers EDGE as well as SOLID: this fix narrowed a previously documented
/// contract (the old comment claimed "profile may be an edge, wire, or face"),
/// so the EDGE branch is exactly the one whose status changed and must be
/// pinned.
#[test]
fn sweep_guided_rejects_unsupported_profile_type() {
    // SOLID — the classic wrong-dimensionality profile.
    let mut kernel = OcctKernel::new();
    let solid = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(0.01),
            height: Value::Real(0.01),
            depth: Value::Real(0.01),
        })
        .expect("Box should build")
        .id;
    assert_profile_rejected(&mut kernel, solid, "Solid");

    // EDGE — obtained by topology extraction from a circle profile face, the
    // same route by which a selector can hand an edge handle to an op.
    let mut kernel = OcctKernel::new();
    let face = kernel
        .execute(&GeometryOp::CircleProfile {
            radius: Value::Real(0.02),
        })
        .expect("CircleProfile (20mm) should build")
        .id;
    let edges = kernel
        .extract_edges(face)
        .expect("extract_edges on a circle face should succeed");
    let edge = *edges
        .first()
        .expect("a circle profile face has at least one edge");
    assert_profile_rejected(&mut kernel, edge, "Edge");
}

/// A face carrying inner (hole) wires must be rejected, not silently reduced
/// to its outer wire.
///
/// `BRepTools::OuterWire` would happily drop the holes, and — because a face
/// profile is then solidified — an annular profile would sweep to a *filled*
/// solid: plausible-looking, wrong geometry, with no diagnostic. The compiler's
/// own profile ops are all single-wire, but an extracted face
/// (`extract_faces`, `BRepKind::Face`) is `Surface`-typed and can carry them,
/// so this input is reachable.
#[test]
fn sweep_guided_rejects_holed_face_profile() {
    let mut kernel = OcctKernel::new();
    // A tube's flat end caps are annuli — an outer wire plus one inner wire.
    let tube = kernel
        .execute(&GeometryOp::Tube {
            outer_r: Value::Real(0.03),
            inner_r: Value::Real(0.015),
            height: Value::Real(0.05),
        })
        .expect("Tube should build")
        .id;
    let faces = kernel
        .extract_faces(tube)
        .expect("extract_faces on a tube should succeed");
    let annulus = faces
        .iter()
        .copied()
        .find(|f| {
            matches!(
                kernel.query(&GeometryQuery::FaceSurfaceKind(*f)),
                Ok(Value::String(ref k)) if k == "Plane"
            )
        })
        .expect("a tube has planar annular end faces");

    let msg = assert_profile_rejected(&mut kernel, annulus, "inner wire");
    assert!(
        msg.contains("discard"),
        "diagnostic should explain that the hole(s) would be discarded, got: {msg}"
    );
}

#[test]
fn sweep_guided_orientation_differs_from_plain_sweep() {
    let mut kernel = OcctKernel::new();
    let profile_plain = make_circle_profile(&mut kernel, 0.02);
    let path_plain = make_straight_path(&mut kernel, 0.1);
    let plain = kernel
        .execute(&GeometryOp::Sweep {
            profile: profile_plain,
            path: path_plain,
        })
        .expect("plain Sweep should succeed");
    let plain_centroid = kernel
        .query(&GeometryQuery::Centroid(plain.id))
        .expect("plain Sweep centroid should query");
    let (plain_x, plain_y, plain_z) = match plain_centroid {
        Value::String(s) => parse_centroid(&s),
        other => panic!("expected centroid String, got {:?}", other),
    };

    // Fresh profile/path for the guided sweep — MakePipeShell consumes
    // its inputs and we've already fed these to plain Sweep.
    let profile_g = make_circle_profile(&mut kernel, 0.02);
    let path_g = make_straight_path(&mut kernel, 0.1);
    let guide_g = make_offset_guide(&mut kernel, 0.06, 0.03, 0.1);
    let guided = kernel
        .execute(&GeometryOp::SweepGuided {
            profile: profile_g,
            path: path_g,
            guide: guide_g,
        })
        .expect("SweepGuided should succeed");
    let guided_centroid = kernel
        .query(&GeometryQuery::Centroid(guided.id))
        .expect("SweepGuided centroid should query");
    let (g_x, g_y, g_z) = match guided_centroid {
        Value::String(s) => parse_centroid(&s),
        other => panic!("expected centroid String, got {:?}", other),
    };

    // Centroids should differ — the guide wire biases orientation, which
    // in turn shifts the centroid away from the plain Sweep result.
    // For a rotation-symmetric circular profile the shift is small
    // (~3e-7 m, reflecting MakePipeShell's section parameterization vs
    // plain MakePipe), but the delta is robustly non-zero and several
    // orders of magnitude above OCCT's centroid numerical noise (~1e-12),
    // so the threshold of 1e-8 reliably detects the guide's influence.
    let dx = (g_x - plain_x).abs();
    let dy = (g_y - plain_y).abs();
    let dz = (g_z - plain_z).abs();
    let delta = (dx * dx + dy * dy + dz * dz).sqrt();
    assert!(
        delta > 1e-8,
        "guided centroid ({g_x}, {g_y}, {g_z}) should differ from plain \
         centroid ({plain_x}, {plain_y}, {plain_z}); delta = {delta}"
    );
}
