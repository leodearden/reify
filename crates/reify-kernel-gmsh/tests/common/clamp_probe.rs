//! Shared machinery for this crate's process-global mesh-size guards:
//! `tests/refine_volume_tests.rs` (the CONSUMER end, task #6211),
//! `tests/mesh_size_option_hermeticity.rs` (the PRODUCER end and, since task
//! #6968, the both-orders acceptance surface), `tests/mesh_plane_2d_tests.rs`
//! and `tests/mesher_poison_recovery.rs`.
//!
//! # Why it is shared
//!
//! The guards must stay in separate binaries — each needs a size-table
//! measurement no sibling suite can perturb, and a `tests/*.rs` file is its own
//! process — but they need the SAME instrument to measure with. Written
//! independently they carried near-verbatim copies of the serialising mutex,
//! the probe outline, the defaults pair, the clamp writer and the probe itself.
//! The cost of that is drift, and drift here is silent rather than loud: a
//! probe outline or a defaults pair corrected in one copy and not the other
//! leaves one of the two guards measuring something weaker than it claims
//! instead of failing. That is the same argument the production side uses for
//! sharing `mesh_size_scope::MeshSizeScope` between its consumers.
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

use reify_kernel_gmsh::mesh_size_scope::{
    GMSH_MESH_SIZE_MAX_DEFAULT, GMSH_MESH_SIZE_MIN_DEFAULT, GMSH_SIZE_OPTION_DEFAULTS,
};
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

/// Put every process-global gmsh size option at its documented default.
///
/// Driven by the production [`GMSH_SIZE_OPTION_DEFAULTS`] rather than by a
/// local list, so a sixth option added to the seam is established here too with
/// no test edit — and so this helper cannot drift into establishing a table the
/// production code no longer considers "default".
///
/// This is the explicit form of what [`probe_triangle_count`] used to do
/// implicitly, for the callers that genuinely need a known starting table
/// rather than a measurement of whatever the process is carrying.
pub fn set_all_size_options_to_defaults() {
    let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init::ensure_initialized();
    for (option, default) in GMSH_SIZE_OPTION_DEFAULTS {
        ffi::option_set_number(option, default)
            .unwrap_or_else(|e| panic!("set {option} to its gmsh default: {e:?}"));
    }
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
/// prescribed size here" — so this call writes no size option and reports
/// whatever the process-global table is carrying.
///
/// # Why it pins nothing itself
///
/// It used to write the `MeshSizeFromPoints` / `FromCurvature` /
/// `ExtendFromBoundary` trio to gmsh's defaults before every measurement, so
/// its reading was "a function of the mesh-size CLAMP alone". That was right
/// while #6212's trio leak was live and unowned, and it is wrong now: task
/// #6968 closed that leak, and the pinning would MASK it — a leaked
/// `ExtendFromBoundary = 0` was overwritten before the probe ever measured it,
/// so an unfixed build read 162 and passed. Measured: with the pinning in
/// place, `mesh_size_option_hermeticity.rs`'s both-orders test is green against
/// a `refine_volume` whose `MeshSizeScope` is commented out.
///
/// Removing it makes this probe a detector of a size-option leak by ANY route,
/// which is what a hermeticity instrument should be. Callers that need a known
/// starting table now say so explicitly, via
/// [`set_all_size_options_to_defaults`].
///
/// Observing the leak's EFFECT is still worth having alongside the direct table
/// reads that `ffi::option_get_number` (#6968) now makes possible: a density
/// probe fails on a leak by any route, not only via an option name a test
/// thought to read.
///
/// Measured sensitivity on this unit square, against a baseline of 162
/// triangles from a defaults table: `Mesh.MeshSizeMax = 0.05` -> 944,
/// `MeshSizeFromPoints = 0` -> 4, `MeshSizeExtendFromBoundary = 0` -> 48. A
/// `MeshSizeMin` below the natural element size and any `MeshSizeFromCurvature`
/// are INERT here — a floor nothing reaches, and a flat straight-edged square
/// has no curvature to sample — so this instrument does not detect those two.
/// The per-entry-point table reads do.
pub fn probe_triangle_count() -> usize {
    mesh_plane_2d(&PROBE_OUTER, &[], None, false, true)
        .expect("mesh_plane_2d must succeed for a unit square")
        .triangle_indices
        .len()
        / 3
}
