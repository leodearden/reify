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
//! Two of this crate's four `mesh_generate` sites are driven from here —
//! `mesh_to_volume` and `refine_volume_with_size_field`, in both directions.
//! The other two are uncovered for a measured reason stated at each site:
//! `mesh_profile_2d.rs` (no cheap 2D geometry that fails `mesh_generate(2)`
//! was identified) and `mesh_boundary.rs` (its watertight preflight rejects
//! every unmeshable fixture this binary has before gmsh is reached).
//!
//! These tests live in their own binary rather than in
//! `mesh_to_volume_tests.rs` so that a recovery regression reds one binary
//! whose name states the cause, instead of reddening unrelated assertions
//! spread across the crate's other test binaries.
//!
//! Every test body here takes `clamp_probe::CLAMP_TEST_ORDER` first. Each one
//! spans several `init::GMSH_LOCK` acquisitions with process-global gmsh state
//! under observation across the gaps, and cargo runs one binary's tests on
//! parallel threads — so without it a sibling's in-flight mesh-size clamp can
//! land inside another's measurement window.
//!
//! Only compiled / run when `cfg(has_gmsh)` is set by `build.rs`. On stub
//! builds this file is empty and the test binary contains zero tests —
//! preserving the all-OK posture of `cargo test -p reify-kernel-gmsh` on
//! hosts without libgmsh.

#![cfg(has_gmsh)]

// The clamp probe and its serialising mutex, shared verbatim with
// `tests/refine_volume_tests.rs` and `tests/mesh_to_volume_clamp_hermeticity.rs`.
// Declared by path rather than through `common/mod.rs`, whose stated scope is
// the #6200 geometry fixtures; see `common/clamp_probe.rs` for why one copy
// matters.
#[path = "common/clamp_probe.rs"]
mod clamp_probe;

use clamp_probe::{
    CLAMP_TEST_ORDER, GMSH_CLAMP_DEFAULTS, probe_triangle_count, set_global_mesh_size_clamp,
};
use reify_ir::{ElementOrderTag, GeometryError, Mesh};
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

/// The premise every test in this binary rests on: the call failed AT THE
/// MESHER, so there is real damage for the recovery to repair.
///
/// The `ierr=` is load-bearing, which is why it is asserted in ONE place: the
/// FFI layer formats a real failure as `<symbol>: ierr=<n> (<msg>)`, whereas
/// the zero-tet backstop in `init::read_tet_connectivity` opens
/// `gmshModelMeshGenerate reported success but …`. Matching the bare symbol
/// name would let a test pass having poisoned nothing and exercised no
/// recovery — and a per-test copy of this assertion is exactly what drifts
/// into that weaker form.
#[track_caller]
fn assert_failed_at_the_mesher<T>(entry_point: &str, result: Result<T, GeometryError>) {
    let Err(err) = result else {
        panic!("{entry_point}: an open triangle bounds no volume — it must report a failure");
    };
    let msg = format!("{err:?}");
    assert!(
        msg.contains("gmshModelMeshGenerate: ierr="),
        "{entry_point} was expected to fail at the mesher itself, leaving the \
         process-global mesher damaged; it failed somewhere earlier instead, so \
         this test would prove nothing about recovery. Got: {msg}"
    );
}

/// Poison the shared mesher through `GmshKernel::mesh_to_volume`.
fn poison_via_mesh_to_volume() {
    assert_failed_at_the_mesher(
        "mesh_to_volume",
        GmshKernel::new().mesh_to_volume(
            &unmeshable_open_triangle(),
            &MeshingOptions::default(),
            ElementOrderTag::P1,
        ),
    );
}

/// Poison the shared mesher through `refine_volume_with_size_field`.
///
/// One size hint per surface vertex; the open triangle has three.
fn poison_via_refine() {
    assert_failed_at_the_mesher(
        "refine_volume_with_size_field",
        refine_volume_with_size_field(
            &unmeshable_open_triangle(),
            &[0.5, 0.5, 0.5],
            &MeshingOptions::default(),
            ElementOrderTag::P1,
        ),
    );
}

