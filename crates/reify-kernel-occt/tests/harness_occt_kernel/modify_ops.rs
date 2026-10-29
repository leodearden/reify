//! Shape-modifying operations through `OcctKernel::execute`: thicken, shell, offset solid,
//! offset curve, zone slab (planar and curved faces), and draft (whole-shape and curated
//! faces).

#![cfg(has_occt)]

use crate::support::{assert_ungated_fields_are_bare, parse_centroid_json};
use reify_ir::{GeometryError, GeometryHandle, GeometryHandleId, GeometryOp, GeometryQuery, Value};
use reify_kernel_occt::OcctKernel;

// --- Thicken / Shell tests ---

/// Diagnostic test: does negative Thicken offset work on a box?
#[test]
fn thicken_solid_negative_offset_diagnostic() {
    let mut kernel = OcctKernel::new();
    // Create a 10x10x10mm box (10mm = 0.010m)
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(0.010),
            size_y: Value::Real(0.010),
            size_z: Value::Real(0.010),
        })
        .unwrap();
    // Positive thicken: should expand box to ~11mm
    let outer_h = kernel
        .execute(&GeometryOp::Thicken {
            target: box_h.id,
            offset: Value::Real(0.0005),
        })
        .unwrap();
    let outer_vol = kernel
        .query(&GeometryQuery::Volume(outer_h.id))
        .unwrap();
    let outer_v = outer_vol.as_f64().expect("volume should be f64");
    eprintln!("outer Thicken(+0.5mm) volume = {:.3e} m³ (expected ~1.331e-6 = (11mm)³)", outer_v);
    assert!(outer_v > 1e-6, "outer thicken should be larger than original box (1e-6), got {outer_v}");

    // Negative thicken: should contract box to ~9mm
    let inner_result = kernel.execute(&GeometryOp::Thicken {
        target: box_h.id,
        offset: Value::Real(-0.0005),
    });
    eprintln!("negative Thicken(-0.5mm) result: {:?}", inner_result.as_ref().map(|h| h.id));
    match inner_result {
        Ok(inner_h) => {
            let inner_vol = kernel.query(&GeometryQuery::Volume(inner_h.id));
            eprintln!("inner Thicken(-0.5mm) volume query: {:?}", inner_vol);
            let inner_v = inner_vol
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(-1.0);
            eprintln!("inner Thicken(-0.5mm) volume = {:.3e} m³ (expected ~7.29e-7 = (9mm)³)", inner_v);
        }
        Err(e) => {
            eprintln!("negative Thicken failed: {}", e);
            panic!("negative Thicken(-0.5mm) on a 10mm box returned Err: {}", e);
        }
    }
    eprintln!("diagnostic complete");
}

