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
//! `clamp_probe::CLAMP_TEST_ORDER` documents — and adding that mutex there
//! would serialise thirteen unrelated tests as a side effect.
//! `refine_volume_tests.rs` is topically the wrong home: its subject is the
//! consumer. A separate `tests/*.rs` is its own compiled binary and therefore
//! its own process, so no OTHER suite can perturb the option table between a
//! baseline and its re-measure.
//!
//! Isolation *within* this binary is a separate mechanism and is not free
//! either: `CLAMP_TEST_ORDER` serialises the two test bodies but restores
//! nothing, and the second test calls `refine_volume_with_size_field`, which
//! leaves `Mesh.MeshSizeFromPoints` / `MeshSizeFromCurvature` /
//! `MeshSizeExtendFromBoundary` behind (task #6212's open leak). That is why
//! `clamp_probe::probe_triangle_count` pins those three to gmsh's defaults
//! itself — see its docstring — so the numbers recorded below are a function
//! of the clamp alone rather than of which test won cargo's thread race.
//!
//! Only compiled and run when `cfg(has_gmsh)` is set by `build.rs` (i.e. when
//! libgmsh was found at build time). On stub builds this file is empty and the
//! test binary contains zero tests, preserving the all-OK posture of
//! `cargo test -p reify-kernel-gmsh` on hosts without libgmsh.

#![cfg(has_gmsh)]

mod common;

// The clamp probe and its serialising mutex are shared verbatim with
// `tests/refine_volume_tests.rs`, the other half of this discipline. Declared
// by path rather than through `common/mod.rs`, whose stated scope is the #6200
// geometry fixtures; see `common/clamp_probe.rs` for why one copy matters.
#[path = "common/clamp_probe.rs"]
mod clamp_probe;

use clamp_probe::{
    CLAMP_TEST_ORDER, GMSH_CLAMP_DEFAULTS, probe_triangle_count, set_global_mesh_size_clamp,
};
use reify_ir::ElementOrderTag;
use reify_kernel_gmsh::{GmshKernel, MeshingOptions, refine_volume_with_size_field};

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
/// size (the two `option_set_number` calls in `kernel_real.rs` guarded by
/// `if resolved_size > 0.0`). Gmsh's option table is process-global and
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
///    which `mesh_plane_2d` sets and none of which
///    [`probe_triangle_count`] pins. Running one first puts all three in their
///    post-`mesh_to_volume` state for BOTH measurements, so the clamp is the
///    only free variable. `ElementOrderTag::P1` throughout — a leaked
///    `Mesh.ElementOrder = 2` would make gmsh emit 6-node triangles and the
///    probe's element readback would return nothing, a confound unrelated to
///    the clamp. The `MeshSizeFromPoints` / `FromCurvature` /
///    `ExtendFromBoundary` trio is NOT this warm-up's job — `mesh_to_volume`
///    never writes it, so the probe pins it itself.
/// 2. **Baseline**, from an explicitly-defaulted clamp.
/// 3. **A fine `mesh_to_volume`** — `FINE` is 10x finer than the probe's own
///    extent.
/// 4. **Re-measure.** Must equal the baseline exactly.
///
/// # Measured, with `MeshSizeClampReset::armed` commented out of `mesh_to_volume`
///
/// baseline = **162** triangles, after `mesh_to_volume(FINE = 0.1)` = **242**
/// triangles — a +49% jump. That 162 → 242 difference IS the defect. It is
/// smaller than the "orders of magnitude" a naive reading of the cap would
/// predict, because the probe is not unconstrained at gmsh's defaults either —
/// but the assertion is an exact equality between two runs of one function, not
/// a threshold, so the margin only has to be non-zero and repeatable, and 80
/// triangles is far outside any rounding.
///
/// Both numbers are reproducible rather than incidental to one test ordering,
/// which is what [`probe_triangle_count`]'s trio pinning buys. The same
/// `162 / 242` came back from three different process states: this test alone
/// via `--exact`; this whole binary; and this binary under `--test-threads=1`.
/// The same `162` baseline also came back from
/// `refine_volume_tests.rs::refine_leaves_the_default_clamp_behind_for_a_later_defaults_relying_call`
/// — a different binary, whose probe runs after a refine has written the trio
/// to `1 / 0 / 0` (that run measured `162 → 944` with `refine_volume.rs`'s own
/// guard commented out). Before the pinning, the reviewer of #6298 measured
/// `48 / 246` from one interleaving of this binary against `162 / 242` from
/// another.
///
/// The 3D meshes in between are ~4.5k P1 tets at `FINE` and ~200 at the 0.5
/// warm-up, so the test stays fast. Those two counts are a cost note, not an
/// assertion, and unlike the probe they ARE order-sensitive: `mesh_to_volume`
/// inherits the size-source trio rather than pinning it (#6212).
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

