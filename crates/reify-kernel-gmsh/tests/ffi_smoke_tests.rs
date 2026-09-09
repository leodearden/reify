//! Pin the surface-mesh-I/O FFI surface against libgmsh 4.15.2.
//!
//! All tests in this binary route through [`init::ensure_initialized`]
//! (the `OnceLock<()>`-guarded init path that production code uses). The
//! direct-`ffi::initialize`/`ffi::finalize` lifecycle test lives in its
//! own binary at `tests/ffi_lifecycle_test.rs` so it cannot share
//! process state with these tests; mixing the two paths in one binary
//! would couple test ordering to gmsh's process-wide initialisation
//! semantics (see that file's module doc for the failure modes).
//!
//! Only compiled / run when `cfg(has_gmsh)` is set by `build.rs`. On stub
//! builds (no `/opt/reify-deps`) the file is empty and the test binary
//! contains zero tests — preserving the all-OK posture of `cargo test
//! -p reify-kernel-gmsh` on hosts without libgmsh.

#![cfg(has_gmsh)]

use reify_kernel_gmsh::ffi;
use reify_kernel_gmsh::init;

/// RAII reset of the process-global gmsh diagnostics state this file's
/// logger and census tests perturb: `Mesh.ElementOrder`, `General.Terminal`,
/// and the logger-capture buffer. Mirrors
/// `reify_kernel_gmsh::mesh_size_clamp::MeshSizeClampReset`'s shape —
/// [`Self::armed`] borrows the live `GMSH_LOCK` guard so `drop`'s restore
/// FFI writes land on every exit path (assertion failure or panic included)
/// while the lock is still held, not just the success path a trailing
/// statement would cover.
///
/// One guard type serves both tests: restoring `Mesh.ElementOrder` /
/// `General.Terminal` to `1.0` when a test never changed them, and calling
/// `logger_stop()` when a test never started the logger, are each no-ops
/// this type ignores the result of — matching `drop`'s existing best-effort
/// discipline.
struct DiagnosticsTestReset<'g>(std::marker::PhantomData<&'g std::sync::MutexGuard<'g, ()>>);

impl<'g> DiagnosticsTestReset<'g> {
    fn armed(_guard: &'g std::sync::MutexGuard<'g, ()>) -> Self {
        Self(std::marker::PhantomData)
    }
}

impl Drop for DiagnosticsTestReset<'_> {
    fn drop(&mut self) {
        // Best-effort, like the trailing `ffi::clear()` calls below: a
        // failure here cannot be reported from `drop` and must not mask the
        // test's real pass/fail outcome.
        let _ = ffi::logger_stop();
        let _ = ffi::option_set_number("Mesh.ElementOrder", 1.0);
        let _ = ffi::option_set_number("General.Terminal", 1.0);
    }
}