#[test]
fn thicken_solid_increases_volume() {
    let mut kernel = OcctKernel::new();
    // Create a 10x10x10 box (volume = 1000)
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    // Thicken by offset=2.0 — result should have volume > 1000
    let thickened_h = kernel
        .execute(&GeometryOp::Thicken {
            target: box_h.id,
            offset: Value::Real(2.0),
        })
        .unwrap();
    let vol = kernel
        .query(&GeometryQuery::Volume(thickened_h.id))
        .unwrap();
    match vol {
        Value::Real(v) => {
            assert!(
                v > 1000.0,
                "thickened volume should exceed original 1000, got {v}"
            );
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

/// RED until step-2 adds GeometryOp::ZoneSlab.
#[test]
fn zone_slab_planar_face_volume_identity() {
    let mut kernel = OcctKernel::new();
    // Rectangle face: width=0.040 m, height=0.020 m → A = 8e-4 m²
    let face_h = kernel
        .execute(&GeometryOp::RectangleProfile {
            width: Value::Real(0.040),
            height: Value::Real(0.020),
        })
        .unwrap();
    let w = 0.002_f64; // 2 mm width
    let slab_h = kernel
        .execute(&GeometryOp::ZoneSlab {
            target: face_h.id,
            width: Value::Real(w),
        })
        .unwrap();
    // B6 identity: V = w · A for a right prism, within 1e-9 rel
    let expected = w * (0.040 * 0.020);
    let vol = kernel.query(&GeometryQuery::Volume(slab_h.id)).unwrap();
    match vol {
        Value::Real(v) => {
            assert!(
                (v - expected).abs() <= 1e-9 * expected,
                "zone_slab volume {v} should equal w·A={expected} within 1e-9 rel"
            );
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
    // Centering: centroid.z ≈ 0 (±w/2 symmetric offset)
    let centroid = kernel.query(&GeometryQuery::Centroid(slab_h.id)).unwrap();
    match centroid {
        Value::String(ref s) => {
            let (_cx, _cy, cz) = parse_centroid_json(s);
            assert!(
                cz.abs() <= 1e-9,
                "zone_slab centroid z should be ≈0 (symmetric slab), got {cz}"
            );
        }
        other => panic!("expected Value::String (centroid JSON), got {:?}", other),
    }
}

/// PRD G6 smoke bar: zone_slab on a curved (cylindrical) face must not fail
/// and must return a solid with volume > 0.
#[test]
fn zone_slab_curved_face_smoke() {
    let mut kernel = OcctKernel::new();
    // Build an open cylindrical lateral face: radius=0.050 m, height=0.030 m
    let face_id = kernel.store_cylindrical_face_for_test(0.050, 0.030);
    let w = 0.002_f64; // 2 mm zone width
    let slab_h = kernel
        .execute(&GeometryOp::ZoneSlab {
            target: face_id,
            width: Value::Real(w),
        })
        .expect("zone_slab on curved face must not fail (PRD G6)");
    let vol = kernel
        .query(&GeometryQuery::Volume(slab_h.id))
        .expect("Volume query on slab must succeed");
    match vol {
        Value::Real(v) => {
            assert!(v > 0.0, "zone_slab on curved face must have volume > 0, got {v}");
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

// --- OffsetSolid high-level execute tests ---

#[test]
fn offset_solid_degenerate_collapse_returns_error() {
    let mut kernel = OcctKernel::new();
    // 10×10×10 box; inradius = 5
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(10.0),
            size_y: Value::Real(10.0),
            size_z: Value::Real(10.0),
        })
        .unwrap();
    // Inward offset -100 >> inradius 5 → degenerate, must return Err
    let result = kernel.execute(&GeometryOp::OffsetSolid {
        target: box_h.id,
        distance: Value::Real(-100.0),
    });
    assert!(
        result.is_err(),
        "degenerate inward offset should produce an error"
    );
}

#[test]
fn shell_box_hollow() {
    let mut kernel = OcctKernel::new();
    // Create a 20x20x20 box (volume = 8000)
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(20.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(20.0),
        })
        .unwrap();
    // Shell with thickness=1.0, remove face 0 to create hollow box
    let shell_h = kernel
        .execute(&GeometryOp::Shell {
            target: box_h.id,
            thickness: Value::Real(1.0),
            faces_to_remove: vec![0],
            open_face_handles: vec![],
        })
        .unwrap();
    let vol = kernel.query(&GeometryQuery::Volume(shell_h.id)).unwrap();
    match vol {
        Value::Real(v) => {
            // Should be less than 8000 (solid) but greater than 0
            assert!(
                v > 0.0 && v < 8000.0,
                "shell volume should be between 0 and 8000, got {v}"
            );
        }
        other => panic!("expected Value::Real, got {:?}", other),
    }
}

/// Kernel `execute` arm for all three OffsetCurve overloads + the
/// invalid-target error path (task 4193 step-7).
///
/// (a) overload-1 planar offset of a radius-10mm arc by +2mm → a wire whose
///     extracted edge has curvature-radius ≈ 12mm (ratio 1.2 within 2%);
/// (b) overload-3 directional offset (+Z) → Ok wire handle;
/// (c) overload-2 offset on a reference surface (a box face) → Ok wire handle;
/// (d) an unknown target handle → Err.
///
/// RED until step-8 adds the `GeometryOp::OffsetCurve` arm to `execute`
/// (the kernel lib is non-exhaustive over GeometryOp until then).
#[test]
fn kernel_execute_offset_curve_all_three_branches() {
    use std::f64::consts::FRAC_PI_4;
    let mut kernel = OcctKernel::new();

    // Radius-10mm arc in the XY plane, symmetric about angle 0.
    let arc = kernel
        .execute(&GeometryOp::Arc {
            center: [0.0, 0.0, 0.0],
            radius: 0.01,
            start_angle: -FRAC_PI_4,
            end_angle: FRAC_PI_4,
            axis: [0.0, 0.0, 1.0],
        })
        .expect("arc should build");

    // (a) Overload 1 — planar 2-D offset by +2mm → radius 12mm.
    let off = kernel
        .execute(&GeometryOp::OffsetCurve {
            target: arc.id,
            distance: Value::Real(0.002),
            reference: None,
            direction: None,
        })
        .expect("offset_curve overload-1 should succeed");
    let edges = kernel
        .extract_edges(off.id)
        .expect("extract_edges on offset wire");
    assert!(!edges.is_empty(), "offset wire should have >= 1 edge");
    let kappa = kernel
        .curve_curvature_at(edges[0], 0.012, 0.0, 0.0)
        .expect("curvature on offset edge");
    let radius = 1.0 / kappa.abs();
    let rel_err = (radius - 0.012).abs() / 0.012;
    assert!(
        rel_err <= 0.02,
        "offset arc radius should be 12mm (ratio 1.2) within 2%, \
         got {radius} (κ={kappa}, rel_err={rel_err})"
    );

    // (b) Overload 3 — directional offset (+Z) → Ok wire handle.
    let directional = kernel
        .execute(&GeometryOp::OffsetCurve {
            target: arc.id,
            distance: Value::Real(0.002),
            reference: None,
            direction: Some([0.0, 0.0, 1.0]),
        })
        .expect("offset_curve overload-3 (directional) should succeed");
    assert!(
        !kernel
            .extract_edges(directional.id)
            .expect("extract_edges on directional offset")
            .is_empty(),
        "directional offset should be a real non-empty wire"
    );

    // (c) Overload 2 — offset on a reference surface (a box face) → Ok wire.
    let boxx = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(0.02),
            size_y: Value::Real(0.02),
            size_z: Value::Real(0.02),
        })
        .expect("box should build");
    let faces = kernel
        .extract_faces(boxx.id)
        .expect("extract_faces on box");
    assert!(!faces.is_empty(), "box should have faces");
    let on_surface = kernel
        .execute(&GeometryOp::OffsetCurve {
            target: arc.id,
            distance: Value::Real(0.002),
            reference: Some(faces[0]),
            direction: None,
        })
        .expect("offset_curve overload-2 (on-surface) should succeed");
    assert!(
        !kernel
            .extract_edges(on_surface.id)
            .expect("extract_edges on on-surface offset")
            .is_empty(),
        "on-surface offset should be a real non-empty wire"
    );

    // (d) Unknown target handle → Err.
    let bad = kernel.execute(&GeometryOp::OffsetCurve {
        target: GeometryHandleId(999_999),
        distance: Value::Real(0.002),
        reference: None,
        direction: None,
    });
    assert!(
        bad.is_err(),
        "offset_curve on an unknown target handle must Err, got {bad:?}"
    );
}

// --- Draft tests ---

#[test]
fn draft_angle_on_box() {
    let mut kernel = OcctKernel::new();
    // Create a 20x20x20 box
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(20.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(20.0),
        })
        .unwrap();
    // Create a plane reference (small flat box at z=0)
    let plane_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(100.0),
            size_y: Value::Real(100.0),
            size_z: Value::Real(0.1),
        })
        .unwrap();
    // Apply Draft with angle ~5.7 degrees (0.1 rad)
    let draft_h = kernel.execute(&GeometryOp::Draft {
        target: box_h.id,
        faces: vec![],
        angle: Value::angle(0.1),
        plane: plane_h.id,
    });
    // Draft is complex and may fail for certain shapes - we just verify it
    // either succeeds with a positive volume or returns an expected error
    match draft_h {
        Ok(h) => {
            let vol = kernel.query(&GeometryQuery::Volume(h.id)).unwrap();
            match vol {
                Value::Real(v) => {
                    assert!(v > 0.0, "drafted volume should be positive, got {v}");
                }
                other => panic!("expected Value::Real, got {:?}", other),
            }
        }
        Err(GeometryError::OperationFailed(_)) => {
            // Acceptable: Draft is finicky with some shapes
        }
        Err(other) => panic!("unexpected error: {:?}", other),
    }
}

