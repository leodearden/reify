//! Integration tests for the loft_guided operation via the public
//! OcctKernel API.
//!
//! These tests exercise GeometryOp::LoftGuided through
//! OcctKernel::execute(), verifying the happy path (≥2 profile
//! sections, 1 guide producing a finite, non-degenerate shape whose
//! bounding box spans the section profiles) and the validation error
//! paths (missing profiles, missing guide).
//!
//! LoftGuided is implemented via `BRepOffsetAPI_MakePipeShell` with the
//! first guide wire as the spine and each profile added as a section;
//! an optional second guide is passed to `SetMode(aux_wire, false)` as
//! auxiliary-spine constraint.
//!
//! Notes:
//! - Profiles are closed circular wires built from `GeometryOp::Arc`
//!   (full circle, 0 → 2π) at two different z heights so the loft has
//!   non-zero extent along Z.
//! - The guide wire (spine) is a straight line along +Z connecting the
//!   two profile planes, so MakePipeShell has a well-defined spine
//!   traversal.

#![cfg(has_occt)]

use reify_kernel_occt::OcctKernel;
use reify_ir::{GeometryError, GeometryHandleId, GeometryOp, GeometryQuery, Value};

/// Build a closed circular wire profile of the given radius at the
/// given z height, centred on the Z-axis.
fn make_circle_profile(kernel: &mut OcctKernel, radius: f64, z: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::Arc {
            center: [0.0, 0.0, z],
            radius,
            start_angle: 0.0,
            end_angle: 2.0 * std::f64::consts::PI,
            axis: [0.0, 0.0, 1.0],
        })
        .expect("Arc (full circle) creation should succeed")
        .id
}

/// Build a straight line-segment guide wire connecting (0,0,0) to
/// (0,0,length). Used as the spine for LoftGuided.
fn make_spine(kernel: &mut OcctKernel, length: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::LineSegment {
            x1: 0.0,
            y1: 0.0,
            z1: 0.0,
            x2: 0.0,
            y2: 0.0,
            z2: length,
        })
        .expect("LineSegment (spine) creation should succeed")
        .id
}

/// Parse a JSON-encoded bounding box string
/// `{"xmin":…,"ymin":…,"zmin":…,"xmax":…,"ymax":…,"zmax":…}` into
/// (zmin, zmax). We only need Z for the span assertion.
fn parse_bbox_z(s: &str) -> (f64, f64) {
    let mut zmin = f64::NAN;
    let mut zmax = f64::NAN;
    let trimmed = s.trim_start_matches('{').trim_end_matches('}');
    for pair in trimmed.split(',') {
        let mut parts = pair.splitn(2, ':');
        let key = parts.next().unwrap().trim().trim_matches('"');
        let val: f64 = parts.next().unwrap().trim().parse().unwrap();
        match key {
            "zmin" => zmin = val,
            "zmax" => zmax = val,
            _ => {}
        }
    }
    (zmin, zmax)
}

/// The core happy-path assertion: LoftGuided produces a finite,
/// non-degenerate shape whose bounding box spans both profile z heights.
#[test]
fn loft_guided_smooth_surface_between_profiles() {
    let mut kernel = OcctKernel::new();
    // Two coplanar (XY-plane, at different z) circular profiles.
    let p1 = make_circle_profile(&mut kernel, 0.02, 0.0);
    let p2 = make_circle_profile(&mut kernel, 0.01, 0.1);
    // Spine connects the two profile planes.
    let spine = make_spine(&mut kernel, 0.1);

    let result = kernel
        .execute(&GeometryOp::LoftGuided {
            profiles: vec![p1, p2],
            guides: vec![spine],
        })
        .expect("LoftGuided should succeed for 2 profiles + 1 guide");

    // Bounding box: finite extents that span both profiles in Z.
    let bbox = kernel
        .query(&GeometryQuery::BoundingBox(result.id))
        .expect("bbox query should succeed");
    let (zmin, zmax) = match bbox {
        Value::String(s) => parse_bbox_z(&s),
        other => panic!("expected bbox String, got {:?}", other),
    };
    assert!(
        zmin.is_finite() && zmax.is_finite(),
        "loft_guided bbox z-extent must be finite, got [{zmin}, {zmax}]"
    );
    // The lofted shape should span at least from ~0 to ~0.1 in Z
    // (profile-1 at z=0, profile-2 at z=0.1). Allow a small numerical
    // margin for OCCT's bbox padding.
    let span = zmax - zmin;
    assert!(
        span > 0.09,
        "loft_guided bbox z-span should cover both profiles (≥0.09), got {span} from [{zmin}, {zmax}]"
    );
}

#[test]
fn loft_guided_requires_two_profiles_error() {
    let mut kernel = OcctKernel::new();
    let p1 = make_circle_profile(&mut kernel, 0.02, 0.0);
    let spine = make_spine(&mut kernel, 0.1);

    let result = kernel.execute(&GeometryOp::LoftGuided {
        profiles: vec![p1], // only 1 profile — invalid
        guides: vec![spine],
    });

    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.to_lowercase().contains("profile"),
                "expected error mentioning 'profile', got: {msg}"
            );
        }
        Ok(_) => panic!("expected OperationFailed with 1 profile, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

#[test]
fn loft_guided_requires_one_guide_error() {
    let mut kernel = OcctKernel::new();
    let p1 = make_circle_profile(&mut kernel, 0.02, 0.0);
    let p2 = make_circle_profile(&mut kernel, 0.01, 0.1);

    let result = kernel.execute(&GeometryOp::LoftGuided {
        profiles: vec![p1, p2],
        guides: vec![], // missing — invalid
    });

    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("guide") || msg.contains("1"),
                "expected error mentioning guide/1, got: {msg}"
            );
        }
        Ok(_) => panic!("expected OperationFailed with 0 guides, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}
