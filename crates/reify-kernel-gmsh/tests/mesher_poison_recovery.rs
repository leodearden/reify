//! The one place that deliberately drives gmsh's mesher into failure.
//!
//! Gmsh's mesher is process-global, and a failed `gmshModelMeshGenerate`
//! used to leave it unusable for the rest of the process: every later
//! generate returned `ierr=0` with no elements instead of an error, so the
//! next caller got a silent, plausible-looking `Ok` holding zero
//! tetrahedra. `gmshClear()` did not lift that; a
//! `gmshFinalize`+`gmshInitialize` cycle does.
//!
//! Every test here therefore fails a mesh on purpose. Most then assert the
//! process is still usable — that the damage is confined to the call that
//! earned it. The `*_reports_gmshs_captured_log_*` tests assert what that
//! failing call REPORTS, which needs the same deliberate failure to observe.
//! The failing call itself is expected to be loud; what is under test is the
//! state it leaves behind and the diagnosis it hands back.
//!
//! Three of this crate's four `mesh_generate` sites are driven from here:
//! `mesh_to_volume` and `refine_volume_with_size_field`, in both directions,
//! and `mesh_surface_to_volume_with_attribution`, via two disjoint closed
//! cubes that pass its watertight preflight. Only `mesh_profile_2d.rs`'s
//! `mesh_generate(2)` stays uncovered at the mesher, for a measured reason:
//! degenerate outlines make it succeed with zero elements (rejected at the
//! readback, and tested in `mesh_plane_2d_tests.rs`), and a NaN vertex fails
//! it only after ~98 s.
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
// `tests/refine_volume_tests.rs` and `tests/mesh_size_option_hermeticity.rs`.
// Declared by `#[path]`; see `common/clamp_probe.rs` for why, and why one copy
// matters.
#[path = "common/clamp_probe.rs"]
mod clamp_probe;

// The uniform size field the two refine calls below remesh under, shared by
// `#[path]` with `tests/mesh_size_option_hermeticity.rs`.
#[path = "common/size_field.rs"]
mod size_field;

use clamp_probe::{CLAMP_TEST_ORDER, probe_triangle_count};
use reify_ir::{ElementOrderTag, GeometryError, Mesh};
#[cfg(feature = "mesh-morph")]
use reify_kernel_gmsh::{EntityAttribution, mesh_surface_to_volume_with_attribution};
use reify_kernel_gmsh::{GmshKernel, MeshingOptions, ffi, init, refine_volume_with_size_field};
use reify_test_support::mesh_fixtures::unit_cube_mesh;
use size_field::uniform_unit_cube_size_field;

/// A single open triangle: a surface gmsh accepts and classifies happily but
/// that HXT cannot 3D-mesh, because it bounds no closed region.
///
/// This is the cheapest known input that reaches `gmshModelMeshGenerate(3)`
/// and fails there — the precise failure this binary needs. Not hoisted into
/// `reify_test_support::mesh_fixtures`: it has exactly one consumer, and
/// `tests/common/mod.rs` states in its own header that no new shared fixture
/// belongs there.
fn unmeshable_open_triangle() -> Mesh {
    Mesh {
        vertices: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        indices: vec![0, 1, 2],
        normals: None,
    }
}