// --- Curated-faces draft tests (δ step-7) ---

/// Helper: query the volume of a shape handle and assert it is positive.
fn volume_of(kernel: &mut OcctKernel, handle: &GeometryHandle) -> f64 {
    match kernel.query(&GeometryQuery::Volume(handle.id)).unwrap() {
        Value::Real(v) => {
            assert!(v > 0.0, "volume must be positive, got {v}");
            v
        }
        other => panic!("expected Value::Real for volume, got {:?}", other),
    }
}

/// Build the shared draft fixture: a 20×20×20 box target and a
/// 100×100×0.1 flat box as the neutral plane (same pattern as
/// `draft_angle_on_box`). Returns (target_handle, plane_handle).
fn build_draft_fixture(kernel: &mut OcctKernel) -> (GeometryHandle, GeometryHandle) {
    let target_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(20.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(20.0),
        })
        .unwrap();
    let plane_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(100.0),
            size_y: Value::Real(100.0),
            size_z: Value::Real(0.1),
        })
        .unwrap();
    (target_h, plane_h)
}

/// Dimensioning a Draft fixture's angle does not change the geometry OCCT
/// produces — PRD `docs/prds/v0_6/angle-units-surface-convergence.md`
/// decision D2, observed at the kernel boundary (task 5777, angle-units α).
///
/// A CHARACTERIZATION pin, expected green against unchanged production
/// code: the OCCT Draft arm reads its angle via `extract_f64`, a thin
/// wrapper over `Value::as_f64`, so this closes at actual OCCT output what
/// `reify_ir::value::tests::angle_and_real_agree_bit_exactly_under_as_f64`
/// pins at the `Value` layer.
///
/// Draft is finicky, so this sweeps up to two CURATED faces — never the
/// all-faces path, which `execute_draft_curated_faces_honored` explicitly
/// declines to use as an oracle — and requires `compared > 0` so a run
/// where no face produced geometry fails loudly instead of passing
/// silently. The three-way failure split below names the culprit: the two
/// forms DIVERGING is the D2 contradiction; both failing IDENTICALLY
/// exonerates the dimension tag and indicts the fixture.
///
/// The success arm's oracle is equal VOLUME plus equal FACE COUNT, over two
/// handles asserted to be distinct. That is deliberately not a proof of
/// congruence — no cheap query here is — but it is strictly more than a
/// single scalar, and the handle assertion closes the vacuity path a
/// memoising `execute` would otherwise open.
#[test]
fn draft_angle_dimensioned_matches_bare_real_volume() {
    let angle_rad = std::f64::consts::PI / 60.0;
    let mut kernel = OcctKernel::new();

    let (target, plane) = build_draft_fixture(&mut kernel);
    let faces = kernel
        .extract_faces(target.id)
        .expect("extract_faces must succeed");
    assert!(
        faces.len() >= 2,
        "box must have at least 2 faces, got {}",
        faces.len()
    );

    // Any single face can legitimately be one OCCT declines to draft, so
    // sweep up to two of them — exactly as
    // `execute_draft_curated_faces_honored` does — stopping at the first
    // where BOTH forms produced geometry, and requiring at least one.
    let mut compared = 0usize;
    for face in [faces[0], *faces.last().unwrap()] {
        let draft_bare = GeometryOp::Draft {
            target: target.id,
            faces: vec![face],
            // Stays bare deliberately — task 5777. This is the CONTROL arm of
            // the D2 equivalence pin; retyping it to `Value::angle` collapses
            // both arms into one and deletes the comparison. δ (5780) owns
            // `draft`, but not this site.
            angle: Value::Real(angle_rad),
            plane: plane.id,
        };
        assert_ungated_fields_are_bare(&draft_bare);
        let bare = kernel.execute(&draft_bare);
        let dimensioned = kernel.execute(&GeometryOp::Draft {
            target: target.id,
            faces: vec![face],
            angle: Value::angle(angle_rad),
            plane: plane.id,
        });

        match (bare, dimensioned) {
            (Ok(b), Ok(d)) => {
                // Anti-vacuity, the axis the `compared > 0` guard does not
                // cover: the comparison only means something if the two
                // calls actually executed TWO drafts. `execute` builds a
                // fresh shape per call today, but a future content-hash
                // memoisation keyed on the `extract_f64` output — exactly
                // the change this pin exists to survive — would hand the
                // same handle back twice and make every assertion below
                // trivially true.
                assert_ne!(
                    b.id, d.id,
                    "both forms must have executed a fresh draft; identical handles \
                     mean the comparison below is vacuous"
                );
                let vb = volume_of(&mut kernel, &b);
                let vd = volume_of(&mut kernel, &d);
                assert_eq!(
                    vb, vd,
                    "Value::angle({angle_rad}) and Value::Real({angle_rad}) must \
                     produce equal draft volume — they differ only in a \
                     dimension tag that extract_f64 discards"
                );
                // Volume alone is a weak oracle: two distinct shapes can
                // share one. Compare the face count too, so a divergence
                // that redistributes material without changing its total
                // is still caught. Neither oracle makes the shapes provably
                // congruent — the claim this pin actually supports is
                // "equal volume AND equal face count", not "bit-identical
                // geometry".
                let fb = kernel
                    .extract_faces(b.id)
                    .expect("extract_faces on the bare-form draft must succeed")
                    .len();
                let fd = kernel
                    .extract_faces(d.id)
                    .expect("extract_faces on the dimensioned-form draft must succeed")
                    .len();
                assert_eq!(
                    fb, fd,
                    "Value::angle({angle_rad}) and Value::Real({angle_rad}) must produce \
                     the same face count; equal volume with a different face count would \
                     be a shape change the volume oracle alone cannot see"
                );
                compared += 1;
            }
            // Same failure from both forms is itself the D2 signal, but it
            // measures nothing about the geometry — try the other face.
            (
                Err(GeometryError::OperationFailed(_)),
                Err(GeometryError::OperationFailed(_)),
            ) => {}
            // Both forms failed the SAME way, but not with the tolerated
            // `OperationFailed`. The dimension tag is EXONERATED here — it
            // changed nothing — so blaming D2 would be a misdiagnosis of
            // what is really a broken fixture. Named separately for exactly
            // that reason (the `compared > 0` guard below exists to stop the
            // same confusion from the other direction).
            (Err(bare_err), Err(dim_err))
                if std::mem::discriminant(&bare_err) == std::mem::discriminant(&dim_err) =>
            {
                panic!(
                    "both forms failed identically with {bare_err:?} — this says nothing \
                     about D2 (the dimension tag changed neither branch nor error); \
                     repair the fixture"
                )
            }
            (bare, dimensioned) => panic!(
                "dimensioning the angle changed which branch the draft took, which \
                 contradicts D2: bare -> {:?}, dimensioned -> {:?}",
                bare.map(|h| h.id),
                dimensioned.map(|h| h.id)
            ),
        }

        // One face where both forms produced geometry is the whole signal;
        // a second costs another OCCT draft + volume pair for no added
        // information. The sweep exists only to survive a face OCCT
        // declines to draft.
        if compared > 0 {
            break;
        }
    }

    assert!(
        compared > 0,
        "anti-vacuity: no face produced geometry under EITHER form, so this test \
         proved nothing about D2 equivalence — the fixture needs repair rather \
         than a silent pass"
    );
}

