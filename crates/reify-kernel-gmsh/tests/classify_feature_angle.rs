//! Root-cause guard: `classify_surfaces` must actually separate a box's
//! planar faces into distinct B-rep entities (#6200).
//!
//! Only compiled / run when `cfg(has_gmsh)` is set by `build.rs`. On stub
//! builds this file is empty and the test binary contains zero tests —
//! preserving the all-OK posture of `cargo test -p reify-kernel-gmsh` on
//! hosts without libgmsh.
//!
//! # Why a census guard, and not just the fill fraction
//!
//! `tests/volume_fill_fraction.rs` pins the SYMPTOM of #6200 (an incomplete
//! tetrahedralization). This file pins its MECHANISM, so a future regression
//! fails with a message naming the actual cause rather than a bare volume
//! mismatch.
//!
//! The defect existed because a code comment asserted a B-rep property the
//! code did not produce — `kernel_real.rs` claimed a π/2 threshold "splits cube
//! faces into separate B-rep surface entities", while gmsh in fact logged
//! `Found 2 model surfaces` / `Found 1 model curves` and fell back to a purely
//! topological split (`Level 0 partition with 12 triangles split in 2 parts
//! because poincare characteristic 2 is not 0`). Nothing tested the claim.
//!
//! This guard therefore imports the PRODUCTION angle constants rather than
//! re-typing a literal. A test carrying its own copy of the angle would
//! re-create exactly the gap that caused the bug: someone could change the
//! production constant and the guard would keep passing against a stale
//! literal. Importing makes it structurally undriftable.
//!
//! Measured entity census on a welded 8-vertex box (dim2 / dim1 / dim0):
//! at a 90° feature angle 2 / 4 / 4; below 90°, 8 / 14 / 8. gmsh
//! over-decomposes (8 surfaces for 6 planar faces), which is why the
//! assertions are lower bounds rather than exact pins — the property that
//! matters is that the faces are SEPARATED at all.

#![cfg(has_gmsh)]

use reify_ir::Mesh;
use reify_kernel_gmsh::{CLASSIFY_CURVE_ANGLE, CLASSIFY_FEATURE_ANGLE, ffi, init};

/// Inline copy of `crates/reify-kernel-gmsh/tests/mesh_to_volume_tests.rs:13-48`,
/// generalised to arbitrary extents. 8 vertices / 12 outward-wound triangles.
fn prismatic_box_mesh(lx: f32, ly: f32, lz: f32) -> Mesh {
    Mesh {
        vertices: vec![
            0.0, 0.0, 0.0, // 0
            lx, 0.0, 0.0, // 1
            lx, ly, 0.0, // 2
            0.0, ly, 0.0, // 3
            0.0, 0.0, lz, // 4
            lx, 0.0, lz, // 5
            lx, ly, lz, // 6
            0.0, ly, lz, // 7
        ],
        #[rustfmt::skip]
        indices: vec![
            // -Z bottom (outward = -Z, so CW from +Z view)
            0, 2, 1,  0, 3, 2,
            // +Z top
            4, 5, 6,  4, 6, 7,
            // -Y front
            0, 1, 5,  0, 5, 4,
            // +Y back
            3, 7, 6,  3, 6, 2,
            // -X left
            0, 4, 7,  0, 7, 3,
            // +X right
            1, 2, 6,  1, 6, 5,
        ],
        normals: None,
    }
}

