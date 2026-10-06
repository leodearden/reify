//! What this crate's gmsh test binaries share that does not live in
//! `reify_test_support`: one not-yet-hoisted fixture
//! [`subdivided_unit_cube_surface`] and the raw-FFI entity census
//! [`entity_census`].
//!
//! A `tests/common/` subdirectory (rather than a sibling `tests/*.rs` file) is
//! the cargo idiom for both: files under `tests/` are each compiled as their
//! own test binary, files under `tests/common/` are not. The directory would
//! stay even without this file: `clamp_probe.rs` and `size_field.rs` sit beside
//! it and are `#[path]`-included by the binaries that use them.
//!
//! # Fixtures live in `reify_test_support`
//!
//! The workspace-canonical box, cube and cylinder fixtures live in
//! [`reify_test_support::mesh_fixtures`] (#6387). Consumers import them from
//! there directly, spelling the `mesh_fixtures::` module path; #7141 removed
//! the list that used to forward them through here. No new shared FIXTURE
//! belongs here — put it in `reify_test_support`. The one exception,
//! [`subdivided_unit_cube_surface`], is pending that same move under #8048.
//!
//! # The census is NOT a fixture, and stays (#6830)
//!
//! [`entity_census`] cannot follow the fixtures into `reify_test_support`, for
//! two independent reasons. It is not a fixture: it holds no geometry, it is a
//! raw-FFI harness that takes geometry as an argument. And it structurally
//! cannot move: its body calls `reify_kernel_gmsh::{ffi, init,
//! CLASSIFY_FEATURE_ANGLE, CLASSIFY_CURVE_ANGLE}`, all `#[cfg(has_gmsh)]`
//! crate-root items, while `reify-test-support` has no reify-kernel-gmsh edge.
//! Adding one would drag the gmsh FFI and its libgmsh link closure into
//! `crates/reify-audit`'s PRODUCTION build graph — reify-audit carries
//! reify-test-support as a normal dependency, not a dev-dependency — and would
//! invert the adapter -> test-support direction the workspace maintains.
//!
//! # The lint below is load-bearing
//!
//! Every consumer binary compiles its own copy of this module and uses only
//! part of it — `classify_feature_angle.rs` never calls
//! [`subdivided_unit_cube_surface`], `gmsh_classify_diagnostics.rs` never calls
//! [`entity_census`] — so the uncalled `fn` is `dead_code` in that binary and
//! must not be an error under `-D warnings`.

#![allow(dead_code)]

use reify_ir::Mesh;

// ---------------------------------------------------------------------------
// subdivided_unit_cube_surface — awaiting its hoist, see the module docs
// ---------------------------------------------------------------------------

/// Build a 2×2-subdivided unit cube (side 1.0, centred at origin):
/// 8 corners + 12 edge midpoints + 6 face centres = 26 unique vertices, 48
/// triangles (6 faces × 8 sub-triangles, outward-facing).
pub fn subdivided_unit_cube_surface() -> Mesh {
    #[rustfmt::skip]
    let corners: [[f32; 3]; 8] = [
        [-0.5, -0.5, -0.5], [ 0.5, -0.5, -0.5],
        [-0.5,  0.5, -0.5], [ 0.5,  0.5, -0.5],
        [-0.5, -0.5,  0.5], [ 0.5, -0.5,  0.5],
        [-0.5,  0.5,  0.5], [ 0.5,  0.5,  0.5],
    ];
    #[rustfmt::skip]
    let edges: [[f32; 3]; 12] = [
        [ 0.0, -0.5, -0.5], [-0.5,  0.0, -0.5], [ 0.5,  0.0, -0.5], [ 0.0,  0.5, -0.5],
        [ 0.0, -0.5,  0.5], [-0.5,  0.0,  0.5], [ 0.5,  0.0,  0.5], [ 0.0,  0.5,  0.5],
        [-0.5, -0.5,  0.0], [ 0.5, -0.5,  0.0], [-0.5,  0.5,  0.0], [ 0.5,  0.5,  0.0],
    ];
    #[rustfmt::skip]
    let face_centers: [[f32; 3]; 6] = [
        [ 0.0,  0.0, -0.5], [ 0.0,  0.0,  0.5],
        [ 0.0, -0.5,  0.0], [ 0.0,  0.5,  0.0],
        [-0.5,  0.0,  0.0], [ 0.5,  0.0,  0.0],
    ];
    let mut vertices: Vec<f32> = Vec::with_capacity(26 * 3);
    for c in &corners { vertices.extend_from_slice(c); }
    for e in &edges   { vertices.extend_from_slice(e); }
    for f in &face_centers { vertices.extend_from_slice(f); }
    assert_eq!(vertices.len(), 26 * 3);

    #[rustfmt::skip]
    let indices: Vec<u32> = vec![
        // Bottom (z=-0.5): vertex indices 8=edge[0], 9=edge[1], 10=edge[2], 11=edge[3], 20=fc[0]
        0, 9,20,  0,20, 8,  8,20,10,  8,10, 1,
        9, 2,11,  9,11,20, 20,11, 3, 20, 3,10,
        // Top (z=0.5)
        4,12,21,  4,21,13, 12, 5,14, 12,14,21,
       13,21,15, 13,15, 6, 21,14, 7, 21, 7,15,
        // Front (y=-0.5)
        0, 8,22,  0,22,16,  8, 1,17,  8,17,22,
       16,22,12, 16,12, 4, 22,17, 5, 22, 5,12,
        // Back (y=0.5)
        2,18,23,  2,23,11, 11,23,19, 11,19, 3,
       18, 6,15, 18,15,23, 23,15, 7, 23, 7,19,
        // Left (x=-0.5)
        0,16,24,  0,24, 9,  9,24,18,  9,18, 2,
       16, 4,13, 16,13,24, 24,13, 6, 24, 6,18,
        // Right (x=0.5)
        1,10,25,  1,25,17, 10, 3,19, 10,19,25,
       17,25,14, 17,14, 5, 25,19, 7, 25, 7,14,
    ];
    assert_eq!(indices.len(), 48 * 3);

    Mesh { vertices, indices, normals: None }
}

