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
//! - Exception: `loft_guided_accepts_circle_face_profiles` feeds
//!   `CircleProfile` **faces** — the only section kind a DSL `loft_guided()`
//!   call can produce — and pins the face → outer-wire → solid path.
//!   `loft_guided_mixed_face_and_wire_sections_yield_shell` pins the third
//!   branch (reduce the face, but do not solidify),
//!   `loft_guided_accepts_vertex_section` pins the vertex passthrough, and the
//!   `loft_guided_rejects_*` cases pin this entry point's own rejection
//!   diagnostics — `sweep_guided_integration.rs` drives only
//!   `GeometryOp::SweepGuided`, so it cannot cover them. That applies with most
//!   force to `loft_guided_rejects_unbounded_face_section`: its guard is the
//!   one whose regression is a SIGSEGV that would take the whole harness
//!   binary down, not a bad message.

#![cfg(has_occt)]

use crate::common;
use reify_kernel_occt::OcctKernel;
use reify_ir::{BRepKind, GeometryError, GeometryHandleId, GeometryOp, GeometryQuery, Value};

/// The label `wrap_occt_call` prefixes onto every diagnostic raised by the C++
/// entry point behind `GeometryOp::LoftGuided`, and its `make_pipe_shell`
/// counterpart — both ops share `section_profile_to_wire` and
/// `BRepOffsetAPI_MakePipeShell` underneath, so the attribution assertions
/// below check the loft label is present AND the sweep label is not.
///
/// Both are C++ wrapper symbols rather than the `loft_guided(...)` a DSL author
/// wrote; promoting kernel diagnostics to DSL-level op names is tracked as
/// #5895, which these constants reduce to a one-line edit per file.
const LOFT_GUIDED_OP_LABEL: &str = "loft_guided_profiles";
const SWEEP_GUIDED_OP_LABEL: &str = "make_pipe_shell";

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

/// Query the bounding box of `id`, assert every component is finite, and
/// return the (x_span, y_span, z_span) extents.
///
/// Thin wrapper over the shared [`common::bbox_of`] parser (task 5893), which
/// replaced the file-local parsers this task's earlier revision had grown.
/// Kept as a named helper only to carry the loft-specific finiteness message
/// that the assertions below rely on.
fn bbox_spans(kernel: &OcctKernel, id: GeometryHandleId) -> (f64, f64, f64) {
    let bbox = common::bbox_of(kernel.query(&GeometryQuery::BoundingBox(id)));
    assert!(
        bbox.all_finite(),
        "loft_guided bbox components must all be finite, got {bbox:?}"
    );
    bbox.spans()
}

/// Build the `CircleProfile` face the DSL's `circle(r)` lowers to.
fn make_circle_face(kernel: &mut OcctKernel, radius: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::CircleProfile {
            radius: Value::Real(radius),
        })
        .expect("CircleProfile should build")
        .id
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
    // The lofted shape should span at least from ~0 to ~0.1 in Z
    // (profile-1 at z=0, profile-2 at z=0.1). Allow a small numerical
    // margin for OCCT's bbox padding.
    let (_, _, z_span) = bbox_spans(&kernel, result.id);
    assert!(
        z_span > 0.09,
        "loft_guided bbox z-span should cover both profiles (≥0.09), got {z_span}"
    );

    // All sections are WIRES, so the result must stay an un-capped SHELL. This
    // is the false branch of loft_guided_profiles' `all_sections_were_faces`
    // solidification gate — the reason that gate is conditional at all.
    // Without this assertion, flipping the C++ to an unconditional
    // `MakeSolid()` would pass every test in this file.
    assert_open_shell(&kernel, result.id, "an all-wire section set");
}

/// Assert `id` is an open (un-capped) SHELL rather than a closed solid.
///
/// Both halves are load-bearing. `IsClosed` alone is too weak: `is_closed` in
/// cpp/occt_wrapper.cpp short-circuits to `false` for anything that is not a
/// SOLID, COMPSOLID or SHELL, so a lone `!closed` also passes for a COMPOUND or
/// a stray FACE — precisely the degradation this assertion exists to catch. The
/// kind half reads `repr_of`, which since this task stamps the result's real
/// top-level type (not the default `BRepKind::Solid`) reports what the shape
/// actually is.
#[track_caller]
fn assert_open_shell(kernel: &OcctKernel, id: GeometryHandleId, what: &str) {
    match kernel
        .query(&GeometryQuery::IsClosed(id))
        .expect("IsClosed query should succeed")
    {
        Value::Bool(closed) => assert!(
            !closed,
            "{what} must loft to an open (un-capped) shell; got a closed \
             result, which means the all-faces solidification gate has been \
             widened beyond all-face section sets"
        ),
        other => panic!("expected IsClosed Bool, got {:?}", other),
    }
    assert_eq!(
        kernel.repr_of(id),
        Some(BRepKind::Shell),
        "{what} must loft to a SHELL — an un-capped result that degraded to \
         some other type would satisfy the IsClosed check vacuously"
    );
}

