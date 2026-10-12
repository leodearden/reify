//! The sweep pose guard and the derived `BRepKind` stamp on both plain-sweep
//! entry points, and the guard's scope fences (task #7248).
//!
//! `GeometryOp::Sweep` is NOT the entry point the eval layer reaches:
//! `OcctKernelHandle`'s worker routes it to `OcctKernel::sweep_with_history`,
//! a separate `make_pipe_*` call site. Both must consult the one guard and the
//! one stamp derivation, or they drift, which is how they came to disagree
//! about the `BRepKind` stamp (#5713).
//!
//! Pinned here:
//! - the degenerate-pose refusal on BOTH plain-sweep entry points, and every
//!   fact it carries;
//! - the guard's scope fences: it fails open for a wire profile and stays
//!   silent on a curved face, but still judges a circle;
//! - a correctly-posed sweep keeps its volume and its cap faces;
//! - a wire-profile sweep is stamped Shell, the local-feature gate refuses to
//!   fillet it, and a correctly-posed face sweep is still a filletable Solid.

#![cfg(has_occt)]

use reify_ir::{BRepKind, GeometryError, GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::{OCCT_AVAILABLE, OcctKernel};

/// 20×20mm rect profile, in SI metres, with its own normal along +Z.
fn pose_test_rect_profile(kernel: &mut OcctKernel) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::RectangleProfile {
            width: Value::Real(0.020),
            height: Value::Real(0.020),
        })
        .expect("RectangleProfile should build")
        .id
}

/// A straight 200mm spine from the origin towards `(x, 0, z)`, in metres.
fn pose_test_spine(kernel: &mut OcctKernel, x: f64, z: f64) -> GeometryHandleId {
    kernel
        .execute(&GeometryOp::LineSegment {
            x1: 0.0,
            y1: 0.0,
            z1: 0.0,
            x2: x,
            y2: 0.0,
            z2: z,
        })
        .expect("LineSegment (spine) should build")
        .id
}

/// The history-tracked entry point must refuse the degenerate pose too.
///
/// A +Z-normal rect swept along a +X spine is exactly in-plane. Unguarded,
/// this entry point returned a zero-volume body and full history records
/// for it, so a history-tracked sweep bypassed the guard entirely.
#[test]
fn sweep_with_history_refuses_a_profile_posed_in_the_spine_plane() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let profile_id = pose_test_rect_profile(&mut kernel);
    let spine_id = pose_test_spine(&mut kernel, 0.200, 0.0);

    match kernel.sweep_with_history(profile_id, spine_id) {
        Err(GeometryError::OperationFailed(msg)) => assert!(
            msg.contains("sweep") && msg.contains("tangent"),
            "expected a degenerate-pose error naming the sweep and the \
             start tangent, got: {msg}"
        ),
        Ok(_) => panic!(
            "sweep_with_history accepted a profile posed in the spine's \
             plane; the guard is wired into only one of the two plain-sweep \
             entry points"
        ),
        Err(other) => panic!("expected OperationFailed, got {other:?}"),
    }
}

/// The plain `execute` entry point must refuse the degenerate pose as well.
///
/// The eval layer never reaches this arm, but the handle's plain `Execute`
/// request does, so a guard wired into `sweep_with_history` alone would leave
/// it open.
#[test]
fn execute_refuses_a_profile_posed_in_the_spine_plane() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let profile = pose_test_rect_profile(&mut kernel);
    let path = pose_test_spine(&mut kernel, 0.200, 0.0);

    match kernel.execute(&GeometryOp::Sweep { profile, path }) {
        Err(GeometryError::OperationFailed(msg)) => assert!(
            msg.contains("posed in the path's plane"),
            "expected the degenerate-pose refusal, got: {msg}"
        ),
        Ok(_) => panic!(
            "execute(GeometryOp::Sweep) accepted a profile posed in the \
             spine's plane; the execute arm is the unguarded plain-sweep \
             entry point"
        ),
        Err(other) => panic!("expected OperationFailed, got {other:?}"),
    }
}

