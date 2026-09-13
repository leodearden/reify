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
        msg.contains("gmshModelMeshGenerate: ierr="),
        "this test's premise is that the fixture fails AT THE MESHER itself. The \
         `ierr=` is load-bearing: `ffi.rs` formats a real FFI failure as \
         `<symbol>: ierr=<n> (<msg>)`, whereas the zero-tet backstop in \
         `kernel_real.rs` opens `gmshModelMeshGenerate reported success but ...`. \
         Matching the bare symbol would let this test pass having poisoned \
         nothing and exercised no recovery. Got: {msg}"
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
/// It is driven through `refine_volume_with_size_field` as a REPRESENTATIVE
/// second entry point into the same process-global mesher — not because that
/// sibling is unprotected; it routes through the recovery wrapper too. The
/// failing call and the call under test are deliberately different entry
/// points, since a caller's exposure to someone else's failed mesh is the
/// only way this contract can be breached in practice.
///
/// `a_failed_sibling_mesher_leaves_mesh_to_volume_usable` below drives the
/// identical sequence and demands the stronger outcome. Both are kept: when
/// they disagree — strong red, this one green — the process degraded to
/// failing LOUDLY, which is a materially different regression from returning
/// a silent empty mesh, and worth being able to tell apart at a glance.
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
        msg.contains("gmshModelMeshGenerate: ierr="),
        "this test's premise is that the sibling fails AT THE MESHER itself — the \
         `ierr=` distinguishes a real FFI failure from the zero-tet backstop, \
         whose message also names that symbol; got: {msg}"
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

/// A failed SIBLING mesher must leave `mesh_to_volume` fully usable — not
/// merely loud.
///
/// The strong form of `mesh_to_volume_never_returns_ok_with_zero_tets`
/// above: that test accepts an `Err` as satisfying its contract, because a
/// loud failure is at least honest. This one requires a real mesh, which is
/// what a caller who did nothing wrong is entitled to. The four meshers in
/// this crate share ONE process-global gmsh mesher, so a failure in one that
/// degrades another is a cross-mesher defect, not a local one.
#[test]
fn a_failed_sibling_mesher_leaves_mesh_to_volume_usable() {
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
        msg.contains("gmshModelMeshGenerate: ierr="),
        "this test's premise is that the sibling fails AT THE MESHER itself — the \
         `ierr=` distinguishes a real FFI failure from the zero-tet backstop, \
         whose message also names that symbol; got: {msg}"
    );

    let recovered = GmshKernel::new()
        .mesh_to_volume(
            &unit_cube_mesh(),
            &MeshingOptions::default(),
            ElementOrderTag::P1,
        )
        .expect(
            "a closed unit cube must still mesh after a SIBLING mesher failed — the \
             meshers share one process-global gmsh mesher, so a sibling's failure \
             must not reach this caller at all",
        );
    let tets = recovered
        .tet_indices()
        .expect("a P1 volume mesh must carry tet_indices");
    assert!(
        !tets.is_empty(),
        "the cube meshed to ZERO tets after a SIBLING mesher failed: the sibling's \
         failure left the shared mesher unusable",
    );
}

/// …and the reverse direction: a failed `mesh_to_volume` must leave the
/// sibling meshers usable.
///
/// This is the test that stops the fix from being one-way. Measured before
/// this task, `refine_volume_with_size_field` returned `Ok` with zero tets
/// after an unrelated `mesh_to_volume` failure — the same silent wrong
/// answer, reached through a different door.
///
/// What makes it green is the recovery wrapper on the POISONING call, in
/// `kernel_real.rs`: recovery runs inside the call that failed, while
/// `GMSH_LOCK` is still held, so the sibling never sees the damaged library
/// and its own wrapper never runs here. Delete the wrapping in
/// `kernel_real.rs` and this test reds.
#[test]
fn a_failed_mesh_to_volume_leaves_the_sibling_meshers_usable() {
    let poisoning = GmshKernel::new().mesh_to_volume(
        &unmeshable_open_triangle(),
        &MeshingOptions::default(),
        ElementOrderTag::P1,
    );
    poisoning.expect_err("an open triangle bounds no volume; mesh_to_volume must report a failure");

    // The unit cube has 8 vertices, and this entry point requires one size
    // hint per surface vertex.
    let recovered = refine_volume_with_size_field(
        &unit_cube_mesh(),
        &[0.5; 8],
        &MeshingOptions::default(),
        ElementOrderTag::P1,
    )
    .expect(
        "a closed unit cube must still refine after mesh_to_volume failed — the \
         meshers share one process-global gmsh mesher, so a failure in either \
         direction is a cross-mesher defect",
    );
    let tets = recovered
        .tet_indices()
        .expect("a P1 volume mesh must carry tet_indices");
    assert!(
        !tets.is_empty(),
        "the cube refined to ZERO tets after mesh_to_volume failed: the poisoning \
         call left the shared mesher unusable for this sibling",
    );
}