/// Run a `LoftGuided` that must be rejected and assert the diagnostic is
/// Reify-authored and attributed to this entry point — NOT to `make_pipe_shell`,
/// even though both share `section_profile_to_wire` and
/// `BRepOffsetAPI_MakePipeShell` underneath.
///
/// `expected_fragment` is matched verbatim and case-sensitively; see the note
/// on `sweep_guided_integration::assert_sweep_rejected` for why a bare type word
/// would match vacuously.
///
/// `guides` lets a caller drive the guide-argument contract as well as the
/// section one; pass a plain spine for section cases.
#[track_caller]
fn assert_loft_rejected(
    kernel: &mut OcctKernel,
    sections: Vec<GeometryHandleId>,
    guides: Vec<GeometryHandleId>,
    expected_fragment: &str,
) {
    match kernel.execute(&GeometryOp::LoftGuided {
        profiles: sections,
        guides,
    }) {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains(expected_fragment),
                "error must contain the distinguishing phrase \
                 {expected_fragment:?}, got: {msg}"
            );
            // One assertion, two properties — see the twin in
            // sweep_guided_integration.rs: `wrap_occt_call` renders a
            // Reify-detected ContractViolation as "<op>: <message>" and reserves
            // "OCCT <op>: unexpected: …" for genuine OCCT-originated failures, so
            // a leading "<label>: " proves both attribution and framing.
            let prefix = format!("{LOFT_GUIDED_OP_LABEL}: ");
            assert!(
                msg.starts_with(&prefix),
                "a Reify contract violation must surface as {prefix:?} + message \
                 — no \"OCCT \" prefix, no \"unexpected\" framing; got: {msg}"
            );
            assert!(
                !msg.contains(SWEEP_GUIDED_OP_LABEL),
                "error must not misattribute a loft failure to the sweep entry \
                 point, got: {msg}"
            );
            assert!(
                !msg.contains("BRepFill_Section"),
                "error must be a Reify diagnostic, not OCCT's opaque internal \
                 wording, got: {msg}"
            );
        }
        Ok(_) => panic!("expected OperationFailed for {expected_fragment:?}, got Ok"),
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
}

/// [`assert_loft_rejected`] with a valid straight spine, for the cases where a
/// *section* is the argument under test.
#[track_caller]
fn assert_section_rejected(
    kernel: &mut OcctKernel,
    sections: Vec<GeometryHandleId>,
    expected_fragment: &str,
) {
    let spine = make_spine(kernel, 0.1);
    assert_loft_rejected(kernel, sections, vec![spine], expected_fragment);
}

