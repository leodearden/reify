//! Hermeticity guards for gmsh's process-global mesh-size OPTION TABLE, across
//! every entry point in this crate that writes one — the acceptance surface of
//! task #6968.
//!
//! Its remit widened twice. It began as the PRODUCER half of task #6298, about
//! the `Mesh.MeshSizeMin`/`MeshSizeMax` pair leaving
//! [`reify_kernel_gmsh::GmshKernel::mesh_to_volume`]; #6968 widened it from
//! that pair to all five size options, and from one entry point to four.
//! Hence the rename from `mesh_to_volume_clamp_hermeticity.rs`.
//!
//! # What this binary is for that the per-entry-point guards are not
//!
//! Each of the four entry points carries its own guard in its own suite, and
//! each reads the option table back directly through `ffi::option_get_number`.
//! Those pin what ONE function leaves behind. This binary pins what the
//! functions do TO EACH OTHER: `every_entry_point_measures_the_same_whatever_ran_before_it`
//! runs every ORDERED PAIR of entry points in one process and requires the
//! second one's output to equal the output it produces alone.
//!
//! That is the property the task is actually about, and it is not implied by
//! the four table reads. A table read cannot see a leak through an option
//! nobody thought to name, and the pair sweep does not care which option
//! carried it — it fails if the answer changes at all.
//!
//! # Why a separate test binary
//!
//! `tests/mesh_to_volume_tests.rs` holds 13 unserialised `mesh_to_volume`
//! calls; cargo runs one binary's tests in a single process across parallel
//! threads with no ordering guarantee, so an option-table measurement there
//! would race siblings writing the same process-globals — the false-pass mode
//! `clamp_probe::CLAMP_TEST_ORDER` documents — and adding that mutex there
//! would serialise thirteen unrelated tests as a side effect.
//! `refine_volume_tests.rs` is topically the wrong home: its subject is one
//! entry point. A test about call ORDER needs to own its process, and a
//! separate `tests/*.rs` is its own compiled binary and therefore its own
//! process, so no OTHER suite can perturb the option table between a baseline
//! and its re-measure.
//!
//! Isolation *within* this binary is a separate mechanism: `CLAMP_TEST_ORDER`
//! serialises the test bodies. It restores nothing — that is now the entry
//! points' own job, which is the whole point of #6968, and it is why
//! `clamp_probe::probe_triangle_count` no longer pins the size-SOURCE trio
//! itself. It used to, so that its numbers were a function of the clamp alone
//! while #6212's trio leak was live; keeping that would have MASKED the very
//! leak this binary now tests for.
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
    CLAMP_TEST_ORDER, GMSH_CLAMP_DEFAULTS, assert_all_size_options_at_gmsh_defaults,
    probe_triangle_count, set_all_size_options_to_defaults, set_global_mesh_size_clamp,
    write_size_options,
};
use reify_ir::ElementOrderTag;
use reify_kernel_gmsh::{GmshKernel, MeshingOptions, refine_volume_with_size_field};
#[cfg(feature = "mesh-morph")]
use reify_kernel_gmsh::{EntityAttribution, mesh_surface_to_volume_with_attribution};

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

/// `mesh_to_volume` on the unit cube at gmsh's auto-derived size — the shape
/// the pair sweep measures, with no explicit `mesh_size` so the call is
/// sensitive to the size table it inherits.
fn mesh_to_volume_default_tet_count() -> usize {
    let cube = common::unit_cube_mesh();
    let opts = MeshingOptions {
        deterministic: true,
        ..Default::default()
    };
    GmshKernel::new()
        .mesh_to_volume(&cube, &opts, ElementOrderTag::P1)
        .unwrap_or_else(|e| panic!("mesh_to_volume must succeed: {e:?}"))
        .tet_indices()
        .expect("P1 tet mesh")
        .len()
        / 4
}

/// `refine_volume_with_size_field` on the unit cube with a uniform field.
fn refine_tet_count() -> usize {
    let cube = common::unit_cube_mesh();
    let n_surface_verts = cube.vertices.len() / 3;
    let opts = MeshingOptions {
        deterministic: true,
        ..Default::default()
    };
    refine_volume_with_size_field(
        &cube,
        &vec![0.5_f64; n_surface_verts],
        &opts,
        ElementOrderTag::P1,
    )
    .unwrap_or_else(|e| panic!("refine_volume_with_size_field must succeed: {e:?}"))
    .tet_indices()
    .expect("P1 tet mesh")
    .len()
        / 4
}

