//! The one place that deliberately drives gmsh's mesher into failure.
//!
//! Gmsh's mesher is process-global, and a failed `gmshModelMeshGenerate`
//! used to leave it unusable for the rest of the process: every later
//! generate returned `ierr=0` with no elements instead of an error, so the
//! next caller got a silent, plausible-looking `Ok` holding zero
//! tetrahedra. `gmshClear()` did not lift that; a
//! `gmshFinalize`+`gmshInitialize` cycle does.
//!
//! Every test here therefore fails a mesh on purpose and then asserts the
//! process is still usable — that the damage is confined to the call that
//! earned it. The failing call itself is expected to be loud; what is under
//! test is the state it leaves behind.
//!
//! These tests live in their own binary rather than in
//! `mesh_to_volume_tests.rs` so that a recovery regression reds one binary
//! whose name states the cause, instead of reddening unrelated assertions
//! spread across the crate's other test binaries.
//!
//! Only compiled / run when `cfg(has_gmsh)` is set by `build.rs`. On stub
//! builds this file is empty and the test binary contains zero tests —
//! preserving the all-OK posture of `cargo test -p reify-kernel-gmsh` on
//! hosts without libgmsh.

#![cfg(has_gmsh)]

use reify_ir::{ElementOrderTag, Mesh};
use reify_kernel_gmsh::{GmshKernel, MeshingOptions, refine_volume_with_size_field};
use reify_test_support::fixtures::unit_cube_mesh;

/// A single open triangle: a surface gmsh accepts and classifies happily but
/// that HXT cannot 3D-mesh, because it bounds no closed region.
///
/// This is the cheapest known input that reaches `gmshModelMeshGenerate(3)`
/// and fails there — the precise failure this binary needs. Not hoisted into
/// `reify_test_support::fixtures`: it has exactly one consumer, and
/// `tests/common/mod.rs` states in its own header that no new shared fixture
/// belongs there.
fn unmeshable_open_triangle() -> Mesh {
    Mesh {
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        indices: vec![0, 1, 2],
        normals: None,
    }
}

/// A `mesh_to_volume` that fails at the mesher must not take the next
/// caller's mesh down with it.
///
/// Meshes the open triangle (expected `Err` from `gmshModelMeshGenerate`),
/// then meshes a known-good closed cube through the same entry point and
/// requires a real tet mesh back.
///
/// The tet assertion is `> 0`, never an exact count: measured counts for this
/// same cube varied 744/748/752 across runs under multithreaded HXT, a jitter
/// `mesh_to_volume_tests.rs::cuboid_round_trip_within_count_variation_budget`
/// already acknowledges in tree.
#[test]
fn a_failed_mesh_to_volume_leaves_the_mesher_usable_for_the_next_caller() {
    let kernel = GmshKernel::new();
    let options = MeshingOptions::default();

    let poisoning =
        kernel.mesh_to_volume(&unmeshable_open_triangle(), &options, ElementOrderTag::P1);
    let err = poisoning
        .expect_err("an open triangle bounds no volume; mesh_to_volume must report a failure");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("gmshModelMeshGenerate"),
        "this test's premise is that the fixture fails AT THE MESHER, not at an \
         earlier preflight guard; got: {msg}"
    );

    let recovered = kernel
        .mesh_to_volume(&unit_cube_mesh(), &options, ElementOrderTag::P1)
        .expect(
            "a closed unit cube must still mesh after an unrelated meshing failure — \
             the failed call must not leave the process-global mesher unusable",
        );
    let tets = recovered
        .tet_indices()
        .expect("a P1 volume mesh must carry tet_indices");
    assert!(
        !tets.is_empty(),
        "the cube meshed to ZERO tets after an earlier meshing failure: the mesher is \
         still poisoned, and the zero-tet result reached the caller as a silent Ok",
    );
}

/// `mesh_to_volume` never hands back an `Ok` `VolumeMesh` with no tetrahedra.
///
/// That is the invariant a caller actually needs, so it is what this test
/// states — not whichever of the two remedies happens to reach it first. A
/// loud `Err` satisfies the contract just as well as a real mesh does; the
/// only forbidden outcome is a silent empty answer.
///
/// It is driven through a SIBLING mesher because `mesh_to_volume` can no
/// longer poison itself: `refine_volume_with_size_field` reaches the same
/// process-global `gmshModelMeshGenerate` and (today) does not yet route
/// through the recovery wrapper, so it is the remaining way in.
#[test]
fn mesh_to_volume_never_returns_ok_with_zero_tets() {
    let poisoning = refine_volume_with_size_field(
        &unmeshable_open_triangle(),
        &[0.5, 0.5, 0.5],
        &MeshingOptions::default(),
        ElementOrderTag::P1,
    );
    let err = poisoning.expect_err(
        "an open triangle bounds no volume; refine_volume_with_size_field must report a failure",
    );
    let msg = format!("{err:?}");
    assert!(
        msg.contains("gmshModelMeshGenerate"),
        "this test's premise is that the sibling fails AT THE MESHER, not at an \
         earlier preflight guard; got: {msg}"
    );

    let result = GmshKernel::new().mesh_to_volume(
        &unit_cube_mesh(),
        &MeshingOptions::default(),
        ElementOrderTag::P1,
    );
    // An `Err` is deliberately NOT asserted against: a loud failure satisfies
    // this contract just as well, because the caller learns something went
    // wrong instead of acting on an empty mesh. Only the `Ok` case can breach it.
    if let Ok(vm) = result {
        assert!(
            vm.tet_indices().is_some_and(|tets| !tets.is_empty()),
            "mesh_to_volume returned Ok with ZERO tetrahedra — a silent wrong answer. \
             A caller cannot distinguish it from a real mesh; failing loudly is the \
             only acceptable alternative to meshing successfully",
        );
    }
}