/// `loft_guided_profiles` used to add each section with a bare
/// `maker.Add(profile)`, exactly as `make_pipe_shell` did, so it carried the
/// same defect: `BRepFill_Section` stores only a wire or a vertex and throws
/// "bad shape type of section" on a face. The tests above only ever feed Arc
/// **wires** (see the module note), so the face path — the one the DSL's
/// `circle(r)` produces — was uncovered here too.
///
/// Sections are now normalized to their outer wire, and an all-face section
/// set is solidified, matching `make_pipe_shell` and the declared signature
/// `loft_guided(profiles: List<Surface>, guides: List<Curve>) -> Solid`.
#[test]
fn loft_guided_accepts_circle_face_profiles() {
    let mut kernel = OcctKernel::new();
    let p1 = make_circle_face(&mut kernel, 0.02);
    // CircleProfile always builds at z=0; lift the second section to z=0.1 so
    // the loft has non-zero Z extent.
    let p2_flat = make_circle_face(&mut kernel, 0.03);
    let p2 = kernel
        .execute(&GeometryOp::Translate {
            target: p2_flat,
            dx: 0.0,
            dy: 0.0,
            dz: 0.1,
        })
        .expect("Translate should succeed")
        .id;
    let spine = make_spine(&mut kernel, 0.1);

    let result = match kernel.execute(&GeometryOp::LoftGuided {
        profiles: vec![p1, p2],
        guides: vec![spine],
    }) {
        Ok(r) => r,
        Err(e) => panic!("LoftGuided with CircleProfile (face) sections must succeed, got: {e}"),
    };

    // Extents: the loft must span both profile planes in Z and reach the
    // larger (30 mm) section's diameter in X/Y. Without the X/Y bound a
    // collapsed or radially degenerate surface between the two outer wires
    // would still satisfy the Z-span check.
    let (x_span, y_span, z_span) = bbox_spans(&kernel, result.id);
    assert!(
        z_span > 0.09,
        "loft bbox z-span should cover both profile planes (≥0.09), got {z_span}"
    );
    for (axis, span) in [("x", x_span), ("y", y_span)] {
        assert!(
            span > 0.059,
            "loft {axis}-span should reach the larger section's diameter \
             (2 × 0.03 = 0.06), got {span}"
        );
    }

    // All sections were faces, so the result must be a closed SOLID — the
    // declared signature is `loft_guided(..) -> Solid`. An un-capped shell has
    // free edges at both ends and reports false here; the repr assertion pins
    // that the kernel registers what it actually built, so a shell-valued
    // result could not pass by being mislabelled Solid.
    match kernel
        .query(&GeometryQuery::IsClosed(result.id))
        .expect("IsClosed query should succeed")
    {
        Value::Bool(closed) => assert!(
            closed,
            "an all-face section set must loft to a closed solid, got an open shell"
        ),
        other => panic!("expected IsClosed Bool, got {:?}", other),
    }
    assert_eq!(
        kernel.repr_of(result.id),
        Some(BRepKind::Solid),
        "an all-face section set must be registered as a Solid"
    );

    // ...and enclose a positive volume in the neighbourhood of the conical
    // frustum through the two sections: V = (π·h/3)(r1² + r1·r2 + r2²).
    // MakePipeShell interpolates the sections along the spine rather than
    // ruling them exactly, so this is a sanity band (±25%), not an exact pin
    // — it exists to catch an inside-out, collapsed, or shell-valued result.
    let volume = match kernel
        .query(&GeometryQuery::Volume(result.id))
        .expect("Volume query should succeed")
    {
        Value::Real(v) => v,
        other => panic!("expected Volume Real, got {:?}", other),
    };
    let frustum =
        std::f64::consts::PI * 0.1 / 3.0 * (0.02 * 0.02 + 0.02 * 0.03 + 0.03 * 0.03);
    assert!(
        volume > 0.0 && (volume - frustum).abs() / frustum < 0.25,
        "lofted solid volume {volume} should be near the frustum volume {frustum}"
    );
}

/// A VERTEX section must be accepted — the loft-to-a-point (cone tip) case.
///
/// `BRepFill_Section` stores a wire *or a vertex*, so `section_profile_to_wire`
/// passes vertices straight through; without this test that arm is asserted
/// only by the header prose and by the rejection message's own "must be a Wire,
/// a Vertex, or a Face". A vertex carries no enclosed region, so the section set
/// is not all-faces and the result stays an un-capped shell.
#[test]
fn loft_guided_accepts_vertex_section() {
    let mut kernel = OcctKernel::new();
    let base = make_circle_face(&mut kernel, 0.02);
    let tip = kernel.store_vertex_at_for_test(0.0, 0.0, 0.1);
    let spine = make_spine(&mut kernel, 0.1);

    let result = match kernel.execute(&GeometryOp::LoftGuided {
        profiles: vec![base, tip],
        guides: vec![spine],
    }) {
        Ok(r) => r,
        Err(e) => panic!("LoftGuided with a vertex section must succeed, got: {e}"),
    };

    // The vertex really did terminate the loft at z=0.1 rather than being
    // dropped: the shape spans both section planes.
    let (_, _, z_span) = bbox_spans(&kernel, result.id);
    assert!(
        z_span > 0.09,
        "vertex-terminated loft z-span should reach the tip plane (≥0.09), \
         got {z_span}"
    );

    assert_open_shell(&kernel, result.id, "a face/vertex section set");
}

/// A MIXED section set — one face, one wire — must still be accepted (the face
/// is reduced to its outer wire like any other), but must NOT be solidified.
///
/// This is the third branch of `all_sections_were_faces`, distinct from both
/// the all-wire and all-face cases: the flag goes false while the face is still
/// reduced, so `Add` succeeds and the result stays a shell. A mixed set has no
/// coherent solid interpretation — a wire section carries no enclosed region
/// for an end cap to close over — so leaving it open is the documented
/// behaviour, and this test is what pins it.
#[test]
fn loft_guided_mixed_face_and_wire_sections_yield_shell() {
    let mut kernel = OcctKernel::new();
    // Section 1: a CircleProfile FACE at z=0.
    let face = make_circle_face(&mut kernel, 0.02);
    // Section 2: an Arc WIRE at z=0.1.
    let wire = make_circle_profile(&mut kernel, 0.01, 0.1);
    let spine = make_spine(&mut kernel, 0.1);

    let result = match kernel.execute(&GeometryOp::LoftGuided {
        profiles: vec![face, wire],
        guides: vec![spine],
    }) {
        Ok(r) => r,
        Err(e) => panic!(
            "a mixed face/wire section set must still loft (the face is reduced \
             to its outer wire), got: {e}"
        ),
    };

    // The face was reduced, so the loft really did span both sections.
    let (_, _, z_span) = bbox_spans(&kernel, result.id);
    assert!(
        z_span > 0.09,
        "mixed-section loft z-span should cover both profile planes (≥0.09), \
         got {z_span}"
    );

    assert_open_shell(&kernel, result.id, "a mixed face/wire section set");
}

