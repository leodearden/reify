//! Hermeticity guards for the process-global mesh-size clamp around
//! [`reify_kernel_gmsh::GmshKernel::mesh_to_volume`] — the PRODUCER half of
//! task #6298.
//!
//! Task #6211 closed the CONSUMER half in `refine_volume_with_size_field`
//! (inbound: write the clamp yourself; outbound: restore gmsh's defaults on
//! every exit path) and pinned it in `tests/refine_volume_tests.rs`. This
//! binary pins the same discipline at the other end: `mesh_to_volume` writes
//! `Mesh.MeshSizeMin`/`MeshSizeMax` to its resolved size and — before #6298 —
//! never restored them, so `[size, size]` survived `gmshClear()` and the whole
//! process for any later caller that deliberately writes no clamp of its own.
//!
//! # Why a separate test binary
//!
//! `tests/mesh_to_volume_tests.rs` holds 13 unserialised `mesh_to_volume`
//! calls; cargo runs one binary's tests in a single process across parallel
//! threads with no ordering guarantee, so a clamp measurement there would race
//! siblings writing the same process-global pair — the false-pass mode
//! `refine_volume_tests.rs:19-38` documents — and adding this file's
//! `CLAMP_TEST_ORDER` mutex there would serialise thirteen unrelated tests as
//! a side effect. `refine_volume_tests.rs` is topically the wrong home: its
//! subject is the consumer. A separate `tests/*.rs` is its own compiled binary
//! and therefore its own process, so no sibling suite can perturb the option
//! table between a baseline and its re-measure.
//!
//! Only compiled and run when `cfg(has_gmsh)` is set by `build.rs` (i.e. when
//! libgmsh was found at build time). On stub builds this file is empty and the
//! test binary contains zero tests, preserving the all-OK posture of
//! `cargo test -p reify-kernel-gmsh` on hosts without libgmsh.

#![cfg(has_gmsh)]

mod common;

use std::sync::Mutex;

use reify_ir::ElementOrderTag;
use reify_kernel_gmsh::refine_volume::{GMSH_MESH_SIZE_MAX_DEFAULT, GMSH_MESH_SIZE_MIN_DEFAULT};
use reify_kernel_gmsh::{GmshKernel, MeshingOptions, ffi, init, mesh_plane_2d};

/// Whole-test-body serialisation, layered *above* `init::GMSH_LOCK`.
///
/// Every test here manipulates the process-global gmsh mesh-size clamp across
/// MULTIPLE lock acquisitions — set the clamp, then call `mesh_to_volume` or
/// `mesh_plane_2d` (each of which takes `GMSH_LOCK` itself). `GMSH_LOCK` is
/// released between those steps, so cargo's parallel test threads can
/// interleave inside the gap.
///
/// That interleave cannot produce a false FAILURE, only a false PASS, which is
/// the worse direction for a regression guard: once the fix restores the clamp
/// to gmsh's defaults on every exit, a sibling landing in the gap *erases* the
/// state under measurement and the two legs compare equal for the wrong
/// reason.
///
/// Taking this mutex as the first statement of every test makes each
/// baseline → perturb → re-measure sequence atomic with respect to its
/// siblings. `GMSH_LOCK` is strictly finer-grained (always acquired while this
/// one is held, never the reverse), so the nesting order is fixed and adds no
/// deadlock risk. Poison recovery matches the crate convention at
/// `mesh_profile_2d.rs`: a panicking test must not cascade into "lock
/// poisoned" failures for every sibling.
static CLAMP_TEST_ORDER: Mutex<()> = Mutex::new(());