/// Build a unit box (`(0,0,0)`-`(1,1,1)`) from the built-in-CAD `geo_*`
/// primitives `ffi.rs` already binds, returning the assigned volume tag.
/// Shared by the logger-capture test and the element-type census test
/// (#6205 step-1 / step-3) so both exercise `mesh_generate(3)` against the
/// same geometry.
///
/// Ported from a validated C probe (`gcc` against
/// `/opt/reify-deps/lib/libgmsh.so.4.15.2`, every call `ierr=0`). Signed
/// curve-loop tags matter — a curve loop must be a consistently oriented
/// closed circuit, so the four vertical faces each negate two of their
/// four edges.
fn build_geo_unit_box() -> i32 {
    const COORDS: [(f64, f64, f64); 8] = [
        (0.0, 0.0, 0.0),
        (1.0, 0.0, 0.0),
        (1.0, 1.0, 0.0),
        (0.0, 1.0, 0.0),
        (0.0, 0.0, 1.0),
        (1.0, 0.0, 1.0),
        (1.0, 1.0, 1.0),
        (0.0, 1.0, 1.0),
    ];
    let mut p = [0i32; 8];
    for (i, (x, y, z)) in COORDS.iter().enumerate() {
        p[i] = ffi::geo_add_point(*x, *y, *z, 0.0)
            .unwrap_or_else(|e| panic!("build_geo_unit_box: geo_add_point({i}) failed: {e:?}"));
    }

    const EDGES: [(usize, usize); 12] = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];
    let mut l = [0i32; 12];
    for (i, (a, b)) in EDGES.iter().enumerate() {
        l[i] = ffi::geo_add_line(p[*a], p[*b])
            .unwrap_or_else(|e| panic!("build_geo_unit_box: geo_add_line({i}) failed: {e:?}"));
    }

    // Six faces as signed curve loops (consistently oriented closed circuits).
    let faces: [[i32; 4]; 6] = [
        [l[0], l[1], l[2], l[3]],     // bottom z=0
        [l[4], l[5], l[6], l[7]],     // top    z=1
        [l[0], l[9], -l[4], -l[8]],   // y=0
        [l[1], l[10], -l[5], -l[9]],  // x=1
        [l[2], l[11], -l[6], -l[10]], // y=1
        [l[3], l[8], -l[7], -l[11]],  // x=0
    ];
    let mut surf = [0i32; 6];
    for (i, face) in faces.iter().enumerate() {
        let cl = ffi::geo_add_curve_loop(face).unwrap_or_else(|e| {
            panic!("build_geo_unit_box: geo_add_curve_loop({i}) failed: {e:?}")
        });
        surf[i] = ffi::geo_add_plane_surface(&[cl]).unwrap_or_else(|e| {
            panic!("build_geo_unit_box: geo_add_plane_surface({i}) failed: {e:?}")
        });
    }

    let shell = ffi::geo_add_surface_loop(&surf)
        .expect("build_geo_unit_box: geo_add_surface_loop failed");
    let volume =
        ffi::geo_add_volume(&[shell]).expect("build_geo_unit_box: geo_add_volume failed");
    ffi::geo_synchronize().expect("build_geo_unit_box: geo_synchronize failed");
    volume
}

/// Round-trip a single triangle through the gmsh model API: add a discrete
/// surface entity, push 3 nodes + 1 triangle into it, then read them back
/// and assert the values match.
///
/// Pins the surface-mesh I/O FFI surface that `mesh_to_volume` builds on:
/// `gmshModelAdd`, `gmshModelAddDiscreteEntity`, `gmshModelMeshAddNodes`,
/// `gmshModelMeshAddElements`, `gmshModelMeshGetNodes`,
/// `gmshModelMeshGetElementsByType`. A regression on any of these would
/// surface here as a coordinate / tag mismatch rather than as a confusing
/// 3D-meshing failure inside `mesh_to_volume`.
#[test]
fn gmsh_add_and_read_mesh_nodes_and_triangles_round_trip() {
    let _guard = init::GMSH_LOCK
        .lock()
        .expect("GMSH_LOCK poisoned — a prior test panicked while holding it");

    init::ensure_initialized();
    ffi::clear().expect("ffi::clear failed");

    ffi::model_add("rt").expect("ffi::model_add failed");
    let surf_tag =
        ffi::add_discrete_entity(2, &[]).expect("ffi::add_discrete_entity(dim=2) failed");

    let in_node_tags: [u64; 3] = [1, 2, 3];
    let in_coords: [f64; 9] = [
        0.0, 0.0, 0.0, // node 1
        1.0, 0.0, 0.0, // node 2
        0.0, 1.0, 0.0, // node 3
    ];
    ffi::add_nodes_2d(surf_tag, &in_node_tags, &in_coords).expect("ffi::add_nodes_2d failed");

    let in_tri_tags: [u64; 1] = [1];
    let in_tri_node_tags: [u64; 3] = [1, 2, 3];
    ffi::add_elements_2d(
        surf_tag,
        2, // gmsh element-type 2 = 3-node triangle
        &in_tri_tags,
        &in_tri_node_tags,
    )
    .expect("ffi::add_elements_2d failed");

    let (out_node_tags, out_coords) = ffi::get_nodes_all().expect("ffi::get_nodes_all failed");
    assert_eq!(
        out_node_tags.len(),
        3,
        "expected 3 node tags after add_nodes_2d, got {}",
        out_node_tags.len(),
    );
    assert_eq!(
        out_coords.len(),
        9,
        "expected 9 coords (3 nodes × 3) after add_nodes_2d, got {}",
        out_coords.len(),
    );
    // Build a sorted (tag → coords) mapping so the assertion is index-order
    // independent (gmsh does not promise to return tags in insertion order).
    let mut paired: Vec<(u64, [f64; 3])> = out_node_tags
        .iter()
        .copied()
        .zip(out_coords.chunks_exact(3))
        .map(|(t, c)| (t, [c[0], c[1], c[2]]))
        .collect();
    paired.sort_by_key(|(t, _)| *t);
    for (i, (tag, coord)) in paired.iter().enumerate() {
        assert_eq!(*tag, in_node_tags[i], "node tag mismatch at slot {i}");
        let expected = [in_coords[3 * i], in_coords[3 * i + 1], in_coords[3 * i + 2]];
        for k in 0..3 {
            assert!(
                (coord[k] - expected[k]).abs() < 1e-9,
                "coord mismatch at node tag {tag} component {k}: got {} expected {}",
                coord[k],
                expected[k],
            );
        }
    }

    let (out_elem_tags, out_elem_node_tags) =
        ffi::get_elements_by_type(2).expect("ffi::get_elements_by_type(2) failed");
    assert_eq!(
        out_elem_tags.len(),
        1,
        "expected 1 triangle tag after add_elements_2d, got {}",
        out_elem_tags.len(),
    );
    assert_eq!(
        out_elem_node_tags.len(),
        3,
        "expected 3 node tags (1 triangle × 3 nodes) after add_elements_2d, got {}",
        out_elem_node_tags.len(),
    );
    assert_eq!(
        out_elem_node_tags.as_slice(),
        &in_tri_node_tags[..],
        "triangle node tags must round-trip exactly (gmsh preserves connectivity order)",
    );

    ffi::clear().expect("ffi::clear failed (cleanup)");
}