/// FAIL-OPEN fence. A WIRE profile has no surface normal, so the pose
/// predicate is undefined and the guard must skip rather than reject.
///
/// Green on arrival, and it must STAY green: measured, this sweep realizes
/// a 20mm × 200mm face of area 0.004 m², stamped an un-capped Shell (pinned
/// by `sweep_with_history_of_a_wire_profile_is_stamped_shell` below). A pose
/// error has nothing to claim about it: the shape has no normal to be
/// measured against.
#[test]
fn sweep_with_history_fails_open_for_a_wire_profile() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let profile_id = pose_test_spine(&mut kernel, 0.020, 0.0);
    let spine_id = pose_test_spine(&mut kernel, 0.0, 0.200);

    let (result, _history) = kernel
        .sweep_with_history(profile_id, spine_id)
        .expect("a Wire profile has no normal, so the pose guard must skip it");
    let area = kernel
        .query(&GeometryQuery::SurfaceArea(result.id))
        .expect("area query should succeed")
        .as_f64()
        .expect("area should be numeric");
    let expected = 0.020 * 0.200;
    assert!(
        (area - expected).abs() / expected < 1e-6,
        "wire-profile sweep should still realize a {expected} m² face, got {area}"
    );
}

/// The planarity half of the entry test must not exempt the real victims.
///
/// Every subject the band was measured against sweeps a CIRCLE, not a
/// rectangle (`helix_sweep_e2e.rs`, `helix_sweep_dep_5342.ri`,
/// `pnrg_envelope_spline.ri`). A circle profile is a planar face like any
/// other, but only a test says so: were OCCT to classify its surface as
/// anything but `Plane`, the guard would fail open on exactly the shape it
/// was built for, and every other test here would stay green.
#[test]
fn an_in_plane_circle_profile_is_still_refused() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let profile_id = kernel
        .execute(&GeometryOp::CircleProfile {
            radius: Value::Real(0.003),
        })
        .expect("CircleProfile should build")
        .id;
    let spine_id = pose_test_spine(&mut kernel, 0.200, 0.0);

    match kernel.sweep_with_history(profile_id, spine_id) {
        Err(GeometryError::OperationFailed(msg)) => assert!(
            msg.contains("posed in the path's plane"),
            "expected the degenerate-pose refusal for a circle profile, got: {msg}"
        ),
        Ok(_) => panic!(
            "an in-plane CIRCLE profile was admitted; the planarity half of \
             the entry test is exempting the very shape the band was \
             measured on"
        ),
        Err(other) => panic!("expected OperationFailed, got {other:?}"),
    }
}

/// FAIL-OPEN fence for the planarity half. A CURVED face has no single
/// normal — `query_face_normal` samples the area centroid only — so
/// `|t̂·n̂|` would not be the "degrees off-plane" the refusal claims to
/// report, and the guard must not speak.
///
/// Reachable because the compiler's Closed-and-Planar precondition only
/// inspects statically-known Surface producers: a `let`-bound opaque
/// profile such as `faces(cyl)[0]` arrives here unexamined.
///
/// Asserts only that the POSE guard stays silent, not that the sweep
/// succeeds: what OCCT makes of a cylindrical profile is its own business
/// and not this guard's contract.
#[test]
fn a_non_planar_face_profile_is_not_judged_on_pose() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let cylinder = kernel
        .execute(&GeometryOp::Cylinder {
            radius: Value::Real(0.010),
            height: Value::Real(0.020),
        })
        .expect("Cylinder should build")
        .id;
    let faces = kernel
        .extract_faces(cylinder)
        .expect("a cylinder enumerates faces");
    let lateral = faces
        .iter()
        .copied()
        .find(|id| {
            kernel
                .query(&GeometryQuery::FaceSurfaceKind(*id))
                .ok()
                .and_then(|v| match v {
                    Value::String(kind) => Some(kind == "Cylinder"),
                    _ => None,
                })
                .unwrap_or(false)
        })
        .expect("a cylinder has a Cylinder-surfaced lateral face");
    let spine_id = pose_test_spine(&mut kernel, 0.200, 0.0);

    if let Err(GeometryError::OperationFailed(msg)) = kernel.sweep_with_history(lateral, spine_id) {
        assert!(
            !msg.contains("posed in the path's plane"),
            "the pose guard judged a CURVED face, whose centroid normal \
             describes no plane: {msg}"
        );
    }
}

