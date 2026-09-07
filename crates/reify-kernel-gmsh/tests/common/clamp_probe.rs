//! Shared machinery for this crate's two process-global mesh-size-clamp
//! guards: `tests/refine_volume_tests.rs` (the CONSUMER end, task #6211) and
//! `tests/mesh_to_volume_clamp_hermeticity.rs` (the PRODUCER end, task #6298).
//!
//! # Why it is shared
//!
//! The two guards must stay in separate binaries — each needs a clamp
//! measurement no sibling suite can perturb, and a `tests/*.rs` file is its own
//! process — but they need the SAME instrument to measure with. Written
//! independently they carried near-verbatim copies of the serialising mutex,
//! the probe outline, the defaults pair, the clamp writer and the probe itself.
//! The cost of that is drift, and drift here is silent rather than loud: a
//! probe outline or a defaults pair corrected in one copy and not the other
//! leaves one of the two guards measuring something weaker than it claims
//! instead of failing. That is the same argument the production side uses for
//! sharing `mesh_size_clamp::MeshSizeClampReset` between its two consumers.
//!
//! Declared by `#[path]` from each binary rather than as a submodule of
//! `common/mod.rs`, whose stated scope is the #6200 box/cylinder geometry
//! fixtures; either way there is exactly one copy of the source.
//!
//! Every consumer is `#[allow(dead_code)]`-tolerant by construction: each test
//! binary compiles its own copy of this module and uses only part of it, so the
//! unused remainder must not be an error under `-D warnings`. Each binary also
//! gets its own `CLAMP_TEST_ORDER` instance, which is exactly right — the
//! mutex serialises threads within one process, and cross-process isolation is
//! what the separate binaries already provide.

#![allow(dead_code)]

use std::sync::Mutex;

use reify_kernel_gmsh::mesh_size_clamp::{GMSH_MESH_SIZE_MAX_DEFAULT, GMSH_MESH_SIZE_MIN_DEFAULT};
use reify_kernel_gmsh::{ffi, init, mesh_plane_2d};

/// Whole-test-body serialisation, layered *above* `init::GMSH_LOCK`.
///
/// Every test that uses this module manipulates the process-global gmsh
/// mesh-size clamp across MULTIPLE lock acquisitions — poison or default the
/// clamp, then call the entry point under test (each of which takes
/// `GMSH_LOCK` itself), then re-measure. `GMSH_LOCK` is released between those
/// steps, so cargo's parallel test threads can interleave inside the gap.
///
/// That interleave cannot produce a false FAILURE, only a false PASS, which is
/// the worse direction for a regression guard: once the fix restores the clamp
/// to gmsh's defaults on every exit, a sibling landing in the gap *erases* the
/// state under measurement and the two legs compare equal for the wrong
/// reason.
///
/// Taking this mutex as the first statement of every test that touches gmsh
/// makes each set → perturb → measure sequence atomic with respect to its
/// siblings. `GMSH_LOCK` is strictly finer-grained (always acquired while this
/// one is held, never the reverse), so the nesting order is fixed and adds no
/// deadlock risk. Poison recovery matches the crate convention at
/// `mesh_profile_2d.rs`: a panicking test must not cascade into "lock
/// poisoned" failures for every sibling.
pub static CLAMP_TEST_ORDER: Mutex<()> = Mutex::new(());