/// Smoke test the five new built-in-CAD FFI bindings added for the 2D
/// profile-mesher pipeline (T2987, PRD docs/prds/v0_3/hex-wedge-meshing.md
/// task #6): `geo_add_point`, `geo_add_line`, `geo_add_curve_loop`,
/// `geo_add_plane_surface`, `mesh_set_recombine`.
///
/// Builds a unit-square plane surface from 4 points / 4 lines / 1 loop / 1
/// surface and asserts each call returns a positive Gmsh tag (or
/// `Ok(())` for the void-returning `mesh_set_recombine`). A regression on
/// any of these would surface here as a non-positive tag or an error from
/// the wrapper rather than as a confusing 2D-meshing failure inside
/// `mesh_plane_2d`.
#[test]
fn geo_add_point_line_curve_loop_plane_surface_and_set_recombine_round_trip() {
    let _guard = init::GMSH_LOCK
        .lock()
        .expect("GMSH_LOCK poisoned — a prior test panicked while holding it");

    init::ensure_initialized();
    ffi::clear().expect("ffi::clear failed");

    ffi::model_add("smoke_2987").expect("ffi::model_add failed");

    // (a) geo_add_point — two distinct points with positive tags.
    let p1 = ffi::geo_add_point(0.0, 0.0, 0.0, 0.0).expect("ffi::geo_add_point(0,0,0) failed");
    let p2 = ffi::geo_add_point(1.0, 0.0, 0.0, 0.0).expect("ffi::geo_add_point(1,0,0) failed");
    assert!(p1 > 0, "geo_add_point returned non-positive tag {p1}");
    assert!(p2 > 0, "geo_add_point returned non-positive tag {p2}");
    assert_ne!(p1, p2, "geo_add_point returned the same tag twice: {p1}");

    let p3 = ffi::geo_add_point(1.0, 1.0, 0.0, 0.0).expect("ffi::geo_add_point(1,1,0) failed");
    let p4 = ffi::geo_add_point(0.0, 1.0, 0.0, 0.0).expect("ffi::geo_add_point(0,1,0) failed");

    // (b) geo_add_line — four lines forming a unit-square loop.
    let l1 = ffi::geo_add_line(p1, p2).expect("ffi::geo_add_line(p1,p2) failed");
    let l2 = ffi::geo_add_line(p2, p3).expect("ffi::geo_add_line(p2,p3) failed");
    let l3 = ffi::geo_add_line(p3, p4).expect("ffi::geo_add_line(p3,p4) failed");
    let l4 = ffi::geo_add_line(p4, p1).expect("ffi::geo_add_line(p4,p1) failed");
    assert!(
        l1 > 0 && l2 > 0 && l3 > 0 && l4 > 0,
        "non-positive line tag(s)"
    );

    // (c) geo_add_curve_loop — one closed loop from the four lines.
    let loop_tag =
        ffi::geo_add_curve_loop(&[l1, l2, l3, l4]).expect("ffi::geo_add_curve_loop failed");
    assert!(
        loop_tag > 0,
        "geo_add_curve_loop returned non-positive tag {loop_tag}"
    );

    // (d) geo_add_plane_surface — plane surface bounded by the loop.
    let surf_tag =
        ffi::geo_add_plane_surface(&[loop_tag]).expect("ffi::geo_add_plane_surface failed");
    assert!(
        surf_tag > 0,
        "geo_add_plane_surface returned non-positive tag {surf_tag}"
    );

    // Synchronise the built-in CAD into the gmsh model so the surface
    // becomes a real model entity. Without this, the next call hits
    // "Surface N does not exist" — `gmshModelMeshSetRecombine` resolves
    // its (dim, tag) against the synchronised model, not the built-in CAD.
    ffi::geo_synchronize().expect("ffi::geo_synchronize failed");

    // (e) mesh_set_recombine — scopes recombination to this surface. The
    // 45.0 angle is the per-corner deviation tolerance Gmsh uses to decide
    // whether two triangles can be merged into a quad.
    ffi::mesh_set_recombine(2, surf_tag, 45.0).expect("ffi::mesh_set_recombine failed");

    ffi::clear().expect("ffi::clear failed (cleanup)");
}

