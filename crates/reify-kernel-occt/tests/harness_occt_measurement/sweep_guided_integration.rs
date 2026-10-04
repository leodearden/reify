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
//!
//! The `sweep_guided_rejects_*` cases cover the other two arguments as well as
//! the profile: `path` and `guide` are wire-typed and get the same
//! Reify-authored, argument-naming rejection, since a selector can hand any
//! extracted sub-shape to either of them.

#![cfg(has_occt)]

use crate::common::{self, Xyz};
use reify_kernel_occt::OcctKernel;
use reify_ir::{BRepKind, GeometryError, GeometryHandleId, GeometryOp, GeometryQuery, Value};

/// The label `wrap_occt_call` prefixes onto every diagnostic raised by the C++
/// entry point behind `GeometryOp::SweepGuided`.
///
/// It is the wrapper symbol `make_pipe_shell`, not the `sweep_guided(...)` a
/// DSL author actually wrote — promoting kernel diagnostics to DSL-level op
/// names is tracked as #5895. The rejection assertions below reference this
/// constant rather than spelling the symbol out, so that change is a one-line
/// edit here instead of a rewrite of every test.
const SWEEP_GUIDED_OP_LABEL: &str = "make_pipe_shell";

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

/// Build the `CircleProfile` face the DSL's `circle(r)` lowers to.
fn make_circle_face(kernel: &mut OcctKernel, radius: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::CircleProfile {
            radius: Value::Real(radius),
        })
        .expect("CircleProfile should build")
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

/// Assert `id` is an open (un-capped) SHELL.
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
            "{what} must sweep to an open (un-capped) shell; got a closed \
             result, which means the face-only solidification gate has been \
             widened to non-face profiles too"
        ),
        other => panic!("expected IsClosed Bool, got {:?}", other),
    }
    assert_eq!(
        kernel.repr_of(id),
        Some(BRepKind::Shell),
        "{what} must sweep to a SHELL — an un-capped result that degraded to \
         some other type would satisfy the IsClosed check vacuously"
    );
}

/// Assert `id` is a closed SOLID — the declared result kind of
/// `sweep_guided(profile: Surface, …) -> Solid`.
#[track_caller]
fn assert_closed_solid(kernel: &OcctKernel, id: GeometryHandleId, what: &str) {
    match kernel
        .query(&GeometryQuery::IsClosed(id))
        .expect("IsClosed query should succeed")
    {
        Value::Bool(closed) => assert!(
            closed,
            "{what} must sweep to a closed (capped) result, got an open shell"
        ),
        other => panic!("expected IsClosed Bool, got {:?}", other),
    }
    assert_eq!(
        kernel.repr_of(id),
        Some(BRepKind::Solid),
        "{what} must be registered as a Solid — the repr the declared \
         signature promises and downstream sub-shape classification trusts"
    );
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
    let _ = bbox_spans(&kernel, result.id);

    // A WIRE profile must stay an un-capped SHELL. This is the false branch of
    // make_pipe_shell's `profile_was_face` solidification gate — the reason
    // that gate is conditional at all. Without this assertion, flipping the
    // C++ to an unconditional `MakeSolid()` would pass every test in this
    // file. A wire section carries no enclosed region for end caps to close
    // over, so there is nothing to solidify.
    assert_open_shell(&kernel, result.id, "a wire profile");
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
    let profile = make_circle_face(&mut kernel, 0.02);
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
    assert_closed_solid(&kernel, result.id, "a face profile");

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

/// A VERTEX profile must clear Reify's section-profile contract.
///
/// `BRepFill_Section` stores a wire *or a vertex*, so `section_profile_to_wire`
/// hands vertices through untouched, and both the header contract and the
/// rejection message advertise Vertex as a supported profile kind — without
/// this test that arm is asserted only by prose.
///
/// A *lone* vertex section is nevertheless refused DOWNSTREAM by OCCT itself:
/// `BRepOffsetAPI_MakePipeShell` needs at least one non-punctual section and
/// answers "OCCT make_pipe_shell: Wrong usage of punctual sections" (measured)
/// when every section is a point. That is an OCCT constraint on the sweep, not
/// a Reify contract violation, and the assertions below pin exactly that
/// distinction: the diagnostic must keep the "OCCT <op>: " framing and must NOT
/// be the "unsupported profile shape type 'Vertex'" rejection the helper would
/// raise if the vertex arm regressed. An `Ok` is accepted too — a future OCCT
/// that lifts the punctual-section restriction is not a regression here.
///
/// The building half of the vertex arm is covered by
/// `loft_guided_accepts_vertex_section`, which pairs a vertex tip with a face
/// section so the section set is not punctual-only.
#[test]
fn sweep_guided_vertex_profile_clears_the_section_contract() {
    let mut kernel = OcctKernel::new();
    let profile = kernel.store_vertex_at_for_test(0.0, 0.0, 0.0);
    let path = make_straight_path(&mut kernel, 0.1);
    let guide = make_offset_guide(&mut kernel, 0.05, 0.03, 0.1);

    match kernel.execute(&GeometryOp::SweepGuided {
        profile,
        path,
        guide,
    }) {
        Ok(_) => {}
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                !msg.contains("unsupported profile shape type"),
                "a vertex profile must clear section_profile_to_wire's type \
                 contract — it is one of the two kinds BRepFill_Section stores; \
                 got the unsupported-type rejection: {msg}"
            );
            assert!(
                msg.starts_with("OCCT "),
                "a vertex profile may only fail INSIDE OCCT (a punctual-section \
                 sweep is degenerate), which wrap_occt_call frames as \
                 \"OCCT <op>: …\"; a Reify-authored \"<op>: …\" diagnostic here \
                 would mean the vertex arm regressed to a rejection: {msg}"
            );
        }
        Err(other) => panic!("expected OperationFailed, got {:?}", other),
    }
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
    let profile = make_circle_face(&mut kernel, 0.02);
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
    let profile_plain = make_circle_face(&mut kernel, 0.02);
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