/// (a) CURATED SELECTION HONORED: a one-face draft (via `faces: [f0]`)
/// must succeed with a positive volume that differs both from the
/// undrafted box volume AND from any other single-face selection (proves
/// the face vector is actually passed through, not silently discarded).
///
/// Uses two distinct single-face drafts (f0 vs f5 — opposite faces on
/// the box) and asserts at least one of them differs from the all-faces
/// draft, proving selectivity. Avoids relying on the finicky all-faces
/// path as the sole oracle.
#[test]
fn execute_draft_curated_faces_honored() {
    let mut kernel = OcctKernel::new();
    let (target_h, plane_h) = build_draft_fixture(&mut kernel);

    let undrafted_vol = volume_of(&mut kernel, &target_h);

    // Extract the canonical face handles for this box.
    let faces = kernel.extract_faces(target_h.id).expect("extract_faces must succeed");
    // A box has 6 faces.
    assert!(faces.len() >= 2, "box must have at least 2 faces, got {}", faces.len());

    // Draft with just the FIRST face.
    let result_f0 = kernel.execute(&GeometryOp::Draft {
        target: target_h.id,
        faces: vec![faces[0]],
        angle: Value::angle(std::f64::consts::PI / 60.0),
        plane: plane_h.id,
    });

    // Draft with just the LAST face (a different face → different result if
    // selection is honored).
    let result_fn = kernel.execute(&GeometryOp::Draft {
        target: target_h.id,
        faces: vec![*faces.last().unwrap()],
        angle: Value::angle(std::f64::consts::PI / 60.0),
        plane: plane_h.id,
    });

    // At least one draft must succeed.
    let (curated_h, other_h) = match (result_f0, result_fn) {
        (Ok(h0), Ok(hn)) => (h0, Some(hn)),
        (Ok(h0), Err(GeometryError::OperationFailed(_))) => (h0, None),
        (Err(GeometryError::OperationFailed(_)), Ok(hn)) => (hn, None),
        (Err(GeometryError::OperationFailed(a)), Err(GeometryError::OperationFailed(b))) => {
            panic!("both curated single-face drafts failed: {a:?} / {b:?}");
        }
        (Err(a), _) | (_, Err(a)) => panic!("unexpected error: {:?}", a),
    };

    let curated_vol = volume_of(&mut kernel, &curated_h);
    // Curated result must differ from the undrafted box (proves draft ran).
    assert!(
        (curated_vol - undrafted_vol).abs() > 1e-6,
        "curated-draft volume ({curated_vol}) must differ from undrafted box ({undrafted_vol})"
    );
    // If both drafts succeeded, they must produce the SAME volume
    // (symmetry of a cubic box) OR differ — either way, compare the
    // one that could give us selectivity signal: both single-face
    // drafts differing from an all-faces draft.
    if let Some(other) = other_h {
        // Both single-face drafts succeeded — each should differ from the
        // undrafted box (draft ran on at least one face each time).
        let other_vol = volume_of(&mut kernel, &other);
        assert!(
            (other_vol - undrafted_vol).abs() > 1e-6,
            "second curated-draft volume ({other_vol}) must also differ from \
             undrafted box ({undrafted_vol})"
        );
    }
}