/// Two disjoint closed unit cubes, the second shifted +3 in x.
///
/// Both shells are closed, consistently wound and welded, so the attributed
/// producer's watertight preflight (#4876) accepts them and the call reaches
/// gmsh. Gmsh's single surface loop over two shells then fails HXT. MEASURED
/// (libgmsh 4.15.2): `Info: some volumes of the BRep were not found`, then
/// `Error: HXT 3D mesh failed`, in ~50 ms at [`poison_via_attributed`]'s
/// options. Not hoisted into `reify_test_support::mesh_fixtures`, for the same
/// reason as [`unmeshable_open_triangle`]: it has one consumer.
#[cfg(feature = "mesh-morph")]
fn two_disjoint_unit_cubes() -> Mesh {
    const OFFSET_X: f32 = 3.0;
    let cube = unit_cube_mesh();
    let cube_vertex_count =
        u32::try_from(cube.vertices.len() / 3).expect("a unit cube's vertex count fits in u32");

    let mut vertices = cube.vertices.clone();
    vertices.extend(
        cube.vertices
            .chunks_exact(3)
            .flat_map(|v| [v[0] + OFFSET_X, v[1], v[2]]),
    );
    let mut indices = cube.indices.clone();
    indices.extend(cube.indices.iter().map(|&i| i + cube_vertex_count));

    Mesh {
        vertices,
        indices,
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
///
/// Returns the error it checked, so a caller with more to say about the
/// message — the captured-log tests at the foot of this file — states this
/// premise by reusing it rather than by re-deriving a weaker copy.
#[track_caller]
fn assert_failed_at_the_mesher<T>(
    entry_point: &str,
    result: Result<T, GeometryError>,
) -> GeometryError {
    let Err(err) = result else {
        panic!("{entry_point}: this fixture cannot be tet-meshed — it must report a failure");
    };
    let msg = format!("{err:?}");
    assert!(
        msg.contains("gmshModelMeshGenerate: ierr="),
        "{entry_point} was expected to fail at the mesher itself, leaving the \
         process-global mesher damaged; it failed somewhere earlier instead, so \
         this test would prove nothing about recovery. Got: {msg}"
    );
    err
}

/// Options asking gmsh for more worker threads than either refine fixture here
/// has classified curves: the open triangle classifies to 3, the cube to 14.
/// gmsh 4.15.2 hangs once threads exceed the curve count unless refine
/// pre-builds its view octree, so a literal keeps these guards from going
/// vacuous on a small host; see
/// `docs/notes/gmsh-postview-background-field-threading.md`.
fn many_threads() -> MeshingOptions {
    MeshingOptions {
        threads: Some(32),
        deterministic: false,
        ..MeshingOptions::default()
    }
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
/// The size field is a sizing MESH in its own right, independent of the
/// surface being remeshed, so it is built over the unit cube rather than over
/// the open triangle: a degenerate field would be rejected at construction and
/// the failure would land before gmsh, proving nothing about recovery.
/// `assert_failed_at_the_mesher` is what holds that line.
fn poison_via_refine() -> GeometryError {
    assert_failed_at_the_mesher(
        "refine_volume_with_size_field",
        refine_volume_with_size_field(
            &unmeshable_open_triangle(),
            &uniform_unit_cube_size_field(0.5),
            &many_threads(),
            ElementOrderTag::P1,
        ),
    )
}

/// Poison the shared mesher through `mesh_surface_to_volume_with_attribution`.
///
/// `mesh_size` and `deterministic` are both pinned because they set what the
/// failure costs: measured 4-6 s per failure under `MeshingOptions::default()`,
/// against ~50 ms here. The empty attribution matches nothing, which costs
/// nothing either: the call fails before any matching runs.
#[cfg(feature = "mesh-morph")]
fn poison_via_attributed() -> GeometryError {
    assert_failed_at_the_mesher(
        "mesh_surface_to_volume_with_attribution",
        mesh_surface_to_volume_with_attribution(
            &two_disjoint_unit_cubes(),
            &MeshingOptions {
                mesh_size: Some(0.5),
                deterministic: true,
                ..MeshingOptions::default()
            },
            ElementOrderTag::P1,
            None,
            None,
            None,
            &EntityAttribution {
                faces: vec![],
                edges: vec![],
                vertices: vec![],
                match_tolerance: 0.0,
            },
        ),
    )
}

/// Require `err` — a failure [`assert_failed_at_the_mesher`] already placed at
/// the mesher — to carry gmsh's captured log on top of the last-error line,
/// folded in exactly once.
///
/// One copy for every mesher-failure test in this binary; why each assertion
/// is there is set out on
/// [`a_failed_mesh_to_volume_reports_gmshs_captured_log_not_just_the_last_error`].
/// Display, not Debug: this is the form that reaches a log or the GUI.
#[track_caller]
fn assert_carries_gmshs_captured_log_once(entry_point: &str, err: &GeometryError) {
    let msg = format!("{err}");
    assert!(
        msg.contains("gmshModelMeshGenerate") && msg.contains("HXT 3D mesh failed"),
        "{entry_point}: the pre-existing last-error annotation must be preserved, not \
         replaced; got: {msg}",
    );
    assert!(
        msg.contains("gmsh log ("),
        "{entry_point}: expected the captured-log header; got: {msg}",
    );
    assert!(
        msg.contains("Info:"),
        "{entry_point}: expected a captured Info line — gmshLoggerGetLastError can never \
         supply one; got: {msg}",
    );
    assert_eq!(
        msg.matches("gmsh log (").count(),
        1,
        "{entry_point}: the mesher failure must be annotated exactly ONCE — \
         init::mesh_generate_with_recovery folds the capture in itself, so a \
         LogCapture seam drawn over that call would append the same tail a \
         second time; got: {msg}",
    );
}

/// Require gmsh's capture to be stopped and drained after `entry_point` failed
/// at the mesher — the exit shape where recovery recycled the library holding
/// the buffer while the capture was armed.
///
/// Call it after the entry point has returned: it takes `GMSH_LOCK` itself, so
/// the read is serialised against any concurrent mesher rather than racing one
/// mid-flight, and the caller's `CLAMP_TEST_ORDER` keeps this binary's siblings
/// out of the window. Mirrors
/// `mesh_to_volume_tests::mesh_to_volume_leaves_the_gmsh_logger_stopped`.
#[track_caller]
fn assert_capture_left_stopped_and_drained(entry_point: &str) {
    let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let leftover = ffi::logger_get().expect("ffi::logger_get failed");
    assert!(
        leftover.is_empty(),
        "a {entry_point} that failed AT THE MESHER must still leave gmsh's capture \
         stopped and drained, even though recovery recycled the library holding the \
         buffer mid-window; {} lines left: {leftover:?}",
        leftover.len(),
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

    let recovered = refine_volume_with_size_field(
        &unit_cube_mesh(),
        &uniform_unit_cube_size_field(0.5),
        &many_threads(),
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

/// What [`probe_triangle_count`] reads from a gmsh sitting at its own default
/// options — the state a `gmshFinalize`+`gmshInitialize` recycle leaves behind.
///
/// Measured in this binary. The same probe reads 242 under a leaked
/// `[0.1, 0.1]` clamp, the pair `tests/mesh_size_option_hermeticity.rs`
/// records, so the equality below has an 80-triangle margin rather than a
/// rounding one.
const PROBE_TRIANGLES_AT_DEFAULT_OPTIONS: usize = 162;

/// A `mesh_to_volume` that fails AT THE MESHER must leave gmsh's process-global
/// options at their defaults, exactly as a successful one does.
///
/// `tests/mesh_size_option_hermeticity.rs` pins that for the SUCCESS path
/// (#6298). The failure path reaches the same end by a different route and was
/// unpinned: `mesh_to_volume` writes `Mesh.MeshSizeMin`/`MeshSizeMax` BEFORE it
/// reaches `mesh_generate`, so a call that fails there has already poisoned the
/// table, and what clears it is recovery's `gmshFinalize`+`gmshInitialize`.
///
/// The assertion is against a RECORDED constant, not against a baseline probe,
/// and that is what makes it discriminating. Recovery resets the whole option
/// table, so any baseline measured after an identical recycle moves with a
/// regression instead of catching it — the two would compare equal however
/// badly the failure path behaved. Measured absolutely, a recovery that stops
/// re-initializing reds here by one of two routes: the failing call's own 0.1
/// clamp survives into the probe, or the probe's `mesh_generate(2)` meets the
/// still-poisoned mesher and fails outright.
#[test]
fn a_failed_mesh_to_volume_leaves_the_default_clamp_behind() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());

    // 0.1 is a size the probe can see, and `mesh_to_volume` writes it into the
    // process-global table before it ever reaches the mesher.
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
        after, PROBE_TRIANGLES_AT_DEFAULT_OPTIONS,
        "a FAILED mesh_to_volume did not leave gmsh at its default options: a later \
         defaults-relying call meshed to {after} triangles where a just-recycled \
         gmsh gives {PROBE_TRIANGLES_AT_DEFAULT_OPTIONS} (242 is this probe's \
         reading under a leaked 0.1 clamp)",
    );
}

/// A `mesh_to_volume` that fails at the mesher must report gmsh's OWN
/// diagnosis, not only the single line `gmshLoggerGetLastError` holds.
///
/// The `Info:` assertion is the decisive one, and the reason this test is
/// worth its runtime: `gmshLoggerGetLastError` only ever holds the last
/// ERROR, so an `Info:` line in the message can have reached it only through
/// gmsh's capture buffer. Measured on this fixture, that buffer is where the
/// actual explanation lives — `Info: all vertices are coplanar or nearly
/// coplanar` — while the last error says no more than `HXT 3D mesh failed`.
///
/// It belongs in THIS binary rather than beside `log_capture`'s formatter
/// cases: it needs a real mesher failure, which is what this binary exists to
/// provoke, and `assert_failed_at_the_mesher` is what establishes that the
/// failure landed there and so that there is a real capture to read.
///
/// The assertions stay on stable gmsh phrases and deliberately do not pin the
/// captured line COUNT, which is version- and thread-sensitive.
///
/// Two further properties ride on this one failure, because provoking it is
/// the expensive part and both are invisible anywhere else:
///
/// The tail is folded in exactly ONCE. `init::mesh_generate_with_recovery`
/// annotates its own failure — it has to, since it destroys the library
/// holding the capture — so every mesher deliberately leaves that one call
/// outside its `LogCapture` seams. Only a comment marks that exclusion at
/// each call site, and a later edit extending a seam over it "for symmetry"
/// with its neighbours would fail SILENTLY: every mesher-failure message
/// would carry the same ~40 lines twice. The count assertion in
/// [`assert_carries_gmshs_captured_log_once`], shared by every mesher-failure
/// test here, is what reds instead.
///
/// The capture is still stopped and drained afterwards. This is the third
/// and riskiest of the guard's exit shapes:
/// `mesh_to_volume_tests::mesh_to_volume_leaves_the_gmsh_logger_stopped`
/// covers success, `log_capture_tests`'s guard test covers an error that
/// leaves libgmsh standing, and only this path RECYCLES the library
/// mid-window — `LogCapture::drop` therefore calls `logger_stop` on a
/// library that is not the one `logger_start` ran against. Leaving the
/// capture armed there is exactly the contamination `log_capture`'s module
/// doc names: the next unrelated failure would report these lines as its own.
#[test]
fn a_failed_mesh_to_volume_reports_gmshs_captured_log_not_just_the_last_error() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());

    let err = assert_failed_at_the_mesher(
        "mesh_to_volume",
        GmshKernel::new().mesh_to_volume(
            &unmeshable_open_triangle(),
            &MeshingOptions::default(),
            ElementOrderTag::P1,
        ),
    );

    assert_carries_gmshs_captured_log_once("mesh_to_volume", &err);
    assert_capture_left_stopped_and_drained("mesh_to_volume");
}

/// A `refine_volume_with_size_field` that fails at the mesher must report
/// gmsh's own diagnosis too, folded in exactly once, and leave the capture
/// stopped and drained.
///
/// The exactly-once assertion is what stops a `LogCapture` seam being drawn
/// over refine's own `init::mesh_generate_with_recovery` call. The drained
/// check carries more weight here than at `mesh_to_volume`: after recovery
/// recycles the library, refine's `BackgroundFieldGuard::drop` tears its field
/// down against the NEW library and logs errors into the still-armed capture
/// (measured: `Error: Cannot delete field id 1, it does not exist`), so those
/// lines too must be drained before the call returns.
#[test]
fn a_failed_refine_reports_gmshs_captured_log_not_just_the_last_error() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());

    let err = poison_via_refine();

    assert_carries_gmshs_captured_log_once("refine_volume_with_size_field", &err);
    assert_capture_left_stopped_and_drained("refine_volume_with_size_field");
}

