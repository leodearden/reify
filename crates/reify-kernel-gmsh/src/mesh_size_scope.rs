//! The crate-wide mesh-size option-table discipline.
//!
//! # The invariant
//!
//! Gmsh's option table is **process-global** and `gmshClear()` clears MODELS,
//! not OPTIONS. Any size-controlling option written by one entry point
//! therefore survives for the life of the process and is inherited by every
//! later call that does not write it. So:
//!
//! > **Gmsh's size-option table is at its documented defaults outside a
//! > [`MeshSizeScope`].**
//!
//! [`MeshSizeScope`] enforces that from both ends. [`MeshSizeScope::entered`]
//! writes every entry of [`GMSH_SIZE_OPTION_DEFAULTS`] on the way IN, and
//! `drop` writes them again on the way OUT — on every exit path, early
//! `?`-returns included. An entry point layers its own deviations on top of
//! the defaults the scope established, and they die with the scope.
//!
//! Taking both halves in one type is what makes the invariant total, and
//! either half alone is why this defect survived two earlier attempts at it.
//! Inbound-at-defaults means no entry point's output depends on call order
//! even if some future writer forgets to restore. Outbound-at-defaults means
//! no entry point poisons a later one even if that one forgets to set. A hole
//! in either direction is silent rather than loud: the victim still produces a
//! mesh, just not the one it asked for.
//!
//! A `Drop` guard specifically — rather than restores at the end of each
//! function — is forced by the many `?` early-return paths in
//! [`crate::refine_volume::refine_volume_with_size_field`] and
//! [`crate::kernel_real::GmshKernel::mesh_to_volume`], each of which would
//! otherwise need its own restore before every `?`.
//!
//! # Why DEFAULTS rather than "as found"
//!
//! Entering at defaults makes "as found" always "defaults", so neither half
//! ever needs to read the table to know what to WRITE. The getter
//! [`crate::ffi::option_get_number`] (task #6968) is used only to check that a
//! write landed — see [`MeshSizeScope::entered`]'s "Why each write is read
//! back" — and, in `tests/`, so the per-entry-point guards can OBSERVE the
//! invariant directly instead of inferring it from mesh density.
//!
//! Defaults are the right target on their own merits anyway: they are what a
//! caller that writes no size option of its own expects to get.
//!
//! # The victim is real, not hypothetical
//!
//! [`crate::mesh_profile_2d::mesh_plane_2d`] puts its clamp writes behind
//! `if let Some(s) = mesh_size && s > 0.0`, and `geo_add_point` passes
//! meshSize `0.0` — "no prescribed size here" — so with
//! `Mesh.MeshSizeFromPoints` on and no point sizes, the process-global table
//! alone decides element size. Measured on the unit-square probe in
//! `tests/common/clamp_probe.rs`: 162 triangles with
//! `Mesh.MeshSizeExtendFromBoundary` at its default of `1`, and 48 with the
//! `0` that `refine_volume_with_size_field` used to leak — a 3.4x error in an
//! unrelated 2D mesh, decided by nothing but which entry point ran first.
//! `reify_solver_elastic::mesher` reaches that path for real, passing
//! `mesh_size: None` whenever `auto_mesh_size_from_boundary` returns `0.0`.
//!
//! # Consumers
//!
//! Every entry point in this crate that writes a size option:
//!
//! * [`crate::refine_volume::refine_volume_with_size_field`] — guarded by
//!   `tests/refine_volume_tests.rs::refine_volume_leaves_every_size_option_at_gmsh_defaults`.
//! * [`crate::kernel_real::GmshKernel::mesh_to_volume`] — guarded by
//!   `tests/mesh_to_volume_tests.rs::mesh_to_volume_leaves_every_size_option_at_gmsh_defaults`,
//!   with the poisoned-table form of the same read in
//!   `tests/mesh_size_option_hermeticity.rs::mesh_to_volume_enters_and_leaves_gmshs_size_defaults_whatever_the_table_held`
//!   (this producer never writes the size-SOURCE trio, so only a poisoned
//!   table makes those three rows bite).
//! * [`crate::mesh_profile_2d::mesh_plane_2d`] — guarded by
//!   `tests/mesh_plane_2d_tests.rs::mesh_plane_2d_leaves_every_size_option_at_gmsh_defaults`.
//! * `mesh_boundary::mesh_surface_to_volume_with_attribution`, via its
//!   `run_meshing_with_entity_queries` helper — guarded by
//!   `tests/mesh_surface_to_volume_attributed.rs::mesh_surface_to_volume_with_attribution_leaves_every_size_option_at_gmsh_defaults`.
//!   Named rather than linked because `mesh_boundary` is
//!   `#[cfg(feature = "mesh-morph")]`, so an intra-doc link to it is
//!   unresolvable in a default-feature `cargo doc`.
//!
//! One guard per writer, all four reading the table through
//! [`crate::ffi::option_get_number`], so none can rot into a comment. Both
//! call-order directions are pinned together, in one process, by
//! `tests/mesh_size_option_hermeticity.rs`.
//!
//! # Adding a writer
//!
//! A fifth entry point that writes a size option must arm a scope, and a sixth
//! process-global that decides element size must join
//! [`GMSH_SIZE_OPTION_DEFAULTS`] with its default MEASURED, not assumed. The
//! four guards above iterate that constant, so a new entry there is asserted
//! against every existing writer on the day it lands — but nothing forces a
//! new WRITER to arm a scope, which is why each one carries its own guard.