/// `mesh_surface_to_volume_with_attribution` on the unit cube, with no explicit
/// `mesh_size` so the call is sensitive to the size table it inherits.
///
/// The fourth writer. `resolve_mesh_size` returns `None` for
/// `mesh_size: None` + `auto_size_cfg: None`, so
/// `run_meshing_with_entity_queries` writes no clamp of its own and the
/// process-global table alone decides element size — which is what makes this
/// a DETECTOR leg of the pair sweep rather than a lock-in.
///
/// The attribution is empty and `match_tolerance` is `0.0` ("matching
/// disabled") because anchors decide which B-rep handle each boundary node is
/// attributed TO, not how many elements gmsh produces. The mesh-size writes
/// under test are in the shared meshing helper, reached identically either way,
/// and an empty attribution keeps this binary free of the six-face-anchor
/// fixture `mesh_surface_to_volume_attributed.rs` needs for its own subject.
#[cfg(feature = "mesh-morph")]
fn attributed_tet_count() -> usize {
    let cube = common::unit_cube_mesh();
    let opts = MeshingOptions {
        deterministic: true,
        ..Default::default()
    };
    let attribution = EntityAttribution {
        faces: Vec::new(),
        edges: Vec::new(),
        vertices: Vec::new(),
        match_tolerance: 0.0,
    };
    mesh_surface_to_volume_with_attribution(
        &cube,
        &opts,
        ElementOrderTag::P1,
        None,
        None,
        None,
        &attribution,
    )
    .unwrap_or_else(|e| panic!("mesh_surface_to_volume_with_attribution must succeed: {e:?}"))
    .volume
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
///    `ExtendFromBoundary` trio is NOT this warm-up's job either:
///    `mesh_to_volume` never writes it, and since #6968 every entry point
///    LEAVES it at gmsh's defaults, so both measurements see the same trio
///    without anyone pinning it. (The probe used to pin it itself; removing
///    that is what lets it detect a trio leak rather than mask one.)
/// 2. **Baseline**, from an explicitly-defaulted clamp.
/// 3. **A fine `mesh_to_volume`** — `FINE` is 10x finer than the probe's own
///    extent.
/// 4. **Re-measure.** Must equal the baseline exactly.
///
/// # Measured, with `MeshSizeScope::entered` commented out of `mesh_to_volume`
///
/// baseline = **162** triangles, after `mesh_to_volume(FINE = 0.1)` = **242**
/// triangles — a +49% jump. That 162 → 242 difference IS the defect. It is
/// smaller than the "orders of magnitude" a naive reading of the cap would
/// predict, because the probe is not unconstrained at gmsh's defaults either —
/// but the assertion is an exact equality between two runs of one function, not
/// a threshold, so the margin only has to be non-zero and repeatable, and 80
/// triangles is far outside any rounding.
///
/// Both numbers are reproducible rather than incidental to one test ordering.
/// The same `162 / 242` came back from three different process states: this
/// test alone via `--exact`; this whole binary; and this binary under
/// `--test-threads=1`. The same `162` baseline also came back from
/// `refine_volume_tests.rs::refine_leaves_the_default_clamp_behind_for_a_later_defaults_relying_call`
/// — a different binary, whose probe runs after a refine (that run measured
/// `162 → 944` with `refine_volume.rs`'s own guard commented out).
///
/// That reproducibility used to depend on [`probe_triangle_count`] pinning the
/// size-SOURCE trio itself, because a refine leaked it: the reviewer of #6298
/// measured `48 / 246` from one interleaving of this binary against
/// `162 / 242` from another. Task #6968 closed the leak at its source, so every
/// entry point now restores the trio and the probe pins nothing — which is what
/// lets it detect a leak by any route instead of masking one.
///
/// The 3D meshes in between are ~4.5k P1 tets at `FINE` and ~200 at the 0.5
/// warm-up, so the test stays fast. Those two counts are a cost note, not an
/// assertion.
///
/// Why the probe observes the leak's EFFECT rather than reading the option
/// table back: the effect-based probe is the stronger of the two, because it
/// fails if a size option leaks by ANY route, not only via the one option name
/// a test thought to read. It is no longer the only option — `#6968` added
/// `ffi::option_get_number`, and the direct table read now runs beside this one
/// as [`mesh_to_volume_enters_and_leaves_gmshs_size_defaults_whatever_the_table_held`]
/// below and, from a defaults table,
/// `mesh_to_volume_tests.rs::mesh_to_volume_leaves_every_size_option_at_gmsh_defaults`.
/// The two are complementary: a table read is decisive where this probe is
/// blind (`Mesh.MeshSizeExtendFromBoundary` has no effect under a shut clamp),
/// and this probe catches what a table read cannot name.
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
         `mesh_size_scope::MeshSizeScope` entered in kernel_real.rs::mesh_to_volume",
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
/// the `MeshSizeScope::entered(…)` binding in `kernel_real.rs::mesh_to_volume`
/// (OUTBOUND, #6298 and #6968):
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
         mesh_size_scope::MeshSizeScope entered in kernel_real.rs::mesh_to_volume",
    );
}