/// Require `mesh_to_volume` to hand back a real tet mesh for a known-good
/// closed cube — what a caller who did nothing wrong is entitled to.
///
/// `after` names the failure being recovered from, so a red run says which
/// poisoning path leaked rather than merely that one did.
///
/// A loud `Err` here is a failure too, and deliberately so: it is a materially
/// milder regression than a silent empty mesh, but a caller whose own input is
/// fine should never see either. The panic message tells the two apart.
///
/// The tet assertion is `> 0`, never an exact count: measured counts for this
/// same cube varied 744/748/752 across runs under multithreaded HXT, a jitter
/// `mesh_to_volume_tests.rs::cuboid_round_trip_within_count_variation_budget`
/// already acknowledges in tree.
#[track_caller]
fn assert_cube_still_meshes(after: &str) {
    let recovered = GmshKernel::new()
        .mesh_to_volume(
            &unit_cube_mesh(),
            &MeshingOptions::default(),
            ElementOrderTag::P1,
        )
        .unwrap_or_else(|e| {
            panic!(
                "a closed unit cube must still mesh after {after} — it failed LOUDLY \
                 instead, so the mesher is still damaged even though nothing silently \
                 wrong reached the caller: {e:?}"
            )
        });
    let tets = recovered
        .tet_indices()
        .expect("a P1 volume mesh must carry tet_indices");
    assert!(
        !tets.is_empty(),
        "the cube meshed to ZERO tets after {after}: the mesher is still poisoned, and \
         the empty result reached the caller as a SILENT Ok it cannot tell from a real \
         mesh",
    );
}

/// A `mesh_to_volume` that fails at the mesher must not take the next
/// caller's mesh down with it.
#[test]
fn a_failed_mesh_to_volume_leaves_the_mesher_usable_for_the_next_caller() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());

    poison_via_mesh_to_volume();
    assert_cube_still_meshes("a failed mesh_to_volume");
}

/// A failed SIBLING mesher must leave `mesh_to_volume` fully usable.
///
/// The four meshers in this crate share ONE process-global gmsh mesher, so a
/// failure in one that degrades another is a cross-mesher defect, not a local
/// one. Measured before this task, this sequence returned an `Ok` cube holding
/// zero tets.
#[test]
fn a_failed_sibling_mesher_leaves_mesh_to_volume_usable() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());

    poison_via_refine();
    assert_cube_still_meshes("a failed refine_volume_with_size_field");
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
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());

    poison_via_mesh_to_volume();

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

/// A `mesh_to_volume` that fails AT THE MESHER must leave gmsh's process-global
/// mesh-size clamp at its defaults, exactly as a successful one does.
///
/// `tests/mesh_to_volume_clamp_hermeticity.rs` pins that for the success path
/// (#6298). The failure path reaches the same end through a different route and
/// was unpinned: `mesh_to_volume` writes `Mesh.MeshSizeMin`/`MeshSizeMax`
/// BEFORE it reaches `mesh_generate`, so a call that fails there has already
/// poisoned the table; recovery's `gmshFinalize`+`gmshInitialize` then resets
/// the whole option table, and `MeshSizeClampReset::drop` writes the defaults
/// into the RE-INITIALIZED library on the way out.
///
/// What this asserts is the observable contract — a later defaults-relying
/// caller meshes as if the failure had never happened — not either mechanism
/// individually. Today the two are redundant, which is the posture this crate
/// wants for its one measured cross-caller leak (#6298 / #6212) and is why the
/// assertion is worth having even though no single deletion reds it: it is
/// future changes to the recovery's option handling that it catches.
///
/// # Why it opens with a throwaway poisoning
///
/// Both probes must read the same option table apart from the clamp. The
/// warm-up ends in a just-recycled gmsh, which is exactly the state the
/// measured poisoning leaves behind, so `General.NumThreads`,
/// `Mesh.Algorithm3D` and `Mesh.ElementOrder` — none of which
/// `probe_triangle_count` pins — are identical across the two measurements and
/// the clamp is the only free variable. Without it the baseline would inherit
/// whichever sibling test won cargo's thread race.
#[test]
fn a_failed_mesh_to_volume_leaves_the_default_clamp_behind() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());

    poison_via_mesh_to_volume();
    set_global_mesh_size_clamp(GMSH_CLAMP_DEFAULTS);
    let baseline = probe_triangle_count();

    // 0.1 is a size this probe can see. Measured in THIS binary's state — a
    // just-recycled gmsh, which is not the state the sibling binary measures
    // from — the probe reads 162 triangles at gmsh's default clamp and 242
    // under a leaked [0.1, 0.1], the same pair
    // `mesh_to_volume_clamp_hermeticity.rs` records. So the equality below has
    // an 80-triangle margin, not a rounding one.
    assert_failed_at_the_mesher(
        "mesh_to_volume",
        GmshKernel::new().mesh_to_volume(
            &unmeshable_open_triangle(),
            &MeshingOptions {
                mesh_size: Some(0.1),
                ..MeshingOptions::default()
            },
            ElementOrderTag::P1,
        ),
    );

    let after = probe_triangle_count();
    assert_eq!(
        after, baseline,
        "a FAILED mesh_to_volume left its own 0.1 mesh-size clamp behind: a later \
         defaults-relying call meshed to {after} triangles where gmsh's defaults \
         give {baseline}",
    );
}