/// Run a `SweepGuided` with the given arguments and assert it is rejected with
/// a Reify-authored contract diagnostic.
///
/// `expected_fragment` is matched **verbatim and case-sensitively**, so callers
/// pass the full distinguishing phrase (`"shape type 'Edge'"`, not `"Edge"`).
/// A bare type word would match vacuously: the rejection message's own fixed
/// tail — "must be a Wire, a Vertex, or a Face" — already contains "wire",
/// "vertex" and "face", so a lowercased substring test would pass for those
/// three types no matter what the offending shape actually was.
#[track_caller]
fn assert_sweep_rejected(
    kernel: &mut OcctKernel,
    profile: GeometryHandleId,
    path: GeometryHandleId,
    guide: GeometryHandleId,
    expected_fragment: &str,
) {
    match kernel.execute(&GeometryOp::SweepGuided {
        profile,
        path,
        guide,
    }) {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains(expected_fragment),
                "error must contain the distinguishing phrase \
                 {expected_fragment:?}, got: {msg}"
            );
            // One assertion, two properties. `wrap_occt_call` renders a
            // Reify-detected ContractViolation as "<op>: <message>" and
            // reserves "OCCT <op>: unexpected: …" for genuine OCCT-originated
            // failures, so a leading "<label>: " proves BOTH that the failure
            // is attributed to this entry point (not to loft_guided_profiles,
            // which shares the helper and MakePipeShell underneath) AND that it
            // arrived through the ContractViolation catch arm rather than the
            // broader std::exception one.
            let prefix = format!("{SWEEP_GUIDED_OP_LABEL}: ");
            assert!(
                msg.starts_with(&prefix),
                "a Reify contract violation must surface as {prefix:?} + message \
                 — no \"OCCT \" prefix, no \"unexpected\" framing; got: {msg}"
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

/// [`assert_sweep_rejected`] with a valid straight path and offset guide, for
/// the cases where the *profile* is the argument under test.
#[track_caller]
fn assert_profile_rejected(
    kernel: &mut OcctKernel,
    profile: GeometryHandleId,
    expected_fragment: &str,
) {
    let path = make_straight_path(kernel, 0.1);
    let guide = make_offset_guide(kernel, 0.05, 0.03, 0.1);
    assert_sweep_rejected(kernel, profile, path, guide, expected_fragment);
}

/// A SOLID profile — the classic wrong-dimensionality argument — must be
/// rejected with a Reify-authored diagnostic naming the offending shape type.
#[test]
fn sweep_guided_rejects_solid_profile() {
    let mut kernel = OcctKernel::new();
    let solid = kernel
        .execute(&GeometryOp::Box {
            width: Value::Real(0.01),
            height: Value::Real(0.01),
            depth: Value::Real(0.01),
        })
        .expect("Box should build")
        .id;
    assert_profile_rejected(&mut kernel, solid, "shape type 'Solid'");
}

/// An EDGE profile must be rejected too.
///
/// Split from the SOLID case above so neither arm can mask the other: this fix
/// narrowed a previously documented contract (the old comment claimed "profile
/// may be an edge, wire, or face"), so EDGE is exactly the branch whose status
/// changed and it must report independently.
///
/// The edge is obtained by topology extraction from a circle profile face — the
/// same route by which a selector can hand an edge handle to an op.
#[test]
fn sweep_guided_rejects_edge_profile() {
    let mut kernel = OcctKernel::new();
    let face = make_circle_face(&mut kernel, 0.02);
    let edges = kernel
        .extract_edges(face)
        .expect("extract_edges on a circle face should succeed");
    let edge = *edges
        .first()
        .expect("a circle profile face has at least one edge");
    assert_profile_rejected(&mut kernel, edge, "shape type 'Edge'");
}

/// A non-wire PATH must be rejected by argument name.
///
/// The spine reaches `TopoDS::Wire(...)` the moment the maker is constructed,
/// and that downcast raises OCCT's `Standard_TypeMismatch`, which names no
/// argument (and frequently carries no message at all). A face handle is
/// reachable here by exactly the route that makes a bad profile reachable —
/// `CircleProfile` and `extract_faces` both mint `Surface`-typed handles — so
/// the path argument gets the same contract treatment as the profile.
#[test]
fn sweep_guided_rejects_non_wire_path() {
    let mut kernel = OcctKernel::new();
    let profile = make_circle_face(&mut kernel, 0.02);
    let path = make_circle_face(&mut kernel, 0.02); // a FACE, not a Curve
    let guide = make_offset_guide(&mut kernel, 0.05, 0.03, 0.1);

    assert_sweep_rejected(
        &mut kernel,
        profile,
        path,
        guide,
        "sweep path has unsupported shape type 'Face'",
    );
}

/// A non-wire GUIDE must be rejected by argument name, for the same reason as
/// the path. Uses an extracted EDGE — the shape a `Curve`-typed selector result
/// most plausibly is when it isn't a wire.
#[test]
fn sweep_guided_rejects_non_wire_guide() {
    let mut kernel = OcctKernel::new();
    let profile = make_circle_face(&mut kernel, 0.02);
    let path = make_straight_path(&mut kernel, 0.1);
    let face = make_circle_face(&mut kernel, 0.02);
    let edges = kernel
        .extract_edges(face)
        .expect("extract_edges on a circle face should succeed");
    let guide = *edges
        .first()
        .expect("a circle profile face has at least one edge");

    assert_sweep_rejected(
        &mut kernel,
        profile,
        path,
        guide,
        "sweep guide has unsupported shape type 'Edge'",
    );
}

/// An UNBOUNDED face (zero wires) must be rejected by name.
///
/// This is a CRASH guard, not a message-quality guard. `BRepTools::OuterWire`
/// returns a NULL wire for a face with no bounding wire; measured behaviour
/// with the `wire_count == 0` branch removed from `section_profile_to_wire` is
/// that this test dies with **SIGSEGV** — the null shape is dereferenced inside
/// `BRepFill_Section` rather than raising a catchable `Standard_NullObject`, so
/// `wrap_occt_call` never gets the chance to turn it into a Rust `Err`.
///
/// The input is reachable from the DSL: `make_half_space` builds its boundary
/// from a bare `gp_Pln` via `BRepBuilderAPI_MakeFace`, and `extract_faces` on
/// the resulting solid mints that wire-less face as a `Surface`-typed handle a
/// selector can hand straight to `sweep_guided`. A `wire_count > 1`-only guard
/// lets it through — this test is what pins the guard at `!= 1`.
#[test]
fn sweep_guided_rejects_unbounded_face_profile() {
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

    assert_profile_rejected(&mut kernel, unbounded, "no bounding wire");
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

    // The inner-wire COUNT is the semantic content worth pinning; the prose
    // around it ("… would be silently discarded") is deliberately not asserted,
    // so rewording the diagnostic is not a test-breaking change.
    assert_profile_rejected(&mut kernel, annulus, "1 inner wire(s)");
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
    let Xyz {
        x: plain_x,
        y: plain_y,
        z: plain_z,
    } = common::xyz_of(kernel.query(&GeometryQuery::Centroid(plain.id)), "Centroid");

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
    let Xyz {
        x: g_x,
        y: g_y,
        z: g_z,
    } = common::xyz_of(kernel.query(&GeometryQuery::Centroid(guided.id)), "Centroid");

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