/// `mesh_to_volume` neither inherits nor leaks a mesh-size process-global,
/// whatever the option table held when it was called.
///
/// The poisoned half of `mesh_to_volume`'s per-entry-point guard. The
/// unpoisoned half — an outbound read-back from a defaults table — lives with
/// its siblings in
/// `tests/mesh_to_volume_tests.rs::mesh_to_volume_leaves_every_size_option_at_gmsh_defaults`.
/// It is split this way because a poison takes a `GMSH_LOCK` acquisition of its
/// own before the call, and that binary's 13 unserialised `mesh_to_volume`
/// calls can land in the gap and erase it. Here [`CLAMP_TEST_ORDER`] makes
/// poison → call → measure atomic, which is this binary's whole reason for
/// existing.
///
/// Both legs in one test on purpose. A poisoned table that comes back clean
/// proves the outbound direction; the SAME poisoned table producing the same
/// tet count as an unpoisoned run proves the inbound one. Splitting them would
/// let the inbound assertion run from a table the outbound assertion had
/// already cleaned.
///
/// # Measured RED, and what each leg is worth
///
/// With `MeshSizeScope::entered` commented out of `kernel_real::mesh_to_volume`
/// — unit cube, `deterministic: true`, P1, poison as below:
///
/// ```text
/// leg                                 armed    disarmed
/// tet count, from a defaults table      186         186
/// tet count, from a poisoned table      186         141   <- RED
/// table read, MeshSizeMin                 0           1   <- RED
/// table read, MeshSizeMax              1e22           1   <- RED
/// table read, FromPoints                  1           0   <- RED
/// table read, FromCurvature               0          20   <- RED
/// table read, ExtendFromBoundary          1           0   <- RED
/// ```
///
/// Both legs bite, and they bite for different reasons. The three trio rows
/// read back EXACTLY the poison they were handed: `mesh_to_volume` never wrote
/// those options, so it carried a sibling's leak through untouched — the
/// specific hole #6298 left open and #6968 closes. The clamp rows read `1`
/// rather than the poison because the function writes `Min == Max ==
/// resolved_size` itself, and `resolved_size` is this cube's extent; pre-#6968
/// those two rows were already clean, restored by #6298's guard, so the three
/// trio rows are what this task actually adds.
///
/// The tet-count leg is a genuine detector, not a lock-in: 141 against 186 is
/// a 24% drop, driven by the poisoned `MeshSizeFromPoints = 0` — which a shut
/// `Min == Max` clamp does NOT mask, unlike `ExtendFromBoundary`.
///
/// Note the two 186s in the first row. This producer's output from a clean
/// table is byte-identical armed and disarmed, which is the measurement behind
/// the claim that closing the inbound hole moves nothing downstream and lets
/// `reify-solver-elastic`'s calibrated constants stay untouched.
///
/// `deterministic: true` (via [`mesh_to_volume_default_tet_count`]) is
/// load-bearing, not decoration: `MeshingOptions::default()` leaves it false,
/// which lets gmsh run HXT on `available_parallelism()` threads, and the tet
/// count is then not reproducible — measured 185 / 184 / 184 across three
/// consecutive calls on identical input, against a flat 186 / 186 / 186 with
/// it set. An exact-equality tet assertion under the default options is a coin
/// flip.
#[test]
fn mesh_to_volume_enters_and_leaves_gmshs_size_defaults_whatever_the_table_held() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());

    /// Distinctive, and far finer than the cube's extent, so a leak into the
    /// mesher would be loud rather than marginal.
    const POISON_SIZE: f64 = 0.05;

    set_all_size_options_to_defaults();
    let from_defaults = mesh_to_volume_default_tet_count();
    assert!(from_defaults > 0, "mesh_to_volume must produce tets");

    // Every size option away from its default: a fine shut clamp, plus the
    // three size-SOURCE options flipped.
    write_size_options(&|option, default| match option {
        "Mesh.MeshSizeMin" | "Mesh.MeshSizeMax" => POISON_SIZE,
        "Mesh.MeshSizeFromCurvature" => 20.0,
        _ => 1.0 - default,
    });
    let from_poisoned = mesh_to_volume_default_tet_count();

    assert_all_size_options_at_gmsh_defaults(
        "mesh_to_volume, handed a fully poisoned size table,",
        "`MeshSizeScope` in kernel_real.rs — without which it is a silent CARRIER of \
         another entry point's leak, damaging its successors while its own output stays put",
    );

    assert_eq!(
        from_poisoned, from_defaults,
        "mesh_to_volume must mesh against gmsh's size defaults, not against whatever a \
         sibling entry point left in the process-global table: the same call gave \
         {from_defaults} tets from a defaults table and {from_poisoned} from a fully \
         poisoned one. This is the inbound direction of task #6968, closed by \
         `MeshSizeScope::entered` in kernel_real.rs — which matters most on the \
         resolved_size == 0.0 path, where mesh_to_volume writes no clamp of its own and \
         used to inherit the table wholesale",
    );
}