/// Entity census after classify + createGeometry, as `(dim0, dim1, dim2)`.
///
/// Replays only the classify half of `GmshKernel::mesh_to_volume`
/// (`kernel_real.rs`), stopping before surface-loop / volume / `mesh_generate(3)`.
///
/// This is a RAW-FFI test, so it MUST hold `init::GMSH_LOCK` itself — unlike
/// `volume_fill_fraction.rs`, which goes through the public API and would
/// self-deadlock if it took the lock.
fn entity_census(surface: &Mesh) -> (usize, usize, usize) {
    let n_verts = surface.vertices.len() / 3;
    let n_tris = surface.indices.len() / 3;

    let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init::ensure_initialized();

    ffi::clear().expect("clear");
    ffi::option_set_number("General.Terminal", 0.0).expect("terminal off");
    ffi::model_add("reify_6200_classify_census").expect("model_add");
    let surf_tag = ffi::add_discrete_entity(2, &[]).expect("add_discrete_entity");

    let node_tags: Vec<u64> = (1..=n_verts as u64).collect();
    let coords_f64: Vec<f64> = surface.vertices.iter().map(|&v| v as f64).collect();
    ffi::add_nodes_2d(surf_tag, &node_tags, &coords_f64).expect("add_nodes_2d");

    let tri_tags: Vec<u64> = (1..=n_tris as u64).collect();
    let tri_node_tags: Vec<u64> = surface.indices.iter().map(|&i| i as u64 + 1).collect();
    ffi::add_elements_2d(surf_tag, 2, &tri_tags, &tri_node_tags).expect("add_elements_2d");

    // The PRODUCTION constants, imported rather than re-typed — see the module
    // docs for why that matters.
    ffi::classify_surfaces(CLASSIFY_FEATURE_ANGLE, 1, 1, CLASSIFY_CURVE_ANGLE, 0)
        .expect("classify_surfaces");
    ffi::create_geometry(&[]).expect("create_geometry");

    let n0 = ffi::get_entity_tags(0).expect("get_entity_tags(0)").len();
    let n1 = ffi::get_entity_tags(1).expect("get_entity_tags(1)").len();
    let n2 = ffi::get_entity_tags(2).expect("get_entity_tags(2)").len();

    let _ = ffi::clear();
    (n0, n1, n2)
}

/// The production feature angle must be strictly BELOW a box's 90° dihedral.
///
/// gmsh's sharp-edge test is strictly-greater-than, so a threshold equal to the
/// dihedral angle never fires. This is the whole of #6200 in one assertion, and
/// it is checked without touching gmsh at all.
#[test]
fn feature_angle_is_strictly_below_a_right_dihedral() {
    assert!(
        CLASSIFY_FEATURE_ANGLE < std::f64::consts::FRAC_PI_2,
        "the classify feature angle is {CLASSIFY_FEATURE_ANGLE} rad; a box's dihedral \
         angle is EXACTLY π/2 and gmsh's sharp-edge test is strictly-greater-than, \
         so any threshold >= π/2 fails to register a box's own edges as sharp (#6200)"
    );
}

/// A box's six planar faces must be separated into distinct B-rep surfaces,
/// its twelve edges into curves, and its eight corners into dim-0 entities.
///
/// Measured at the pre-#6200 90° angle: 2 surfaces / 4 curves / 4 corners —
/// all three RED. Measured below 90°: 8 / 14 / 8.
#[test]
fn classify_separates_a_box_into_planar_brep_faces() {
    let (n0, n1, n2) = entity_census(&prismatic_box_mesh(1.0, 1.0, 1.0));

    assert!(
        n2 >= 6,
        "classify_surfaces produced {n2} dim-2 entities for a box's 6 planar faces \
         (census dim0/dim1/dim2 = {n0}/{n1}/{n2}). At a 90° feature angle this reads 2: \
         gmsh registers none of the box's edges as sharp and falls back to a purely \
         topological split, so create_geometry reparametrizes 2 non-planar patches \
         instead of 6 planar faces and the region handed to HXT is not the full box (#6200)."
    );
    assert!(
        n1 >= 12,
        "classify_surfaces produced {n1} dim-1 entities for a box's 12 edges \
         (census dim0/dim1/dim2 = {n0}/{n1}/{n2}). At a 90° feature angle this reads 4 (#6200)."
    );
    assert!(
        n0 >= 8,
        "classify_surfaces produced {n0} dim-0 entities for a box's 8 corners \
         (census dim0/dim1/dim2 = {n0}/{n1}/{n2}). At a 90° feature angle this reads 4 — \
         the same 'no corner (dim-0) entities' degeneracy already documented at \
         mesh_boundary.rs and refine_volume.rs (#6200)."
    );
}

/// The same census on the slender box the production path actually meshes,
/// confirming the decomposition is not an artefact of cubic symmetry.
#[test]
fn classify_separates_a_slender_box_into_planar_brep_faces() {
    let (n0, n1, n2) = entity_census(&prismatic_box_mesh(1.0, 0.1, 0.1));
    assert!(
        n2 >= 6 && n1 >= 12 && n0 >= 8,
        "box 1.0x0.1x0.1 m: census dim0/dim1/dim2 = {n0}/{n1}/{n2}, expected at least \
         8/12/6. A 10:1 box has the same exactly-90° dihedral angles as a cube, so it \
         fails identically at a 90° feature angle (#6200)."
    );
}