/// The guard must not disturb a correctly-posed sweep, nor its persistent-
/// naming payload: same volume, and one cap face at each end as before.
#[test]
fn sweep_with_history_preserves_caps_for_a_correctly_posed_profile() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let profile_id = pose_test_rect_profile(&mut kernel);
    let spine_id = pose_test_spine(&mut kernel, 0.0, 0.200);

    let (result, history) = kernel
        .sweep_with_history(profile_id, spine_id)
        .expect("a +Z spine is normal to the profile and must still sweep");

    let vol = kernel
        .query(&GeometryQuery::Volume(result.id))
        .expect("volume query should succeed")
        .as_f64()
        .expect("volume should be numeric");
    let expected = 0.020 * 0.020 * 0.200;
    assert!(
        (vol - expected).abs() / expected < 1e-9,
        "correctly-posed sweep should still be {expected:.6e} m³, got {vol:.6e}"
    );
    assert_eq!(
        history.start_cap_face_indices.len(),
        1,
        "expected 1 start cap face, got {:?}",
        history.start_cap_face_indices
    );
    assert_eq!(
        history.end_cap_face_indices.len(),
        1,
        "expected 1 end cap face, got {:?}",
        history.end_cap_face_indices
    );
}

/// The four facts an author needs, plus the reason and a paste-able fix. A
/// degenerate-pose error that names only the failure leaves them nowhere to
/// go. The example is spelled inline because inline is legal: since #6188 the
/// compiler's profile precondition sees through a transform, and the inline
/// `sweep(translate(rotate(circle(3mm), 1,0,0, -90deg), 24mm,0mm,0mm),
/// helix(24mm, 7mm, 63mm))` realizes at 3.836681796674759e-5 m³, identical to
/// the `let`-bound spelling.
///
/// Asserted as a word SET, never as an exact string: the facts are pinned
/// while the wording stays editable.
#[test]
fn degenerate_pose_error_carries_every_fact_the_author_needs() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let profile_id = pose_test_rect_profile(&mut kernel);
    let spine_id = pose_test_spine(&mut kernel, 0.200, 0.0);

    let Err(GeometryError::OperationFailed(msg)) = kernel.sweep_with_history(profile_id, spine_id)
    else {
        panic!("an in-plane pose must fail with OperationFailed");
    };

    for (fact, needle) in [
        ("the band", "4 degrees"),
        ("this call's measured dot", "0.000000000"),
        ("this call's measured angle", "0.000 degrees off-plane"),
        ("the reason", "encloses nothing"),
        ("the corrective action", "pose the profile normal"),
        ("what to pose it against", "start tangent"),
        ("the measured tangent", "start tangent (1.000000,"),
        ("a paste-able inline example", "sweep(rotate("),
    ] {
        assert!(
            msg.contains(needle),
            "the degenerate-pose error must state {fact} (expected to find \
             {needle:?}); got: {msg}"
        );
    }
}

// --- the honest BRepKind stamp on both plain-sweep entry points (#5713) ---
//
// A plain sweep does not always produce a Solid. Sweeping a WIRE profile
// yields an un-capped SHELL — measured via `shape_type_name`. Its single
// 20mm x 200mm face carries area 0.004 m² and volume 0, which is what an
// area-only measurement mistook for a bare FACE. Yet both entry points
// stamped `BRepKind::Solid` unconditionally, one by falling through to
// `store`'s default and one by saying so outright. The stamp is not
// decorative: the local-feature gate admits ONLY Solid, and its own comment
// warns that a mislabelled input "would either crash inside OCCT or
// silently produce a misclassified result".

/// A wire-profile sweep through `execute` must be stamped for what it is.
#[test]
fn sweep_of_a_wire_profile_is_stamped_shell() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let profile_id = pose_test_spine(&mut kernel, 0.020, 0.0);
    let path_id = pose_test_spine(&mut kernel, 0.0, 0.200);

    let result = kernel
        .execute(&GeometryOp::Sweep {
            profile: profile_id,
            path: path_id,
        })
        .expect("a wire-profile sweep realizes; the pose guard fails open on it");

    assert_eq!(
        result.repr,
        Some(BRepKind::Shell),
        "a wire-profile sweep produces an un-capped SHELL, and the handle \
         must say so"
    );
    assert_eq!(
        kernel.repr_of(result.id),
        Some(BRepKind::Shell),
        "the persisted repr must agree with the returned handle"
    );
}

