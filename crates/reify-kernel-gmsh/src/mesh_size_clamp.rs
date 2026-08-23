//! The crate-wide `Mesh.MeshSizeMin`/`MeshSizeMax` restore discipline.
//!
//! # The invariant
//!
//! Gmsh's option table is **process-global** and `gmshClear()` clears MODELS,
//! not OPTIONS. A `Mesh.MeshSizeMin` / `Mesh.MeshSizeMax` pair written by one
//! entry point therefore survives for the life of the process and is inherited
//! by every later call that does not write the pair itself. So:
//!
//! > **Every entry point that writes the `Mesh.MeshSizeMin`/`MeshSizeMax` pair
//! > must restore gmsh's documented defaults before returning — on every exit
//! > path, early `?`-returns included.**
//!
//! Restoring DEFAULTS rather than the values found on entry is deliberate:
//! this crate's FFI surface exposes `option_set_number` but no
//! `option_get_number`, so "as found" is not observable here. Defaults are the
//! right target anyway — they are what a caller that writes no clamp of its
//! own expects to get, so leaving them behind means no downstream path
//! inherits state from this one.
//!
//! The victim is real, not hypothetical. `mesh_profile_2d::mesh_plane_2d` puts
//! its clamp writes behind `if let Some(s) = mesh_size && s > 0.0`, and
//! `geo_add_point` passes meshSize `0.0` — "no prescribed size here" — so with
//! `Mesh.MeshSizeFromPoints` on and no point sizes, `Mesh.MeshSizeMax` alone
//! decides the element size. A leaked fine cap pins an unrelated 2D profile
//! mesh to a size nobody asked for.
//!
//! # Consumers
//!
//! * [`crate::refine_volume::refine_volume_with_size_field`] — since task
//!   #6211, which is where [`MeshSizeClampReset`] was first written.
//! * [`crate::kernel_real::GmshKernel::mesh_to_volume`] — since task #6298,
//!   which moved the guard here so the two share one implementation rather
//!   than two hand-written resets free to drift apart.
//!
//! Each consumer has its own outbound guard so neither can rot into a comment:
//! `tests/refine_volume_tests.rs::refine_leaves_the_default_clamp_behind_for_a_later_defaults_relying_call`
//! and
//! `tests/mesh_to_volume_clamp_hermeticity.rs::mesh_to_volume_leaves_the_default_clamp_behind_for_a_later_defaults_relying_call`.
//! Both observe the leak's *effect* on a defaults-relying `mesh_plane_2d`
//! probe rather than reading the option table back, for the no-getter reason
//! above.
//!
//! # Known non-compliant writers
//!
//! The invariant above is the crate's stated rule, not yet its measured state.
//! Two entry points still write the pair and never restore it, so a later
//! defaults-relying call in the same process inherits their size:
//!
//! * [`crate::mesh_profile_2d::mesh_plane_2d`] — `mesh_profile_2d.rs`, the
//!   `Mesh.MeshSizeMin`/`MeshSizeMax` writes behind
//!   `if let Some(s) = mesh_size && s > 0.0`.
//! * `mesh_boundary::mesh_surface_to_volume_with_attribution` — the same guard
//!   shape on `options.mesh_size`, inside its
//!   `run_meshing_with_entity_queries` helper. Named rather than linked
//!   because `mesh_boundary` is `#[cfg(feature = "mesh-morph")]`, so an
//!   intra-doc link to it is unresolvable in a default-feature `cargo doc`.
//!
//! Both are named, with file and line, in task **#6212**'s description and
//! appear in its `files_to_modify`, so bringing them onto this seam is that
//! task's to close — not an unowned gap. Listing them here rather than only in
//! `refine_volume.rs`'s inline comment is deliberate: this module is what a
//! future author reads before adding a THIRD writer, and an invariant stated
//! without its live exceptions is how the leak stays open by accident.
//!
//! # Scope
//!
//! This module covers the `MeshSizeMin`/`MeshSizeMax` pair only. The
//! `Mesh.MeshSizeFromPoints` / `MeshSizeFromCurvature` /
//! `MeshSizeExtendFromBoundary` trio is the same defect class in the same
//! direction and is still left behind by `refine_volume_with_size_field` for a
//! later caller to inherit — tracked as task #6212, which owns extending this
//! seam to those three (and adding the `option_get_number` FFI getter that
//! would let a restore be *as found* rather than to defaults). #6212 also owns
//! the remaining INBOUND hole in `mesh_to_volume`, where a resolved size of
//! `0.0` skips the clamp writes entirely and the call inherits whatever is in
//! the table.

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

/// RAII reset of the process-global `Mesh.MeshSizeMin`/`MeshSizeMax` pair to
/// gmsh's defaults, covering the early-`?`-return paths as well as success.
///
/// Restores DEFAULTS rather than the values found on entry: gmsh's C API
/// exposes no reader for a numeric option in this crate's FFI surface, so
/// "as found" is not observable here. Defaults are the right target anyway —
/// they are what a caller that writes no clamp of its own expects to get, so
/// leaving them behind means no downstream path inherits state from this one
/// (task #6211).
///
/// # Why it borrows the lock guard
///
/// The two FFI writes in `drop` mutate gmsh's process-global option table and
/// must therefore happen while `init::GMSH_LOCK` is held. The
/// `PhantomData<&'g MutexGuard<'g, ()>>` makes that structural rather than a
/// comment a refactor can quietly violate: [`Self::armed`] can only be called
/// with a live guard in hand, so the binding cannot be hoisted above the
/// `let _guard = …` line, and because this type has a `Drop` impl (no
/// `#[may_dangle]`) dropck requires the borrow to still be live when it drops
/// — which forces the writes to land *before* the lock is released.
///
/// # Why `pub`
///
/// `pub` rather than `pub(crate)` because three PUBLIC doc surfaces name this
/// type as the mechanism enforcing their stated contract — this module's own
/// doc, `refine_volume`'s module doc, and `GmshKernel::mesh_to_volume`'s. A
/// `pub(crate)` target makes each of those an unresolvable
/// `rustdoc::private_intra_doc_links` link that renders as dead text, so the
/// reader of `mesh_to_volume`'s docs is pointed at a type they cannot navigate
/// to. Exporting it costs nothing in encapsulation: [`Self::armed`] needs a
/// live `&MutexGuard` borrowed from [`crate::init::GMSH_LOCK`] (itself `pub`
/// for the same order of reason), so the only way to construct one is to
/// already hold the lock this crate serialises every gmsh call on.
pub struct MeshSizeClampReset<'g>(std::marker::PhantomData<&'g std::sync::MutexGuard<'g, ()>>);

impl<'g> MeshSizeClampReset<'g> {
    /// Arm the reset. Takes the live `GMSH_LOCK` guard by reference purely for
    /// its lifetime — the guard itself is never touched.
    pub fn armed(_guard: &'g std::sync::MutexGuard<'g, ()>) -> Self {
        Self(std::marker::PhantomData)
    }
}

impl Drop for MeshSizeClampReset<'_> {
    fn drop(&mut self) {
        // Best-effort, like the trailing `ffi::clear()`: a failure here cannot
        // be reported from `drop` and must not mask the real result.
        let _ = crate::ffi::option_set_number("Mesh.MeshSizeMin", GMSH_MESH_SIZE_MIN_DEFAULT);
        let _ = crate::ffi::option_set_number("Mesh.MeshSizeMax", GMSH_MESH_SIZE_MAX_DEFAULT);
    }
}