use reify_ir::GeometryError;

/// Gmsh's documented default for `Mesh.MeshSizeMin` — no floor.
///
/// `pub` (like [`crate::init::GMSH_LOCK`], and for the same reason) so this
/// crate's `tests/` binaries — separate compilation units — can restore the
/// process-global clamp to gmsh's defaults without re-declaring the literal.
/// A test-local copy could drift silently away from the value this module
/// actually writes, which would quietly weaken the "from gmsh's defaults"
/// leg of the clamp guards' assertions rather than fail it.
pub const GMSH_MESH_SIZE_MIN_DEFAULT: f64 = 0.0;

/// Gmsh's documented default for `Mesh.MeshSizeMax` — effectively no cap.
///
/// `pub` for the same reason as [`GMSH_MESH_SIZE_MIN_DEFAULT`].
pub const GMSH_MESH_SIZE_MAX_DEFAULT: f64 = 1.0e22;

/// Every process-global gmsh option that decides element size, paired with
/// gmsh's default for it.
///
/// The single source of truth for both halves of the discipline: what
/// [`MeshSizeScope::entered`] establishes inbound and what `drop` restores
/// outbound, and what each entry point's guard in `tests/` asserts. A future
/// author adding a sixth such option needs to find exactly this list.
///
/// The two CLAMP options say how sizes are bounded; the three SOURCE options
/// say where they come from. Both decide element size, so both belong here —
/// the distinction matters only when reading a density-based probe, which can
/// observe the clamp but not the sources (their effect vanishes whenever
/// `MeshSizeMin == MeshSizeMax`).
///
/// Every default MEASURED against the shipped library rather than assumed:
/// `/opt/reify-deps/bin/gmsh` 4.15.2 `-0` on a `.geo` of
/// `Printf("%g", Mesh.MeshSizeMin)` and friends prints `0`, `1e+22`, `1`, `0`,
/// `1`. Note the last one in particular:
/// `refine_volume_with_size_field` WRITES `Mesh.MeshSizeExtendFromBoundary =
/// 0` as its own deliberate deviation, but gmsh's DEFAULT is `1` — restoring
/// `0` here would entrench the very leak this seam closes, and was the entire
/// measured content of it (task #6968).
pub const GMSH_SIZE_OPTION_DEFAULTS: [(&str, f64); 5] = [
    ("Mesh.MeshSizeMin", GMSH_MESH_SIZE_MIN_DEFAULT),
    ("Mesh.MeshSizeMax", GMSH_MESH_SIZE_MAX_DEFAULT),
    ("Mesh.MeshSizeFromPoints", 1.0),
    ("Mesh.MeshSizeFromCurvature", 0.0),
    ("Mesh.MeshSizeExtendFromBoundary", 1.0),
];

