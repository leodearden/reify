//! Shared machinery for this crate's process-global mesh-size guards:
//! `tests/refine_volume_tests.rs` (the CONSUMER end, task #6211),
//! `tests/mesh_size_option_hermeticity.rs` (the PRODUCER end and, since task
//! #6968, the both-orders acceptance surface), `tests/mesh_plane_2d_tests.rs`,
//! `tests/mesh_to_volume_tests.rs`,
//! `tests/mesh_surface_to_volume_attributed.rs` and
//! `tests/mesher_poison_recovery.rs` — i.e. all four of the per-entry-point
//! outbound guards plus the acceptance binary.
//!
//! # Why it is shared
//!
//! The guards must stay in separate binaries — each needs a size-table
//! measurement no sibling suite can perturb, and a `tests/*.rs` file is its own
//! process — but they need the SAME instrument to measure with. Written
//! independently they carried near-verbatim copies of the serialising mutex,
//! the probe outline, the defaults pair, the clamp writer, the poison table
//! and the probe itself.
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

/// Write every process-global gmsh size option, each to `value_of(name,
/// gmsh_default)`.
///
/// The single writer behind every "establish a known table" and "poison the
/// table" step in this crate's size-option guards. Driven by the production
/// [`GMSH_SIZE_OPTION_DEFAULTS`] rather than by a local list, so a sixth option
/// added to the seam is written here too with no test edit — and so no guard
/// can drift into establishing a table the production code no longer considers
/// "default".
///
/// Takes a function of the option NAME rather than a value per position, so a
/// caller that means "defaults except this one" says exactly that
/// (`mesh_plane_2d_tests.rs`'s per-option inbound sweep) and a caller that
/// means "all of them, away from their defaults" says that
/// (`mesh_size_option_hermeticity.rs`'s poison).
///
/// Acquires `GMSH_LOCK` for the duration of the writes and releases it before
/// returning, so the measuring call that follows can take the lock itself.
/// That gap is exactly why every caller of this function must also hold
/// [`CLAMP_TEST_ORDER`] — see its doc for the false-pass mode.
pub fn write_size_options(value_of: &dyn Fn(&str, f64) -> f64) {
    let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init::ensure_initialized();
    for (option, default) in GMSH_SIZE_OPTION_DEFAULTS {
        ffi::option_set_number(option, value_of(option, default))
            .unwrap_or_else(|e| panic!("ffi::option_set_number({option}) failed: {e:?}"));
    }
}

/// One poison per size option: a value far enough from that option's gmsh
/// default to change a mesh where the fixture is sensitive to it at all.
///
/// The table every "what a leaking sibling left behind" fixture in this crate
/// writes, shared for the reason this module exists plus one specific to
/// poisons. A poison DERIVED from its default by arithmetic — the
/// `1.0 - default` this replaced — silently stops being a poison as soon as
/// the default it is derived from changes: the row is then written at its own
/// default, the fixture covers one option less, and it stays green while doing
/// it. [`poison_for`] makes that loud, and makes an option with no poison at
/// all loud too.
pub const SIZE_OPTION_POISONS: [(&str, f64); 5] = [
    ("Mesh.MeshSizeMin", 0.05),
    ("Mesh.MeshSizeMax", 0.05),
    ("Mesh.MeshSizeFromPoints", 0.0),
    ("Mesh.MeshSizeFromCurvature", 20.0),
    ("Mesh.MeshSizeExtendFromBoundary", 0.0),
];

/// The poison for `option`, given gmsh's `default` for it.
///
/// Panics rather than degrading if [`SIZE_OPTION_POISONS`] has no entry for
/// `option` — a sixth production option must arrive here too — or if the entry
/// equals `default`, which is the same hole reached by omission rather than by
/// arithmetic. Either way the fixture would be asserting against a table one
/// option less poisoned than it claims.
pub fn poison_for(option: &str, default: f64) -> f64 {
    let poison = SIZE_OPTION_POISONS
        .iter()
        .find(|(name, _)| *name == option)
        .map(|(_, value)| *value)
        .unwrap_or_else(|| {
            panic!(
                "SIZE_OPTION_POISONS must cover every GMSH_SIZE_OPTION_DEFAULTS entry: \
                 {option} has none, so a fixture that claims to poison every size option \
                 would leave it at its default and silently cover one option less"
            )
        });
    assert_ne!(
        poison, default,
        "the poison for {option} must differ from gmsh's default for it, or that row of \
         a \"fully poisoned\" table is not poisoned at all — see SIZE_OPTION_POISONS",
    );
    poison
}

/// Put every process-global gmsh size option at its [`poison_for`] value — the
/// table a leaking sibling entry point leaves behind.
pub fn poison_all_size_options() {
    write_size_options(&poison_for);
}

/// Put every process-global gmsh size option at its documented default.
///
/// This is the explicit form of what [`probe_triangle_count`] used to do
/// implicitly, for the callers that genuinely need a known starting table
/// rather than a measurement of whatever the process is carrying.
pub fn set_all_size_options_to_defaults() {
    write_size_options(&|_, default| default);
}

/// Assert that every process-global gmsh size option reads back its documented
/// default — the OUTBOUND half of task #6968, as each of the four entry points
/// that writes a size option must leave the table.
///
/// One shared loop rather than one per suite. The four guards were written
/// independently and carried near-verbatim copies of it, which is the drift
/// this module exists to prevent: a copy corrected in one suite and not the
/// others leaves the rest asserting something weaker than they claim instead of
/// failing. `entry_point` and `enforced_by` keep each guard's own diagnostic —
/// which call was made, and which production site is supposed to make it hold.
///
/// Re-acquires `GMSH_LOCK` for the read, mirroring
/// `mesh_to_volume_tests.rs::mesh_to_volume_leaves_the_gmsh_logger_stopped`:
/// the read is then serialised against any concurrent mesher rather than racing
/// one mid-flight. Callers need no `CLAMP_TEST_ORDER` for this on its own —
/// once the fix is in, "the table is at defaults" is what every sibling also
/// leaves behind, so an interleaving sibling cannot flip the result. A caller
/// that POISONS the table first does need it, for the reason
/// [`write_size_options`] gives.
pub fn assert_all_size_options_at_gmsh_defaults(entry_point: &str, enforced_by: &str) {
    let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    for (option, default) in GMSH_SIZE_OPTION_DEFAULTS {
        let observed = ffi::option_get_number(option)
            .unwrap_or_else(|e| panic!("ffi::option_get_number({option}) failed: {e:?}"));
        assert_eq!(
            observed, default,
            "{entry_point} must leave every mesh-size process-global at gmsh's default on \
             exit: {option} reads {observed}, expected {default}. gmsh's option table \
             survives gmshClear(), so a deviation here is inherited by every later call in \
             this process that does not write the option itself, pinning it to a size \
             nobody requested — the outbound direction of task #6968, enforced by \
             {enforced_by}",
        );
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