// ---------------------------------------------------------------------------
// Raw-FFI entity census (has_gmsh only)
// ---------------------------------------------------------------------------

// GATED DELIBERATELY, and the gate is load-bearing — see the module docs above.
// `ffi`, `init`, `CLASSIFY_FEATURE_ANGLE` and `CLASSIFY_CURVE_ANGLE` are all
// `#[cfg(has_gmsh)]` at the crate root (`src/lib.rs`), while
// `node_attachment_producer.rs` includes this module without a has_gmsh gate
// (it is gated only on `feature = "mesh-morph"`). Un-gating either `use` or the
// `fn` below compiles fine on a libgmsh host and silently breaks every
// stub-host build.
#[cfg(has_gmsh)]
use reify_kernel_gmsh::{CLASSIFY_CURVE_ANGLE, CLASSIFY_FEATURE_ANGLE, ffi, init};

/// Entity census after classify + createGeometry, as `(dim0, dim1, dim2)`.
///
/// Replays only the classify half of `GmshKernel::mesh_to_volume`
/// (`kernel_real.rs`) / the `run_meshing_with_entity_queries` prefix
/// (`mesh_boundary.rs`), stopping before surface-loop / volume /
/// `mesh_generate(3)`. That keeps it fast and isolates just the topology
/// reconstruction step.
///
/// This is a RAW-FFI helper, so it MUST hold `init::GMSH_LOCK` itself — unlike
/// `volume_fill_fraction.rs`, which goes through the public API and would
/// self-deadlock if it took the lock. The guard is scoped to one invocation and
/// drops at return, so back-to-back calls do not deadlock.
///
/// It uses the PRODUCTION angle constants rather than re-typed literals. A test
/// carrying its own copy of the angle would re-create exactly the gap that
/// caused #6200: someone could change the production constant and the guard
/// would keep passing against a stale literal. Centralising the call here means
/// ONE import site instead of two that could drift apart.
///
/// `model_name` is PURELY DIAGNOSTIC — it labels gmsh's own log output when a
/// census test fails, and cannot affect the result because `ffi::clear()` runs
/// first and wipes all models. It is a parameter rather than a fixed constant
/// so each call site keeps its own diagnostic identity. That safety is a TESTED
/// property, not a comment: `classify_feature_angle.rs`'s
/// `entity_census_is_isolated_across_invocations` passes two different names
/// for identical geometry and asserts one triple (measured `(8,14,8)` both
/// times).
#[cfg(has_gmsh)]
pub fn entity_census(surface: &Mesh, model_name: &str) -> (usize, usize, usize) {
    let n_verts = surface.vertices.len() / 3;
    let n_tris = surface.indices.len() / 3;

    let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init::ensure_initialized();

    ffi::clear().expect("clear");
    ffi::option_set_number("General.Terminal", 0.0).expect("terminal off");
    ffi::model_add(model_name).expect("model_add");
    let surf_tag = ffi::add_discrete_entity(2, &[]).expect("add_discrete_entity");

    let node_tags: Vec<u64> = (1..=n_verts as u64).collect();
    let coords_f64: Vec<f64> = surface.vertices.iter().map(|&v| v as f64).collect();
    ffi::add_nodes_2d(surf_tag, &node_tags, &coords_f64).expect("add_nodes_2d");

    let tri_tags: Vec<u64> = (1..=n_tris as u64).collect();
    let tri_node_tags: Vec<u64> = surface.indices.iter().map(|&i| i as u64 + 1).collect();
    ffi::add_elements_2d(surf_tag, 2, &tri_tags, &tri_node_tags).expect("add_elements_2d");

    // The PRODUCTION constants, imported rather than re-typed — see above.
    ffi::classify_surfaces(CLASSIFY_FEATURE_ANGLE, 1, 1, CLASSIFY_CURVE_ANGLE, 0)
        .expect("classify_surfaces");
    ffi::create_geometry(&[]).expect("create_geometry");

    let n0 = ffi::get_entity_tags(0).expect("get_entity_tags(0)").len();
    let n1 = ffi::get_entity_tags(1).expect("get_entity_tags(1)").len();
    let n2 = ffi::get_entity_tags(2).expect("get_entity_tags(2)").len();

    let _ = ffi::clear();
    (n0, n1, n2)
}