/// An unsupported SECTION type must be rejected through the loft entry point
/// with its own attribution. The rejection assertions in
/// sweep_guided_integration.rs all drive `GeometryOp::SweepGuided`, so without
/// this the loft path's diagnostics are unverified even though both ops share
/// `section_profile_to_wire`.
#[test]
fn loft_guided_rejects_unsupported_section_type() {
    let mut kernel = OcctKernel::new();
    let solid = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(0.01),
            height: Value::Real(0.01),
            depth: Value::Real(0.01),
        })
        .expect("Box should build")
        .id;
    let ok_section = make_circle_profile(&mut kernel, 0.02, 0.1);

    assert_section_rejected(
        &mut kernel,
        vec![ok_section, solid],
        "shape type 'Solid'",
    );
}

/// A holed face SECTION must be rejected through the loft entry point too —
/// silently dropping the hole is exactly as wrong here as in `sweep_guided`,
/// and (since an all-face section set is solidified) yields the same
/// plausible-looking filled solid.
#[test]
fn loft_guided_rejects_holed_face_section() {
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
    let ok_section = make_circle_face(&mut kernel, 0.02);

    // The inner-wire COUNT is the semantic content worth pinning; the prose
    // around it ("… would be silently discarded") is deliberately not asserted,
    // so rewording the diagnostic is not a test-breaking change.
    assert_section_rejected(&mut kernel, vec![ok_section, annulus], "1 inner wire(s)");
}

/// An UNBOUNDED face (zero wires) section must be rejected through the loft
/// entry point too.
///
/// This is the branch the C++ documents as a SIGSEGV crash guard rather than a
/// politeness guard: `BRepTools::OuterWire` returns a NULL wire for a face with
/// no bounding wire, and letting that reach `Add` dereferences it inside
/// `BRepFill_Section` — a hard process kill, not a catchable `Standard_Failure`.
/// A regression here would take down the whole harness binary, every other test
/// with it, so the loft path needs its own coverage rather than inheriting
/// `sweep_guided_rejects_unbounded_face_profile`'s: that test drives only
/// `GeometryOp::SweepGuided`. The input is equally reachable from either op —
/// `make_half_space` builds its boundary from a bare `gp_Pln`, and
/// `extract_faces` mints that wire-less face as a `Surface`-typed handle.
#[test]
fn loft_guided_rejects_unbounded_face_section() {
    let mut kernel = OcctKernel::new();
    let half_space = kernel
        .execute(&GeometryOp::HalfSpace {
            px: Value::Real(0.0),
            py: Value::Real(0.0),
            pz: Value::Real(0.0),
            nx: Value::Real(0.0),
            ny: Value::Real(0.0),
            nz: Value::Real(1.0),
        })
        .expect("HalfSpace should build")
        .id;
    let faces = kernel
        .extract_faces(half_space)
        .expect("extract_faces on a half-space should succeed");
    let unbounded = *faces
        .first()
        .expect("a half-space has one (unbounded planar) face");
    let ok_section = make_circle_face(&mut kernel, 0.02);

    assert_section_rejected(
        &mut kernel,
        vec![ok_section, unbounded],
        "no bounding wire",
    );
}

/// A non-wire SPINE (the first guide) must be rejected by role name.
///
/// The spine reaches `TopoDS::Wire(...)` when the maker is constructed, and
/// that downcast raises OCCT's `Standard_TypeMismatch`, which names no argument.
/// A `Surface`-typed handle is reachable here by the same selector route that
/// makes a bad section reachable, so the guide list gets the same contract
/// treatment as the sections.
#[test]
fn loft_guided_rejects_non_wire_spine() {
    let mut kernel = OcctKernel::new();
    let p1 = make_circle_face(&mut kernel, 0.02);
    let p2 = make_circle_face(&mut kernel, 0.03);
    let bad_spine = make_circle_face(&mut kernel, 0.02); // a FACE, not a Curve

    assert_loft_rejected(
        &mut kernel,
        vec![p1, p2],
        vec![bad_spine],
        "loft spine (first guide) has unsupported shape type 'Face'",
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