/// RAII scope over gmsh's process-global size-option table: enters at gmsh's
/// documented defaults and leaves at them, covering the early-`?`-return paths
/// as well as success.
///
/// Arm one immediately after `init::ensure_initialized()`, then write the
/// entry point's own deviations on top. Both directions matter and neither is
/// redundant — see this module's doc for why either half alone leaves a silent
/// hole.
///
/// # Why it borrows the lock guard
///
/// The FFI writes in [`Self::entered`] and in `drop` mutate gmsh's
/// process-global option table and must therefore happen while
/// `init::GMSH_LOCK` is held. The `PhantomData<&'g MutexGuard<'g, ()>>` makes
/// that structural rather than a comment a refactor can quietly violate:
/// [`Self::entered`] can only be called with a live guard in hand, so the
/// binding cannot be hoisted above the `let _guard = …` line, and because this
/// type has a `Drop` impl (no `#[may_dangle]`) dropck requires the borrow to
/// still be live when it drops — which forces the restore to land *before* the
/// lock is released.
///
/// # Why `pub`
///
/// `pub` rather than `pub(crate)` because several PUBLIC doc surfaces name
/// this type as the mechanism enforcing their stated contract — this module's
/// own doc, `refine_volume`'s module doc, `mesh_plane_2d`'s and
/// `GmshKernel::mesh_to_volume`'s. A `pub(crate)` target makes each of those
/// an unresolvable `rustdoc::private_intra_doc_links` link that renders as
/// dead text, so the reader of `mesh_to_volume`'s docs is pointed at a type
/// they cannot navigate to. Exporting it costs nothing in encapsulation:
/// [`Self::entered`] needs a live `&MutexGuard` borrowed from
/// [`crate::init::GMSH_LOCK`] (itself `pub` for the same order of reason), so
/// the only way to construct one is to already hold the lock this crate
/// serialises every gmsh call on.
pub struct MeshSizeScope<'g>(std::marker::PhantomData<&'g std::sync::MutexGuard<'g, ()>>);

impl<'g> MeshSizeScope<'g> {
    /// Enter the scope: write every [`GMSH_SIZE_OPTION_DEFAULTS`] entry, so
    /// the caller starts from a known table rather than from whatever a
    /// sibling entry point last left behind.
    ///
    /// Takes the live `GMSH_LOCK` guard by reference purely for its lifetime —
    /// the guard itself is never touched.
    ///
    /// Fallible, unlike `drop`'s best-effort restore, and deliberately so:
    /// failing to establish a known table means the caller's output is a
    /// function of call order, which is exactly the defect this type exists to
    /// remove. That must be reported, not swallowed.
    ///
    /// # Why each write is read back
    ///
    /// Because a WRITE alone does not report failure. MEASURED against the
    /// shipped `libgmsh.so` 4.15.2: `gmshOptionSetNumber` called before
    /// `gmshInitialize` logs `Error : Gmsh has not been initialized` to stderr,
    /// changes nothing, and returns `ierr = 0`. A set-only `entered` therefore
    /// returns `Ok` having established nothing — the silent hole its own
    /// `Result` claims to close. And since every name in
    /// [`GMSH_SIZE_OPTION_DEFAULTS`] is a compile-time constant today's gmsh
    /// accepts, that `Result` would be unreachable on a live library and
    /// vacuous on a dead one: a type whose whole claim is that its numbers are
    /// measured cannot carry a failure path nothing can take.
    ///
    /// Verifying through [`crate::ffi::option_get_number`] catches both
    /// failures that can actually happen. An UNINITIALISED library: pre-init
    /// `gmshOptionGetNumber` leaves the out-param untouched and also returns
    /// `ierr = 0`, so the wrapper reads its own `0.0` seed and every default
    /// but `Mesh.MeshSizeMin`'s own `0.0` mismatches. A RENAMED OR REMOVED
    /// option after a gmsh version bump: `ierr = 1`, so the getter itself
    /// returns `Err`. The cost is five option-table lookups against a mesh
    /// generation measured in milliseconds.
    ///
    /// A partial write before the failure needs no unwinding — `Self` is not
    /// yet constructed, so the caller propagates the error and the NEXT scope
    /// re-establishes the defaults on entry.
    pub fn entered(_guard: &'g std::sync::MutexGuard<'g, ()>) -> Result<Self, GeometryError> {
        for (option, default) in GMSH_SIZE_OPTION_DEFAULTS {
            crate::ffi::option_set_number(option, default)?;
            let observed = crate::ffi::option_get_number(option)?;
            if observed != default {
                return Err(GeometryError::OperationFailed(format!(
                    "MeshSizeScope::entered: gmsh reported success writing {option} = \
                     {default} but the table reads {observed}. The size-option table is \
                     not at its defaults, so this call's output would be a function of \
                     what ran before it rather than of its own arguments. The usual cause \
                     is gmsh not being initialized — init::ensure_initialized() must run \
                     before a scope is entered"
                )));
            }
        }
        Ok(Self(std::marker::PhantomData))
    }
}

impl Drop for MeshSizeScope<'_> {
    fn drop(&mut self) {
        // Best-effort, like the trailing `ffi::clear()`: a failure here cannot
        // be reported from `drop` and must not mask the real result.
        for (option, default) in GMSH_SIZE_OPTION_DEFAULTS {
            let _ = crate::ffi::option_set_number(option, default);
        }
    }
}