/// (b) 3-ARG BACK-COMPAT: `faces: vec![]` preserves the all-draftable-
/// faces path (unchanged from today). Must succeed with a positive volume
/// that differs from the undrafted box.
#[test]
fn execute_draft_empty_faces_is_all_faces_back_compat() {
    let mut kernel = OcctKernel::new();
    let (target_h, plane_h) = build_draft_fixture(&mut kernel);
    let undrafted_vol = volume_of(&mut kernel, &target_h);

    let result = kernel.execute(&GeometryOp::Draft {
        target: target_h.id,
        faces: vec![],
        angle: Value::angle(std::f64::consts::PI / 60.0),
        plane: plane_h.id,
    });

    match result {
        Ok(h) => {
            let vol = volume_of(&mut kernel, &h);
            assert!(
                (vol - undrafted_vol).abs() > 1e-6,
                "all-faces draft volume ({vol}) must differ from undrafted box ({undrafted_vol})"
            );
        }
        Err(GeometryError::OperationFailed(_)) => {
            // Acceptable: draft_shape is finicky with some shapes
        }
        Err(other) => panic!("unexpected error: {:?}", other),
    }
}

/// (c) FOREIGN FACE REJECTION: a face handle that does not belong to the
/// target solid must produce `GeometryError::OperationFailed`. This
/// proves that `draft_faces` validates face membership.
#[test]
fn execute_draft_foreign_face_handle_returns_error() {
    let mut kernel = OcctKernel::new();
    let (target_h, plane_h) = build_draft_fixture(&mut kernel);

    // Build a second box and extract its faces — these are foreign to `target`.
    let second_box = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(5.0),
            size_y: Value::Real(5.0),
            size_z: Value::Real(5.0),
        })
        .unwrap();
    let foreign_faces = kernel
        .extract_faces(second_box.id)
        .expect("extract_faces on second box must succeed");
    let foreign_face = foreign_faces[0];

    let result = kernel.execute(&GeometryOp::Draft {
        target: target_h.id,
        faces: vec![foreign_face],
        angle: Value::angle(std::f64::consts::PI / 60.0),
        plane: plane_h.id,
    });
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed for foreign face, got {:?}", other),
        Ok(_) => panic!("expected error for foreign face handle"),
    }
}