/// Every entry point that writes a gmsh size option produces the same output
/// whatever ran before it in the process.
///
/// The acceptance test for task #6968, covering BOTH call-order directions the
/// task names — `refine -> mesh_to_volume` / `mesh_plane_2d` (#6212's) and
/// `mesh_to_volume -> refine` (#6262's) — in one process, because they are two
/// halves of one defect and fixing either alone yields a guard that is green
/// for the wrong reason.
///
/// Measures each entry point ALONE from a defaults table, then runs every
/// ordered pair (self-pairs included, which pins idempotence) and requires the
/// second one's output to equal its alone-value exactly. The pair sweep does
/// not care WHICH option carried a leak, so unlike the four per-entry-point
/// table reads it cannot be defeated by a leak through an option no test
/// thought to name.
///
/// `ElementOrderTag::P1` and `deterministic: true` throughout. Both are
/// load-bearing rather than stylistic: every entry point writes
/// `General.NumThreads` under `deterministic`, so pinning it removes the only
/// non-size process-global that would otherwise differ between the alone-run
/// and the paired run; and `Mesh.ElementOrder` — which `mesh_plane_2d` never
/// writes and the other three write unconditionally — stays at `1` for every
/// call, so the known element-order inheritance cannot confound the counts. (It
/// is a real leak of the same class, filed as follow-up work rather than folded
/// in: it is not a mesh-SIZE option and fixing it changes `mesh_plane_2d`'s
/// element-order behaviour.)
///
/// # Falsifiability, measured rather than assumed
///
/// A green-on-arrival guard that cannot fail is worthless, so the scope was
/// actually removed and the test re-run. The probed bindings are the
/// `MeshSizeScope::entered` lines in `refine_volume.rs` (the leaking PRODUCER)
/// and in `mesh_profile_2d.rs` (the defaults-relying CONSUMER), and the pair
/// that moves is `refine -> mesh_plane_2d(mesh_size: None)`:
///
/// ```text
/// refine scope | mesh_plane_2d scope | alone | after | verdict
/// armed        | armed  (today)      |   162 |   162 | PASS
/// disarmed     | armed               |   162 |   162 | PASS
/// armed        | disarmed            |   162 |   162 | PASS
/// disarmed     | disarmed (pre-#6968)|   162 |    60 | FAIL  2.7x
/// ```
///
/// So the test is genuinely falsifiable, and precisely at the point that
/// matters: it reds exactly when the discipline is absent from BOTH ends,
/// which is the state the codebase was in when #6968 was filed. Either half
/// alone suffices for this pair — outbound, refine restores what it wrote;
/// inbound, `mesh_plane_2d` establishes the defaults regardless — and that
/// redundancy is the fix working, not the test failing to measure. It is the
/// same shape [`refine_after_mesh_to_volume_honours_its_own_size_field`] below
/// records for its own four combinations, and the deliberate consequence of
/// taking both directions in one type.
///
/// The failing 60 is `mesh_plane_2d(None)` running under BOTH halves of what a
/// disarmed refine leaves: `Mesh.MeshSizeExtendFromBoundary = 0` and
/// `Mesh.MeshSizeMax = 0.5` (its uniform field's maximum). The trio option
/// alone, against a defaults clamp, is worth 48 rather than 60 — measured
/// separately in `mesh_plane_2d_tests.rs`, and the same anomalous number this
/// file's own history records from an unlucky thread interleaving. The leak
/// was seen in the wild before it was explained.
///
/// The fourth entry point was added to the sweep later (review of #6968) and
/// was measured the same way, over the `refine -> attributed` pair and the
/// `MeshSizeScope::entered` bindings in `refine_volume.rs` and
/// `mesh_boundary.rs`:
///
/// ```text
/// refine scope | attributed scope    | alone | after | verdict
/// armed        | armed  (today)      |  1160 |  1160 | PASS
/// disarmed     | armed               |  1160 |  1160 | PASS
/// armed        | disarmed            |  1160 |  1160 | PASS
/// disarmed     | disarmed (pre-#6968)|  1160 |   355 | FAIL  3.3x
/// ```
///
/// Same shape, same conclusion, and it closes a real hole: before this leg the
/// attributed producer was covered only OUTBOUND, by a table read in
/// `mesh_surface_to_volume_attributed.rs` — and a table read is structurally
/// blind to INHERITED state, so a sibling leaking a size option that producer
/// never writes changed its output with no test able to see it.
///
/// # Which legs are detectors and which are lock-ins
///
/// Not every one of the sixteen pairs can move, and a guard should not imply a
/// sensitivity it lacks.
///
/// As the SECOND of a pair, two entry points cannot move. `refine` writes all
/// five size options itself on entry, so it has nothing to inherit;
/// `mesh_to_volume` writes `Mesh.MeshSizeMin == Mesh.MeshSizeMax`, and a shut
/// clamp masks `Mesh.MeshSizeExtendFromBoundary` entirely. The two that CAN
/// move are the defaults-relying ones — `mesh_plane_2d(None)` and the
/// attributed producer, which with `mesh_size: None` writes no clamp either
/// (`resolve_mesh_size` returns `None`, `mesh_boundary.rs`).
///
/// As the FIRST of a pair, the attributed producer leaks nothing to detect: on
/// the `mesh_size: None` path it writes no size option at all, armed or not.
/// Its four rows are lock-ins, kept because "writes nothing" is a property of
/// today's `resolve_mesh_size`, not a contract — the day it grows an
/// auto-sizing default, those rows start biting with no test edit.
///
/// That masking is precisely why the trio leak survived #6298. The damage is
/// invisible in the leaking function's own output, and invisible in any
/// successor that writes a shut clamp of its own — it shows up only in a
/// successor that relies on the defaults, which is the one row that fails.
///
#[test]
fn every_entry_point_measures_the_same_whatever_ran_before_it() {
    let _order = CLAMP_TEST_ORDER.lock().unwrap_or_else(|e| e.into_inner());

    /// An entry point's display name, paired with a measurement of the mesh it
    /// produces, in elements.
    type EntryPoint = (&'static str, fn() -> usize);

    /// Named so a failure message says which pair diverged rather than which
    /// index did.
    ///
    /// A slice with a `cfg`-gated fourth element rather than a fixed-length
    /// array, because the attributed producer is `feature = "mesh-morph"` and
    /// this binary is not. The crate's self dev-dependency enables that feature
    /// for every `tests/` binary, so in the gate the sweep is always 4x4 = 16
    /// ordered pairs; a default-feature build degrades to 3x3 rather than
    /// failing to compile.
    const ENTRY_POINTS: &[EntryPoint] = &[
        ("mesh_plane_2d(mesh_size: None)", probe_triangle_count),
        ("refine_volume_with_size_field", refine_tet_count),
        ("mesh_to_volume", mesh_to_volume_default_tet_count),
        #[cfg(feature = "mesh-morph")]
        ("mesh_surface_to_volume_with_attribution", attributed_tet_count),
    ];

    let alone: Vec<usize> = ENTRY_POINTS
        .iter()
        .map(|(name, measure)| {
            set_all_size_options_to_defaults();
            let n = measure();
            assert!(
                n > 0,
                "{name} must produce elements when run alone; got an empty mesh"
            );
            n
        })
        .collect();

    for (first_name, run_first) in ENTRY_POINTS {
        for (index, (second_name, measure_second)) in ENTRY_POINTS.iter().enumerate() {
            set_all_size_options_to_defaults();
            run_first();
            let after = measure_second();
            assert_eq!(
                after, alone[index],
                "call order changed a mesh: {second_name} produced {after} elements after \
                 {first_name} ran in the same process, against {} when it ran alone from a \
                 defaults table. gmsh's option table is process-global and survives \
                 gmshClear(), so this means one of the two entry points left a mesh-size \
                 option behind — or failed to establish its own. Task #6968: every entry \
                 point must both ENTER at gmsh's size defaults and LEAVE them there, via \
                 `MeshSizeScope` (mesh_size_scope.rs). Check that the scope is still armed \
                 in {first_name} and in {second_name}",
                alone[index],
            );
        }
    }
}