/// Pins that `logger_start` / `logger_get` / `logger_stop` capture gmsh's
/// Info/Progress stream even when `General.Terminal = 0` — the option every
/// production mesher sets to silence gmsh's own stdout/stderr writes (see
/// the production-mesher list on `ffi::logger_start`'s doc comment).
/// `General.Terminal` and the logger-capture buffer are independent
/// switches on the gmsh side — this test is the whole reason the capture
/// family exists rather than `gmshLoggerGetLastError` alone (which only
/// ever holds the *last error*, not the Info/Progress stream).
///
/// MEASURED baseline (C probe against
/// `/opt/reify-deps/lib/libgmsh.so.4.15.2`, this exact geo box,
/// `General.Terminal=0`): `ierr=0, n=85` lines; `LOG[0]="Info: Meshing
/// 1D..."`, `LOG[1]="Info: Meshing curve 1 (Line)"`, `LOG[2]="Progress:
/// Meshing 1D..."`. gmsh's C-API default for `General.Terminal` is `1`, so
/// setting it to `0` first is what makes this assertion meaningful.
#[test]
fn gmsh_logger_captures_mesh_generate_output_even_with_terminal_silenced() {
    let _guard = init::GMSH_LOCK
        .lock()
        .expect("GMSH_LOCK poisoned — a prior test panicked while holding it");
    // Declared after `_guard` so it drops first (Rust drops locals in
    // reverse declaration order) — its `General.Terminal` restore and
    // `logger_stop()` land while GMSH_LOCK is still held, on every exit
    // path including a panic mid-assertion. See its doc for why one guard
    // covers both this test and the census test below.
    let _diag_reset = DiagnosticsTestReset::armed(&_guard);

    init::ensure_initialized();
    ffi::clear().expect("ffi::clear failed");

    ffi::model_add("logger_smoke").expect("ffi::model_add failed");
    ffi::option_set_number("General.Terminal", 0.0)
        .expect("ffi::option_set_number(General.Terminal) failed");

    build_geo_unit_box();

    // Negative control, and pins logger_get's documented "never started"
    // edge case (see its doc comment in ffi.rs): before logger_start, the
    // capture buffer is empty. This also proves the captured lines below
    // come from the capture window opened below, not some other ambient
    // buffer.
    let pre_start_log = ffi::logger_get().expect("ffi::logger_get (pre-start) failed");
    assert!(
        pre_start_log.is_empty(),
        "expected logger_get to return an empty Vec before logger_start, got {} lines",
        pre_start_log.len(),
    );

    ffi::logger_start().expect("ffi::logger_start failed");
    ffi::mesh_generate(3).expect("ffi::mesh_generate(3) failed");
    let log = ffi::logger_get().expect("ffi::logger_get failed");
    ffi::logger_stop().expect("ffi::logger_stop failed");

    // Pins logger_get's documented "after logger_stop" edge case: stopping
    // the logger drains the buffer, so a subsequent read returns an empty
    // Vec (not an error) rather than replaying what `log` already captured.
    let post_stop_log = ffi::logger_get().expect("ffi::logger_get (post-stop) failed");
    assert!(
        post_stop_log.is_empty(),
        "expected logger_get to return an empty Vec after logger_stop, got {} lines",
        post_stop_log.len(),
    );

    assert!(
        !log.is_empty(),
        "expected at least one captured log line from mesh_generate(3) with \
         General.Terminal=0, got {} lines",
        log.len(),
    );
    for (i, line) in log.iter().enumerate() {
        assert!(!line.is_empty(), "captured log line {i} is empty");
    }
    assert!(
        log.iter().any(|line| line.contains("Meshing")),
        "expected at least one captured line containing \"Meshing\", got: {log:?}",
    );

    // `_diag_reset` restores General.Terminal=1.0 on drop below (see its
    // doc); General.Terminal is a process-global gmsh option (also see the
    // census test below), so a later test must not inherit this one's
    // silenced stdout/stderr regardless of thread-scheduling order.
    ffi::clear().expect("ffi::clear failed (cleanup)");
}