/// #6298's TITLE symptom, end to end: a `mesh_to_volume`-then-refine sequence
/// must respond to the refine's own size field.
///
/// `refine(uniform 0.125)` after `mesh_to_volume(0.5)` must yield strictly
/// more tets than `refine(uniform 0.5)` after the same seed. If the seed's
/// global clamp — not the requested field — decides the density, the two come
/// back equal (and, as #6211 measured, bit-identical).
///
/// # This test is GREEN ON ARRIVAL, and that is expected
///
/// Do not "fix" anything when it passes. Task #6211 already landed the INBOUND
/// half: `refine_volume_with_size_field` writes `Mesh.MeshSizeMin`/`MeshSizeMax`
/// itself on entry, overwriting whatever the seed left behind, so the sequence
/// already responds to the field. #6298 closed the OUTBOUND half at the other
/// end (the seed no longer leaks in the first place).
///
/// It is kept because it is the only guard that drives the real
/// producer→consumer sequence #6298's title names. The four in
/// `refine_volume_tests.rs` poison the clamp *synthetically*, by writing the
/// option table directly, and so pin what the consumer does with a hostile
/// table rather than what the two functions do to each other; the sibling
/// above runs a real `mesh_to_volume` but reads its aftermath through a 2D
/// probe, never through a refine. Neither shape would notice if the two
/// entry points started disagreeing about the clamp in some way the synthetic
/// poison does not model.
///
/// # Falsifiability, measured rather than assumed
///
/// A green-on-arrival guard that cannot fail is worthless, so all four
/// combinations of the two halves were actually run against this test. The
/// probed halves are the two `ffi::option_set_number("Mesh.MeshSizeMin"` /
/// `"Mesh.MeshSizeMax", …)` calls in `refine_volume.rs` (INBOUND, #6211) and
/// the `MeshSizeClampReset::armed(&_guard)` binding in
/// `kernel_real.rs::mesh_to_volume` (OUTBOUND, #6298):
///
/// | inbound (#6211) | outbound (#6298) | coarse | fine | this test |
/// |-----------------|------------------|--------|------|-----------|
/// | on              | on   (today)     |    181 | 2420 | PASS      |
/// | off             | on               |    141 |  367 | PASS      |
/// | on              | off  (pre-#6298) |    181 | 2420 | PASS      |
/// | off             | off  (pre-both)  |    181 |  181 | **FAIL**  |
///
/// So the test is genuinely falsifiable, and precisely at the point that
/// matters: it goes red exactly when BOTH halves are gone, which is the state
/// the codebase was in when #6298 was filed. Either half alone suffices for
/// this particular sequence, which is why removing just one leaves it green —
/// that redundancy is the fix working, not the test failing to measure.
///
/// The two failing counts are `181 == 181`, matching #6211's measured
/// `mesh_to_volume(0.5) → refine(any field) = 181` row exactly. The passing
/// margin is 13x on a strict inequality, no tolerance.
///
/// The row-2 numbers (141 / 367) are worth naming because they are the ones
/// #6211's table records for `refine(uniform 0.5)` / `refine(uniform 0.125)`
/// *alone*: with the inbound writes gone, refine runs under gmsh's default
/// clamp and the per-corner size field alone drives the mesh. Today's 181 /
/// 2420 are denser because the inbound `MeshSizeMax = max(vertex_sizes)` write
/// caps interior growth that `Mesh.MeshSizeExtendFromBoundary = 0` would
/// otherwise leave unbounded — the effect its own inline rationale in
/// `refine_volume.rs` claims, here observed.
#[test]
fn refine_after_mesh_to_volume_honours_its_own_size_field() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());

    /// The size the seeding `mesh_to_volume` requests, and therefore the
    /// `[SEED, SEED]` clamp it used to leave behind.
    const SEED: f64 = 0.5;

    let cube = common::unit_cube_mesh();
    let n_surface_verts = cube.vertices.len() / 3;

    // Seed through the real producer, then refine with a uniform field.
    //
    // `refine_volume_with_size_field` never reads `options.mesh_size` — the
    // per-vertex field is what decides element size — so `opts` deliberately
    // leaves it `None`. Passing a size there would imply a dependency that
    // does not exist.
    let refine_after_seed = |field: f64| -> usize {
        mesh_to_volume_tet_count(SEED);
        let opts = MeshingOptions {
            deterministic: true,
            ..Default::default()
        };
        refine_volume_with_size_field(
            &cube,
            &vec![field; n_surface_verts],
            &opts,
            ElementOrderTag::P1,
        )
        .unwrap_or_else(|e| panic!("refine_volume_with_size_field({field}) must succeed: {e:?}"))
        .tet_indices()
        .expect("P1 tet mesh")
        .len()
            / 4
    };

    let coarse = refine_after_seed(0.5);
    let fine = refine_after_seed(0.125);

    assert!(
        fine > coarse,
        "a refine after a mesh_to_volume seed must honour its own size field: \
         refine(uniform 0.125) gave {fine} tets and refine(uniform 0.5) gave {coarse}, \
         both seeded by mesh_to_volume({SEED}). Equal counts mean the seed's global \
         Mesh.MeshSizeMin/Max clamp — not the requested field — decided the density, \
         i.e. the leak of tasks #6298 / #6211 is back. Check both halves of the clamp \
         discipline: refine_volume.rs's inbound writes at the \"Mesh-size clamp: set \
         explicitly, never inherited\" block, and \
         mesh_size_clamp::MeshSizeClampReset armed in kernel_real.rs::mesh_to_volume",
    );
}
