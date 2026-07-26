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

/// Pipe the SAME `helix(R = 24mm, pitch = 7mm, height = 63mm)` spine with a
/// 3mm circular cross-section, asserting the same `V = π·r²·L` — but with NO
/// manual profile posing. Contrast the Sweep test above, which must build a
/// `CircleProfile`, `Rotate` it about +X by −π/2, then `Translate` it by +R
/// along X (its steps (a)–(c)) before it can sweep at all. `Pipe` derives that
/// same frame itself from the path's start point and start tangent — the
/// ergonomic win task #5343 delivers.
///
/// Before #5343 this test could not pass under ANY tolerance: `Pipe` applied a
/// +Z start-tangent guard, and a helix start tangent is near-horizontal
/// (measured (0, 0.999, 0.046)), so `execute` returned `Err` outright. Assertion
/// (a) below — that the pipe builds a non-`Undef` solid at all — is therefore
/// the primary acceptance signal; the volume check merely pins that the
/// resulting frame is the RIGHT one rather than some other valid-but-wrong pose.
///
/// # Tolerance derivation
///
/// `V = π·r²·L` is EXACT here by the tube-volume (Weyl/Hotelling) theorem: the
/// curvature correction integrates to zero over a rotationally-symmetric
/// cross-section. Both of that theorem's side conditions hold for this spine:
///
/// * `r` is below the radius of curvature. With `c = pitch/2π = 1.114mm` and
///   `κ = R/(R² + c²) = 24/577.24 = 0.0416 mm⁻¹`, that radius is
///   `1/κ = 24.05mm` ≫ `r = 3mm`.
/// * the tube does not self-intersect. The helix angle is
///   `atan(pitch/2πR) = 2.658°`, so the perpendicular separation of adjacent
///   turns is `pitch·cos(2.658°) = 6.99mm` > `2r = 6mm` (a ~1mm clearance —
///   tight, but positive).
///
/// So the only error left is OCCT's BSpline approximation of the swept surface.
/// `L` is accordingly measured by OCCT ITSELF via `GeometryQuery::EdgeLength`
/// (whose `BRepGProp::LinearProperties` call carries no `TopAbs_EDGE` gate, so
/// it returns this multi-edge Wire's total length) rather than taken from the
/// analytic closed form — that removes analytic-vs-BSpline arc-length error
/// from the comparison, leaving the same single error class as the 1% planar
/// bound in `kernel_pipe_xy_arc_quarter_torus_volume`. 2% carries margin over
/// that precedent for a non-planar 9-turn spine, and is both tighter and better
/// founded than the 5% Pappus bound the Sweep test above must use.
///
/// Magnitude sanity check against the analytic form: `n = height/pitch = 9`
/// turns, `L = sqrt((2π·24·9)² + 63²) = 1358.6mm`, so
/// `V ≈ π·(3mm)²·1358.6mm = 3.841e-5 m³`.
///
/// Observed: OCCT measures `L = 1.3586 m` (matching the analytic value to the
/// printed precision, which independently confirms `BRepLib::BuildCurves3d`
/// builds the 3D curve over ALL nine turns, not a prefix), giving
/// `expected = 3.8415e-5 m³` against `actual = 3.8408e-5 m³` —
/// **`rel_err = 2e-4`**, ~100× inside the 2% bound. That margin is itself
/// evidence the profile lands on the correct frame: a mis-posed or twisting
/// cross-section would perturb the swept volume far more than 0.02%.
#[test]
fn helix_wire_pipes_to_positive_volume_solid() {
    if !OCCT_AVAILABLE {
        return;
    }

    let mut kernel = OcctKernel::new();

    // (a) Helix spine: R = 24mm, pitch = 7mm, height = 63mm (9 full turns) —
    // identical to the Sweep test's step (d).
    let helix = kernel
        .execute(&GeometryOp::Helix {
            radius: 0.024,
            pitch: 0.007,
            height: 0.063,
        })
        .expect("Helix spine should build");

    // (b) Pipe a 3mm cross-section along it. No CircleProfile/Rotate/Translate
    // preamble: Pipe poses the profile on the path's own start frame.
    let piped = kernel
        .execute(&GeometryOp::Pipe {
            path: helix.id,
            radius: Value::Real(0.003),
        })
        .expect("pipe along a helix should succeed now that the +Z start-tangent guard is gone");

    assert_eq!(
        piped.repr,
        Some(BRepKind::Solid),
        "helix-piped profile should yield a Solid, got {:?}",
        piped.repr
    );

    // (c) L measured on the spine wire by OCCT, not from the analytic form.
    let len = kernel
        .query(&GeometryQuery::EdgeLength(helix.id))
        .expect("EdgeLength query on the helix spine wire should succeed");
    let l = len
        .as_f64()
        .expect("edge length value should be numeric");

    let vol = kernel
        .query(&GeometryQuery::Volume(piped.id))
        .expect("Volume query on the piped solid should succeed");
    let v = vol.as_f64().expect("volume value should be numeric");

    let expected = std::f64::consts::PI * 0.003_f64.powi(2) * l;
    let rel_err = (v - expected).abs() / expected;
    assert!(
        v > 0.0 && rel_err < 0.02,
        "helix-piped volume should be ≈{:.4e} m³ (π·r²·L, OCCT-measured L={:.4} m), \
         got {:.4e} (rel_err={:.4})",
        expected,
        l,
        v,
        rel_err
    );
}