/// Pins that `get_element_types(3, -1)` censuses exactly `[4]` (P1 4-node
/// tet) and exactly `[11]` (P2 10-node tet) on a geo-built unit box meshed
/// with no recombination/extrusion. A box meshed this way holds only
/// tetrahedra in dim 3, and `Mesh.ElementOrder=2` promotes every one of
/// them to the 10-node tet, so the dim-3 type set is a singleton either
/// way — corroborated by production code: `kernel_real::mesh_to_volume`
/// already sets `Mesh.ElementOrder` from `ElementOrderTag` and its comment
/// states "4 = P1 4-node tet, 11 = P2 10-node tet".
///
/// MEASURED (C probe against `/opt/reify-deps/lib/libgmsh.so.4.15.2`, same
/// geo box): P1 `getElementTypes(3,-1) ierr=0 n=1 -> [4]`; P2
/// `getElementTypes(3,-1) ierr=0 n=1 -> [11]`.
///
/// Both legs run under ONE `GMSH_LOCK` acquisition in ONE test function —
/// `Mesh.ElementOrder` is a PROCESS-GLOBAL gmsh option that MEASURABLY
/// survives `gmshClear()` (probed directly: set to 2, `gmshClear`,
/// `gmshOptionGetNumber("Mesh.ElementOrder")` still reads 2). Splitting
/// this into two `#[test]`s would let the P2 leg leak order-2 elements into
/// whichever test the scheduler runs next, since this binary's tests run
/// on separate threads in nondeterministic order, serialized only by
/// `GMSH_LOCK`. Each leg therefore sets the order EXPLICITLY rather than
/// relying on the default, and the function restores `1.0` before
/// returning.
///
/// Deliberately does NOT assert on `get_element_types(-1, -1)` (whole-mesh
/// census): measured `[1, 2, 4, 15]` at P1 and `[8, 9, 11, 15]` at P2 —
/// that pins gmsh's whole-mesh B-rep decomposition, which is far more
/// brittle than the dim-3 census this test actually needs.
///
/// The P1 leg additionally asserts the entity-scoped variant
/// `get_element_types(3, volume)` against the box's own volume tag (not
/// just the dim-scoped `tag=-1` form used elsewhere in this file), and the
/// dim-2 census `get_element_types(2, -1)` — MEASURED `[2]` (3-node
/// triangle) on the same box — so both `tag` and `dim` are each exercised
/// with a non-default value at least once.
///
/// Also sets `General.Terminal = 0.0` up front (mirroring the
/// logger-capture test above): `General.Terminal` is a process-global gmsh
/// option, so this test's own gmsh meshing chatter must not depend on
/// whether the scheduler happens to run it before or after that test.
/// `DiagnosticsTestReset` restores `1.0` on drop, for the same
/// order-independence reason it restores `Mesh.ElementOrder`.
#[test]
fn gmsh_get_element_types_censuses_p1_then_p2_tets_on_a_meshed_box() {
    let _guard = init::GMSH_LOCK
        .lock()
        .expect("GMSH_LOCK poisoned — a prior test panicked while holding it");
    // Declared after `_guard` so it drops first, restoring Mesh.ElementOrder
    // and General.Terminal while GMSH_LOCK is still held on every exit path
    // — see its doc comment (shared with the logger-capture test above).
    let _diag_reset = DiagnosticsTestReset::armed(&_guard);

    init::ensure_initialized();

    // Silence gmsh's own stdout/stderr chatter regardless of test
    // execution order — General.Terminal is a process-global gmsh option
    // (see the logger-capture test above) that measurably survives
    // gmshClear(), so setting it once here covers both legs below without
    // needing to repeat the call.
    ffi::option_set_number("General.Terminal", 0.0)
        .expect("ffi::option_set_number(General.Terminal=0) failed");

    // P1 leg.
    ffi::clear().expect("ffi::clear failed (P1 setup)");
    // Negative control: an unmeshed model drives get_element_types' null /
    // n==0 out-buffer path (through the shared take_gmsh_buf helper) — the
    // one branch nothing else in this suite exercises, since every other
    // call here reads back a census after meshing.
    assert!(
        ffi::get_element_types(3, -1)
            .expect("ffi::get_element_types(3,-1) failed (empty model)")
            .is_empty(),
        "expected get_element_types to return an empty Vec on a freshly cleared model",
    );
    ffi::model_add("census_p1").expect("ffi::model_add failed (P1)");
    ffi::option_set_number("Mesh.ElementOrder", 1.0)
        .expect("ffi::option_set_number(Mesh.ElementOrder=1) failed");
    let volume = build_geo_unit_box();
    ffi::mesh_generate(3).expect("ffi::mesh_generate(3) failed (P1)");
    let p1_types =
        ffi::get_element_types(3, -1).expect("ffi::get_element_types(3,-1) failed (P1)");
    assert_eq!(
        p1_types,
        vec![4],
        "P1 dim-3 element-type census must be exactly [4] (4-node tet), got {p1_types:?}",
    );
    // Entity-scoped variant (tag = the box's own volume, not -1): exercises
    // the `tag >= 0` branch of gmshc.h's documented scoping semantics,
    // which nothing else in this suite calls with a real positive tag.
    let p1_types_scoped = ffi::get_element_types(3, volume)
        .expect("ffi::get_element_types(3, volume) failed (P1)");
    assert_eq!(
        p1_types_scoped,
        vec![4],
        "P1 entity-scoped element-type census must be exactly [4], got {p1_types_scoped:?}",
    );
    // dim-2 census: covers the doc-claimed measured [2] (3-node triangle)
    // on the same box, exercising a `dim` value other than 3.
    let p1_dim2_types =
        ffi::get_element_types(2, -1).expect("ffi::get_element_types(2,-1) failed (P1)");
    assert_eq!(
        p1_dim2_types,
        vec![2],
        "P1 dim-2 element-type census must be exactly [2] (3-node triangle), got {p1_dim2_types:?}",
    );

    // P2 leg.
    ffi::clear().expect("ffi::clear failed (P2 setup)");
    ffi::model_add("census_p2").expect("ffi::model_add failed (P2)");
    ffi::option_set_number("Mesh.ElementOrder", 2.0)
        .expect("ffi::option_set_number(Mesh.ElementOrder=2) failed");
    build_geo_unit_box();
    ffi::mesh_generate(3).expect("ffi::mesh_generate(3) failed (P2)");
    let p2_types =
        ffi::get_element_types(3, -1).expect("ffi::get_element_types(3,-1) failed (P2)");
    assert_eq!(
        p2_types,
        vec![11],
        "P2 dim-3 element-type census must be exactly [11] (10-node tet), got {p2_types:?}",
    );

    // `_diag_reset` restores Mesh.ElementOrder=1.0 and General.Terminal=1.0
    // on drop below: both are process-global gmsh options that survive
    // gmshClear(), so a later test must not inherit order 2 or silenced
    // stdout/stderr from this one.
    ffi::clear().expect("ffi::clear failed (teardown)");
}