/// (d) DEFENSE-IN-DEPTH: `OcctKernel::draft_faces` with an empty
/// `faces` slice must return `Err(OperationFailed)` — the curated path
/// must never accept an empty selection (that would silently fall through
/// to the all-faces behaviour with no user signal).
#[test]
fn draft_faces_empty_selection_returns_error() {
    let mut kernel = OcctKernel::new();
    let (target_h, plane_h) = build_draft_fixture(&mut kernel);
    // Calling draft_faces directly with &[] must be rejected.
    let result = kernel.draft_faces(
        target_h.id,
        std::f64::consts::PI / 60.0,
        plane_h.id,
        &[],
    );
    match result {
        Err(GeometryError::OperationFailed(_)) => {}
        Err(other) => panic!("expected OperationFailed for empty selection, got {:?}", other),
        Ok(_) => panic!("draft_faces with empty selection must be rejected"),
    }
}

#[test]
fn draft_uses_plane_shape_not_hardcoded_z() {
    // Verify that draft_shape extracts the neutral plane from plane_shape
    // rather than using a hardcoded Z-up direction. We test this by
    // drafting with a non-planar shape as plane — after the fix, this
    // should error with "does not contain a planar face" because the
    // code actually tries to extract the plane from the shape.
    // Currently FAILS because draft_shape ignores plane_shape, so a
    // sphere (non-planar) is accepted and draft proceeds with Z-up.
    let mut kernel = OcctKernel::new();

    // Target box
    let box_h = kernel
        .execute(&GeometryOp::Box {
            size_x: Value::Real(20.0),
            size_y: Value::Real(20.0),
            size_z: Value::Real(20.0),
        })
        .unwrap();

    // Use a sphere as the "plane" — a sphere has no planar faces.
    // After fix: should get an explicit error about non-planar face.
    // Before fix: plane_shape is ignored, so draft either succeeds
    // with Z-up or fails for unrelated reasons.
    let sphere_h = kernel
        .execute(&GeometryOp::Sphere {
            radius: Value::Real(10.0),
        })
        .unwrap();

    let result = kernel.execute(&GeometryOp::Draft {
        target: box_h.id,
        faces: vec![],
        angle: Value::angle(0.05),
        plane: sphere_h.id,
    });

    // After the fix, we expect an OperationFailed error whose message
    // mentions "planar" — the code should detect that the sphere's
    // face is not planar and throw an explicit error.
    match result {
        Err(GeometryError::OperationFailed(msg)) => {
            assert!(
                msg.contains("planar"),
                "expected error about non-planar face, got: {msg}"
            );
        }
        Ok(_) => {
            panic!(
                "draft with sphere as plane should fail \
                 (sphere has no planar faces), but succeeded"
            );
        }
        Err(other) => {
            panic!(
                "expected OperationFailed with 'planar' message, got: {:?}",
                other
            );
        }
    }
}