/// The history-tracked entry point must stamp it honestly too. It is a
/// separate site, and separate sites are exactly how the two came to
/// disagree in the first place.
#[test]
fn sweep_with_history_of_a_wire_profile_is_stamped_shell() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let profile_id = pose_test_spine(&mut kernel, 0.020, 0.0);
    let path_id = pose_test_spine(&mut kernel, 0.0, 0.200);

    let (result, _history) = kernel
        .sweep_with_history(profile_id, path_id)
        .expect("a wire-profile sweep realizes; the pose guard fails open on it");

    assert_eq!(
        result.repr,
        Some(BRepKind::Shell),
        "a wire-profile sweep produces an un-capped SHELL, and the handle \
         must say so"
    );
    assert_eq!(
        kernel.repr_of(result.id),
        Some(BRepKind::Shell),
        "the persisted repr must agree with the returned handle"
    );
}

/// The point of an honest stamp: the local-feature gate can act on it.
///
/// A fillet over the swept shell walked straight through that gate on the
/// strength of the false Solid stamp. With the stamp corrected the gate
/// refuses it with its existing message, which is the whole reason the gate
/// reads the repr rather than trusting the caller.
///
/// Spelled with a CURATED edge selection — the designer-facing
/// `fillet(body, edges_at_height(..), r)` idiom the gate's own REACH note
/// names. The 2-arg back-compat spelling (`edges: vec![]`) is deliberately
/// NOT used here: that arm calls `ffi::fillet_all_edges` directly and
/// reaches no gate at all, which is a pre-existing gap of its own (#7742)
/// rather than anything the `BRepKind` stamp can fix.
#[test]
fn filleting_a_shell_swept_result_is_rejected_by_the_local_feature_gate() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let profile_id = pose_test_spine(&mut kernel, 0.020, 0.0);
    let path_id = pose_test_spine(&mut kernel, 0.0, 0.200);
    let swept = kernel
        .execute(&GeometryOp::Sweep {
            profile: profile_id,
            path: path_id,
        })
        .expect("wire-profile sweep should realize");
    let edges = kernel
        .extract_edges(swept.id)
        .expect("the swept shell enumerates edges");
    let first_edge = *edges.first().expect("the swept shell has edges");

    match kernel.execute(&GeometryOp::Fillet {
        target: swept.id,
        edges: vec![first_edge],
        radius: Value::Real(0.002),
    }) {
        Err(GeometryError::OperationFailed(msg)) => assert!(
            msg.contains("requires a BRepKind::Solid input shape"),
            "expected the local-feature gate's non-Solid refusal, got: {msg}"
        ),
        Ok(_) => panic!(
            "filleting a SHELL walked through the local-feature gate — the \
             gate admits only Solid, so the swept result is still carrying \
             a false Solid stamp"
        ),
        Err(other) => panic!("expected OperationFailed, got {other:?}"),
    }
}

/// Green-on-arrival fence: honest classification must not regress the
/// ordinary case. A correctly-posed face profile still sweeps to a Solid
/// and still fillets.
///
/// Spelled with a CURATED edge selection for the same reason its refusing
/// sibling above is: only that spelling routes through
/// `run_local_feature_with_history`, where the repr gate lives. An
/// empty-selection fillet here would assert that the gate ADMITS a Solid
/// while never executing the gate, so a gate that had regressed to
/// refusing everything would leave both halves of the pair green.
#[test]
fn a_correctly_posed_sweep_is_still_a_solid_and_still_fillets() {
    if !OCCT_AVAILABLE {
        eprintln!("skipping: OCCT not available");
        return;
    }
    let mut kernel = OcctKernel::new();
    let profile_id = pose_test_rect_profile(&mut kernel);
    let path_id = pose_test_spine(&mut kernel, 0.0, 0.200);

    let swept = kernel
        .execute(&GeometryOp::Sweep {
            profile: profile_id,
            path: path_id,
        })
        .expect("a +Z spine is normal to the profile and must sweep");
    assert_eq!(
        swept.repr,
        Some(BRepKind::Solid),
        "a face profile swept normal to the spine really is a Solid"
    );
    assert_eq!(kernel.repr_of(swept.id), Some(BRepKind::Solid));

    let edges = kernel
        .extract_edges(swept.id)
        .expect("the swept box enumerates edges");
    let first_edge = *edges.first().expect("the swept box has edges");
    kernel
        .execute(&GeometryOp::Fillet {
            target: swept.id,
            edges: vec![first_edge],
            radius: Value::Real(0.002),
        })
        .expect("a genuine Solid must still pass the local-feature gate");
}
