//! Integration test: a `helix()` wire must be a usable sweep spine.
//!
//! Root-cause discriminator for task #5342. `make_helix_wire`
//! (crates/reify-kernel-occt/cpp/occt_wrapper.cpp) builds its edge from a
//! `Geom2d_Line` on a `Geom_CylindricalSurface`; that edge carries only a
//! pcurve (curve-on-surface) and NO `Geom_Curve` 3D representation. Every
//! downstream consumer that needs the 3D curve — including
//! `BRepOffsetAPI_MakePipe` behind `GeometryOp::Sweep` — throws a
//! `Standard_Failure` with an empty message, surfacing as the empty-tail
//! error `"OCCT make_pipe_with_history: "`. The fix is one call to
//! `BRepLib::BuildCurves3d` inside `make_helix_wire`.
//!
//! We exercise the fix through `GeometryOp::Sweep { profile, path }` (an
//! explicit posed profile along an arbitrarily-oriented spine), NOT
//! `GeometryOp::Pipe { path, radius }` — Pipe applies a +Z start-tangent
//! guard that rejects a helix spine (its start tangent is ~+Y), which is a
//! separate concern. Sweep has no such guard, so it isolates THIS bug.
//!
//! Mirrors the OCCT-gated skeleton of `sweep_with_history_integration.rs` /
//! `curve_constructors_integration.rs`: `#![cfg(has_occt)]` plus an
//! `OCCT_AVAILABLE` early-return so non-OCCT builds skip cleanly.
//!
//! RED before the fix: the `Sweep` execute returns `Err` and the `.expect`
//! panics before the volume assertion is ever reached.

#![cfg(has_occt)]

use reify_ir::{BRepKind, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernel};

/// Sweep a centred circular profile (r = 3mm), posed perpendicular-ish to the
/// helix start tangent, along a `helix(R = 24mm, pitch = 7mm, height = 63mm)`
/// spine and assert a positive-volume solid whose volume matches Pappus's
/// centroid theorem `V = π·r²·L` (L = spine arc length) within 5%.
///
/// Pose: the `CircleProfile` face has a +Z normal centred at the origin.
/// Rotating about +X by −π/2 takes that normal to +Y (≈ the helix start
/// tangent at (R,0,0)); translating by +R along X places the profile centre
/// on the helix start point. Pitch (7mm) exceeds the profile diameter (6mm),
/// so successive turns clear each other and the sweep does not self-intersect.
#[test]
fn helix_wire_sweeps_to_positive_volume_solid() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernel::new();

    // (a) Circle-face profile, r = 3mm, +Z normal, centred at origin.
    let profile = kernel
        .execute(&GeometryOp::CircleProfile {
            radius: Value::Real(0.003),
        })
        .expect("CircleProfile (3mm) should build");

    // (b) Rotate the +Z normal to +Y (≈ helix start tangent): about +X by −π/2.
    let rotated = kernel
        .execute(&GeometryOp::Rotate {
            target: profile.id,
            axis: [1.0, 0.0, 0.0],
            angle_rad: -std::f64::consts::FRAC_PI_2,
        })
        .expect("Rotate of the circle profile should succeed");

    // (c) Translate the profile centre onto the helix start point (R, 0, 0).
    let posed = kernel
        .execute(&GeometryOp::Translate {
            target: rotated.id,
            dx: 0.024,
            dy: 0.0,
            dz: 0.0,
        })
        .expect("Translate of the circle profile should succeed");

    // (d) Helix spine: R = 24mm, pitch = 7mm, height = 63mm (9 full turns).
    let helix = kernel
        .execute(&GeometryOp::Helix {
            radius: 0.024,
            pitch: 0.007,
            height: 0.063,
        })
        .expect("Helix spine should build");

    // (e) Sweep the posed profile along the helix. THIS is the line that
    // fails before the fix: the helix wire has no 3D curve, so make_pipe
    // throws and execute returns Err.
    let swept = kernel
        .execute(&GeometryOp::Sweep {
            profile: posed.id,
            path: helix.id,
        })
        .expect("sweep along helix should succeed once make_helix_wire builds a 3D curve");

    // Result must be a solid.
    assert_eq!(
        swept.repr,
        Some(BRepKind::Solid),
        "swept helix profile should yield a Solid, got {:?}",
        swept.repr
    );

    // Volume ≈ π·r²·L by Pappus (perpendicular, centred, non-self-intersecting
    // circular sweep). L = helical arc length = sqrt((2π·R·n)² + height²),
    // n = height/pitch = 9 turns.
    let vol = kernel
        .query(&GeometryQuery::Volume(swept.id))
        .expect("Volume query on the swept solid should succeed");
    let v = vol.as_f64().expect("volume value should be numeric");

    let n = 0.063 / 0.007;
    let l = ((2.0 * std::f64::consts::PI * 0.024 * n).powi(2) + 0.063_f64.powi(2)).sqrt();
    let expected = std::f64::consts::PI * 0.003_f64.powi(2) * l;
    let rel_err = (v - expected).abs() / expected;
    assert!(
        v > 0.0 && rel_err < 0.05,
        "helix-swept volume should be ≈{:.4e} m³ (π·r²·L, L={:.4} m), got {:.4e} (rel_err={:.4})",
        expected,
        l,
        v,
        rel_err
    );
}