/// A `mesh_surface_to_volume_with_attribution` that fails at the mesher must
/// report gmsh's own diagnosis too, folded in exactly once, and leave the
/// capture stopped and drained.
///
/// Measured on [`two_disjoint_unit_cubes`], the capture is where the
/// explanation lives — `Info: some volumes of the BRep were not found` — while
/// the last error says only `HXT 3D mesh failed`.
#[cfg(feature = "mesh-morph")]
#[test]
fn a_failed_attributed_mesh_reports_gmshs_captured_log_not_just_the_last_error() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());

    let err = poison_via_attributed();

    assert_carries_gmshs_captured_log_once("mesh_surface_to_volume_with_attribution", &err);
    assert_capture_left_stopped_and_drained("mesh_surface_to_volume_with_attribution");
}

/// A failed attributed mesh must leave `mesh_to_volume` fully usable — the
/// recovery half of the attributed producer's `mesh_generate` site.
///
/// Validated by mutation: with that site's `init::mesh_generate_with_recovery`
/// replaced by a bare `ffi::mesh_generate(3)`, this test reds.
#[cfg(feature = "mesh-morph")]
#[test]
fn a_failed_attributed_mesh_leaves_mesh_to_volume_usable() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());

    poison_via_attributed();
    assert_cube_still_meshes("a failed mesh_surface_to_volume_with_attribution");
}