/// Unit square in the XY plane — the defaults-relying 2D probe's outline.
const PROBE_OUTER: [[f64; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

/// Gmsh's documented defaults for the `Mesh.MeshSizeMin`/`MeshSizeMax` pair —
/// no floor, effectively no cap. This is the state a caller that writes no
/// clamp of its own (e.g. `mesh_plane_2d` with no requested size) expects.
///
/// Built from the production constants rather than from literals so the two
/// cannot drift: if the production notion of gmsh's defaults is ever
/// corrected, a private copy here would keep asserting against the stale pair
/// and this file's "from gmsh's defaults" baseline would quietly stop being
/// from gmsh's defaults — weakening the assertion instead of failing it.
const GMSH_CLAMP_DEFAULTS: (f64, f64) = (GMSH_MESH_SIZE_MIN_DEFAULT, GMSH_MESH_SIZE_MAX_DEFAULT);

/// Write the process-global gmsh mesh-size clamp.
///
/// gmsh's option table is process-global and is **not** reset by `gmshClear()`,
/// so `Mesh.MeshSizeMin` / `Mesh.MeshSizeMax` written by one call survive into
/// every later call in the same process. Acquires `GMSH_LOCK` for the duration
/// of the two writes and releases it before returning, so the subsequent
/// measuring call can take the lock itself.
fn set_global_mesh_size_clamp((min, max): (f64, f64)) {
    let _guard = init::GMSH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    init::ensure_initialized();
    ffi::option_set_number("Mesh.MeshSizeMin", min).expect("set MeshSizeMin");
    ffi::option_set_number("Mesh.MeshSizeMax", max).expect("set MeshSizeMax");
}

/// Triangle count of the defaults-relying 2D probe.
///
/// `mesh_size: None` is the whole point: `mesh_plane_2d` puts its
/// `Mesh.MeshSizeMin/Max` writes behind `if let Some(s) = mesh_size && s > 0.0`
/// (`mesh_profile_2d.rs:90-95`), and `geo_add_point` passes meshSize `0.0` —
/// "no prescribed size here" — so this call writes no clamp and reports
/// whatever `Mesh.MeshSizeMax` the process happens to be carrying.
fn probe_triangle_count() -> usize {
    mesh_plane_2d(&PROBE_OUTER, &[], None, false, true)
        .expect("mesh_plane_2d must succeed for a unit square")
        .triangle_indices
        .len()
        / 3
}

/// Mesh the unit cube through `GmshKernel::mesh_to_volume` at `size` and
/// return the P1 tet count.
fn mesh_to_volume_tet_count(size: f64) -> usize {
    let cube = common::unit_cube_mesh();
    let opts = MeshingOptions {
        mesh_size: Some(size),
        deterministic: true,
        ..Default::default()
    };
    GmshKernel::new()
        .mesh_to_volume(&cube, &opts, ElementOrderTag::P1)
        .unwrap_or_else(|e| panic!("mesh_to_volume({size}) must succeed: {e:?}"))
        .tet_indices()
        .expect("P1 tet mesh")
        .len()
        / 4
}

/// The OUTBOUND half of #6298: after `GmshKernel::mesh_to_volume` returns, a
/// later *defaults-relying* gmsh call must mesh exactly as if it had never
/// happened.
///
/// `mesh_to_volume` writes `Mesh.MeshSizeMin`/`MeshSizeMax` to its resolved
/// size (`kernel_real.rs:203-206`). Gmsh's option table is process-global and
/// `gmshClear()` clears MODELS, not OPTIONS, so without an RAII restore that
/// `[size, size]` pair outlives the call for the rest of the process. The
/// downstream victim is real, not hypothetical: `mesh_plane_2d(_, _, None, …)`
/// deliberately writes no clamp of its own, so `Mesh.MeshSizeMax` alone
/// decides its element size, and a leaked `[0.1, 0.1]` pins an unrelated 2D
/// profile mesh to a size nobody asked for. In production
/// `reify_solver_elastic::mesher` reaches `mesh_plane_2d` with `None` whenever
/// `auto_mesh_size_from_boundary` returns 0.0 ("unavailable") and *deliberately*
/// falls through to "gmsh's own default" (`mesher.rs:286-292`) — except after a
/// `mesh_to_volume` those are no longer the defaults.
///
/// Structure — measure the same defaults-relying call twice, straddling a
/// `mesh_to_volume`:
///
/// 1. **Warm-up `mesh_to_volume`.** Not decoration: that function also writes
///    `General.NumThreads`, `Mesh.Algorithm3D` and `Mesh.ElementOrder`, none of
///    which `mesh_plane_2d` sets. Running one first puts all three in their
///    post-`mesh_to_volume` state for BOTH measurements, so the clamp is the
///    only free variable. `ElementOrderTag::P1` throughout — a leaked
///    `Mesh.ElementOrder = 2` would make gmsh emit 6-node triangles and the
///    probe's element readback would return nothing, a confound unrelated to
///    the clamp.
/// 2. **Baseline**, from an explicitly-defaulted clamp.
/// 3. **A fine `mesh_to_volume`** — `FINE` is 10x finer than the probe's own
///    extent.
/// 4. **Re-measure.** Must equal the baseline exactly.
///
/// # Measured, on unmodified main at 89df46b9ae (pre-fix)
///
/// baseline = **162** triangles, after `mesh_to_volume(FINE = 0.1)` = **242**
/// triangles — a +49% jump, deterministic across repeats and identical with or
/// without the warm-up. (The 3D mesh itself is 4575 P1 tets at 0.1 and 194 at
/// the 0.5 warm-up, so the test stays fast.) That 162 → 242 difference IS the
/// defect. It is smaller than the "orders of magnitude" a naive reading of the
/// cap would predict, because the probe is not unconstrained at gmsh's
/// defaults either — but the assertion is an exact equality between two runs of
/// one function, not a threshold, so the margin only has to be non-zero and
/// repeatable, and 80 triangles is far outside any rounding.
///
/// Why the probe observes the leak's EFFECT rather than reading the option
/// table back: this crate's FFI surface exposes `option_set_number` but no
/// `option_get_number` (the same reason `MeshSizeClampReset` restores defaults
/// rather than as-found). Adding a getter purely for one assertion would widen
/// the FFI surface, and the effect-based probe is the stronger guard anyway —
/// it fails if the clamp leaks by ANY route, not only via the one option name
/// the test thought to read. This is the exact structure already validated by
/// the green `refine_volume_tests.rs::refine_leaves_the_default_clamp_behind_
/// for_a_later_defaults_relying_call`.
#[test]
fn mesh_to_volume_leaves_the_default_clamp_behind_for_a_later_defaults_relying_call() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());

    /// The size the `mesh_to_volume` in the middle requests. 10x finer than
    /// the probe's own extent, so a leak moves the triangle count well clear
    /// of any rounding, while keeping the 3D mesh at ~4.5k tets so the test
    /// stays fast (0.05 on a unit cube is ~48k tets for no extra signal).
    const FINE: f64 = 0.1;

    // 1. Warm-up: normalise every global `mesh_to_volume` writes but the probe
    //    does not, for both measurements.
    mesh_to_volume_tet_count(0.5);

    // 2. Baseline, from a known-default clamp.
    set_global_mesh_size_clamp(GMSH_CLAMP_DEFAULTS);
    let baseline = probe_triangle_count();
    assert!(
        baseline > 0,
        "the defaults-relying 2D probe must produce triangles; got an empty mesh",
    );

    // 3. A fine mesh_to_volume in between.
    mesh_to_volume_tet_count(FINE);

    // 4. The same defaults-relying call must be unaffected by it.
    let after = probe_triangle_count();
    assert_eq!(
        after, baseline,
        "GmshKernel::mesh_to_volume must restore Mesh.MeshSizeMin/Max to gmsh's defaults \
         on exit: the same mesh_plane_2d(mesh_size: None) call gave {baseline} triangles \
         before a mesh_to_volume at size {FINE} and {after} after it. A larger count means \
         mesh_to_volume left its clamp behind and pinned an unrelated downstream mesh to a \
         size nobody requested — the producer half of task #6298, guarded by \
         `mesh_size_clamp::MeshSizeClampReset` armed in kernel_real.rs::mesh_to_volume",
    );
}