/// Unit square in the XY plane — the defaults-relying 2D probe's outline.
const PROBE_OUTER: [[f64; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// Gmsh's documented defaults for the `Mesh.MeshSizeMin`/`MeshSizeMax` pair —
/// no floor, effectively no cap. This is the state a caller that writes no
/// clamp of its own (e.g. `mesh_plane_2d` with no requested size) expects.
///
/// Built from the production constants rather than from literals so the two
/// cannot drift: if the production notion of gmsh's defaults is ever corrected,
/// a private copy here would keep asserting against the stale pair and every
/// "from gmsh's defaults" baseline built on it would quietly stop being from
/// gmsh's defaults — weakening the assertions instead of failing them.
///
/// Both values verified against the shipped library rather than assumed:
/// `gmsh 4.15.2 -parse_and_exit` on a `.geo` of
/// `Printf("%g", Mesh.MeshSizeMin)` / `Printf("%g", Mesh.MeshSizeMax)` prints
/// `0` and `1e+22`.
pub const GMSH_CLAMP_DEFAULTS: (f64, f64) =
    (GMSH_MESH_SIZE_MIN_DEFAULT, GMSH_MESH_SIZE_MAX_DEFAULT);

/// Gmsh's documented defaults for the three size-SOURCE options — the
/// `MeshSizeFromPoints` / `MeshSizeFromCurvature` / `MeshSizeExtendFromBoundary`
/// trio that decides *where* element sizes come from, as distinct from the
/// `MeshSizeMin`/`MeshSizeMax` pair that clamps them.
///
/// Measured, not assumed: `gmsh 4.15.2 -parse_and_exit` on a `.geo` of
/// `Printf("%g", Mesh.MeshSizeFromPoints)` and friends prints `1`, `0`, `1`.
///
/// Local to this module on purpose. It exists only to make
/// [`probe_triangle_count`] hermetic; it is NOT a production restore list and
/// does not close task #6212, which owns extending the real
/// `mesh_size_clamp` seam to these three across every entry point that writes
/// them.
const GMSH_SIZE_SOURCE_DEFAULTS: [(&str, f64); 3] = [
    ("Mesh.MeshSizeFromPoints", 1.0),
    ("Mesh.MeshSizeFromCurvature", 0.0),
    ("Mesh.MeshSizeExtendFromBoundary", 1.0),
];

/// Write the process-global gmsh mesh-size clamp.
///
/// gmsh's option table is process-global and is **not** reset by `gmshClear()`,
/// so `Mesh.MeshSizeMin` / `Mesh.MeshSizeMax` written by one call survive into
/// every later call in the same process. Acquires `GMSH_LOCK` for the duration
/// of the two writes and releases it before returning, so the subsequent
/// measuring call can take the lock itself.
pub fn set_global_mesh_size_clamp((min, max): (f64, f64)) {
    let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init::ensure_initialized();
    ffi::option_set_number("Mesh.MeshSizeMin", min).expect("set MeshSizeMin");
    ffi::option_set_number("Mesh.MeshSizeMax", max).expect("set MeshSizeMax");
}

/// Pin the clamp shut at `size`, reproducing the state a sibling entry point
/// leaves behind (`Min == Max == its own requested size`).
pub fn poison_global_mesh_size_clamp(size: f64) {
    set_global_mesh_size_clamp((size, size));
}

/// Triangle count of the defaults-relying 2D probe — the instrument both clamp
/// guards measure the process-global `Mesh.MeshSizeMax` with.
///
/// `mesh_size: None` is the whole point: `mesh_plane_2d` puts its
/// `Mesh.MeshSizeMin/Max` writes behind `if let Some(s) = mesh_size && s > 0.0`
/// (`mesh_profile_2d.rs`), and `geo_add_point` passes meshSize `0.0` — "no
/// prescribed size here" — so this call writes no clamp and reports whatever
/// `Mesh.MeshSizeMax` the process happens to be carrying. Observing the leak's
/// EFFECT rather than reading the option table back is forced: this crate's FFI
/// surface exposes `option_set_number` but no `option_get_number`. It is also
/// the stronger guard — it fails if the clamp leaks by ANY route, not only via
/// the one option name a test thought to read.
///
/// # Why it pins the size-SOURCE trio first
///
/// `refine_volume_with_size_field` writes `Mesh.MeshSizeFromPoints = 1`,
/// `MeshSizeFromCurvature = 0` and `MeshSizeExtendFromBoundary = 0`
/// (`refine_volume.rs`) and deliberately does NOT restore them — task #6212's
/// still-open leak. Any binary that runs a refine in one test and this probe in
/// another therefore measures under a different option table depending on which
/// test won cargo's thread race, and `CLAMP_TEST_ORDER` does not help: it
/// serialises the bodies but restores nothing. Observed, not theoretical — with
/// #6298's fix reverted, `mesh_to_volume_clamp_hermeticity.rs` measured
/// baseline 48 / after 246 in one interleaving and 162 / 242 in another.
///
/// Writing the trio to gmsh's own defaults here makes this probe's reading a
/// function of the mesh-size CLAMP alone, which is the one thing both guards
/// assert about — so their recorded numbers are reproducible and their
/// sensitivity stops being an order-dependent property of a leak another task
/// owns. This changes nothing outside the probe: #6212's leak is still live for
/// every real caller, and closing it is still #6212's job.
pub fn probe_triangle_count() -> usize {
    {
        let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        init::ensure_initialized();
        for (option, default) in GMSH_SIZE_SOURCE_DEFAULTS {
            ffi::option_set_number(option, default)
                .unwrap_or_else(|e| panic!("set {option} to its gmsh default: {e:?}"));
        }
    }
    mesh_plane_2d(&PROBE_OUTER, &[], None, false, true)
        .expect("mesh_plane_2d must succeed for a unit square")
        .triangle_indices
        .len()
        / 3
}
