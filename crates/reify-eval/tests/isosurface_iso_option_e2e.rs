//! End-to-end options-threading proof for `isosurface(..., iso: X)` (task ε,
//! 5003).
//!
//! PRD `docs/prds/v0_3/voxel-to-mesh-surfacing.md` task ε (Phase-3
//! hardening): proves a NON-DEFAULT `iso:` on the `isosurface(...)` builtin
//! measurably changes the surfaced mesh END-TO-END, guarding PRD D4's
//! options-threading path against the C-10 "declared-but-unexercised" shape.
//!
//! Two independent tests each build a narrow-band 20mm-box fixture with
//! DISTINCT `iso:` values:
//!
//! - `iso_option_changes_surfaced_mesh` compares the DEFAULT in-band value
//!   (`0mm` — the same surface `isosurface(solid)` yields with no `iso:`
//!   argument at all) against a non-default in-band value (`3mm`) and
//!   asserts BOTH surface a non-empty mesh, AND that the pair differs in
//!   triangle count or bounding box — see [`ShellStats`] for why that's a
//!   disjunction rather than two hard asserts. Comparing the default
//!   against a non-default value is precisely what makes this an
//!   options-threading proof: if `iso:` were ignored, the `3mm` build would
//!   silently collapse to the same default surface as the `0mm` build and
//!   the two would be indistinguishable. A third leg builds
//!   `isosurface(solid)` with `iso:` OMITTED entirely and asserts it
//!   matches the explicit `iso: 0mm` build's triangle count and bounding
//!   box exactly, pinning the "omitting `iso:` == passing `iso: 0mm`"
//!   equivalence this bullet asserts in prose — a claim the sibling
//!   `voxel_to_mesh_e2e.rs` never directly checks, since it only ever
//!   builds the no-argument form.
//! - `iso_option_out_of_band_surfaces_empty_mesh` is a SEPARATE regression
//!   guard locking in `realize_mesh_from_voxel_with_options`'s documented
//!   `Ok(empty)` no-crossing contract (`kernel_real.rs`): an `iso:` far
//!   outside `MeshToVoxelOptions::honest_floor`'s narrow band is expected to
//!   surface no crossing. Keeping this claim in its own test means a future
//!   legitimate change that widens the band fails only this contract guard,
//!   not the options-threading proof above.
//!
//! `surface_shell_stats` collects each build's `Severity::Error`
//! diagnostics into `ShellStats::error_diagnostics` rather than asserting
//! on them directly, so each call site can fail with a message tied to its
//! own context: `assert_no_build_errors` covers the common "this build
//! must not be silently degraded" case, while
//! `iso_option_out_of_band_surfaces_empty_mesh` inspects
//! `error_diagnostics` itself so a future contract shift (an out-of-band
//! `iso:` starting to emit an error instead of the documented `Ok(empty)`)
//! fails with a message about THAT contract, not a generic degraded-build
//! message. `surface_shell_stats` also asserts a terminal `MeshSurface`
//! entry EXISTS (regardless of whether the mesh itself is empty),
//! distinguishing the documented `Ok(empty)` contract from an absent
//! tessellation entry. The `OCCT_AVAILABLE` skip preamble shared by all
//! three `#[test]` functions below is factored into
//! `occt_available_or_skip`.
//!
//! ## Reuse
//!
//! - Linker anchors, `OCCT_AVAILABLE` runtime gate, `Engine::with_registered_kernel`
//!   + `ensure_openvdb_kernel()` pairing, `snapshot()` terminal-by-index +
//!     `tessellate_realizations()` terminal-mesh extraction:
//!     `crates/reify-eval/tests/voxel_to_mesh_e2e.rs`.
//! - Runtime-read-fixture RED mechanism (`std::fs::read_to_string` of a
//!   `CARGO_MANIFEST_DIR`-relative example path via `.expect(...)`, NOT
//!   `include_str!`, so a missing fixture is a clean test panic rather than a
//!   compile error): `crates/reify-eval/tests/voxel_to_mesh_e2e.rs:86-93`.

// Anchor: force the linker to include the reify_kernel_occt rlib
// unconditionally so its `inventory::submit!` registration fires at binary
// startup, regardless of cfg(has_occt). Mirrors
// crates/reify-eval/tests/voxel_to_mesh_e2e.rs:44-49.
extern crate reify_kernel_occt as _;

// Anchor: same rationale, for reify_kernel_openvdb. Gated on `has_openvdb`
// because the whole test below only makes sense (and only compiles its
// OpenVDB-touching calls) under that cfg. Mirrors
// crates/reify-eval/tests/voxel_to_mesh_e2e.rs:51-56.
#[cfg(has_openvdb)]
extern crate reify_kernel_openvdb as _;

/// Terminal-mesh aggregate signal returned by [`surface_shell_stats`]: a
/// triangle count plus an axis-aligned bounding box. Triangle count alone is
/// a PROXY for "the surfaced mesh changed" — two genuinely distinct
/// isocontours could in principle marching-cubes to the same triangle count
/// while differing in vertex positions. The bounding box tracks actual
/// vertex POSITIONS (independent of triangulation structure), so
/// `iso_option_changes_surfaced_mesh` treats "differs" as EITHER signal
/// disagreeing (a disjunction, not two independent hard asserts) so a
/// coincidental equal-triangle-count case still passes via the bbox
/// fallback instead of flaking.
#[cfg(has_openvdb)]
#[derive(Debug, Clone)]
struct ShellStats {
    triangle_count: usize,
    /// `(min, max)` corner of the mesh's vertex positions, or `None` for an
    /// empty mesh (no vertices to bound).
    bbox: Option<([f32; 3], [f32; 3])>,
    /// `message` of each `Severity::Error` diagnostic from the build, if
    /// any. Populated (not asserted on) by `surface_shell_stats` so each
    /// call site can assert with a message tied to its own context — see
    /// `assert_no_build_errors` and `iso_option_out_of_band_surfaces_empty_mesh`.
    error_diagnostics: Vec<String>,
}

/// Axis-aligned bounding box of a flat `[x0, y0, z0, x1, y1, z1, ...]`
/// vertex buffer (`Mesh::vertices`'s layout), or `None` if `vertices` is
/// empty.
#[cfg(has_openvdb)]
fn bbox_of(vertices: &[f32]) -> Option<([f32; 3], [f32; 3])> {
    if vertices.is_empty() {
        return None;
    }
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for v in vertices.chunks_exact(3) {
        for (i, &c) in v.iter().enumerate() {
            min[i] = min[i].min(c);
            max[i] = max[i].max(c);
        }
    }
    Some((min, max))
}

/// Returns `true` (and the caller should proceed) if OCCT is available;
/// otherwise emits a skip message naming `test_name` and returns `false` (the
/// caller should `return` immediately). Factors out the OCCT-availability
/// skip preamble that was previously copy-pasted verbatim into all three
/// `#[test]` functions below.
#[cfg(has_openvdb)]
fn occt_available_or_skip(test_name: &str) -> bool {
    if reify_kernel_occt::OCCT_AVAILABLE {
        true
    } else {
        eprintln!(
            "skipping {test_name}: OCCT not available (cfg(has_occt) not set — stub-mode build)"
        );
        false
    }
}

/// Builds `source` with a real OCCT + OpenVDB engine and returns the
/// terminal (highest realization index) realization's surfaced-mesh
/// [`ShellStats`] for `entity`.
///
/// Collects the build's `Severity::Error` diagnostics into
/// [`ShellStats::error_diagnostics`] rather than asserting on them here:
/// this still makes a silently degraded build (e.g. an OpenVDB
/// dispatch/registration failure) masquerading as a 0-triangle `Ok(empty)`
/// result detectable, but leaves each call site free to assert with a
/// message tied to its own context — see `assert_no_build_errors` for the
/// common "must not be degraded" case.
///
/// Also asserts a terminal `MeshSurface` entry EXISTS at the terminal
/// realization's entity path: `surface_subtree`'s `Ok(mesh)` handling
/// (`crates/reify-eval/src/geometry_ops.rs`) pushes a `MeshSurface`
/// unconditionally on a successful `kernel.tessellate` — never conditioned
/// on the mesh being non-empty — so a no-error build's terminal realization
/// always has an entry here, EVEN when
/// `realize_mesh_from_voxel_with_options`'s documented `Ok(empty)`
/// no-crossing contract applies. Asserting presence explicitly distinguishes
/// "terminal mesh present but empty" (the genuine `Ok(empty)` contract) from
/// "no mesh entry at all" (which would indicate a real defect), rather than
/// collapsing both to the same 0 count.
///
/// Mirrors the engine-construction and terminal-extraction sequence in
/// `voxel_to_mesh_e2e.rs::voxel_to_mesh_builds_honest_voxel_operand_and_mesh_terminal`:
/// a FRESH `Engine` per call eliminates cross-build snapshot/cache state
/// ambiguity, so the two triangle counts compared by the caller are each
/// independently honest.
#[cfg(has_openvdb)]
fn surface_shell_stats(source: &str, entity: &str) -> ShellStats {
    use reify_core::Severity;
    use reify_ir::ExportFormat;
    use reify_test_support::parse_and_compile_with_stdlib;

    let compiled = parse_and_compile_with_stdlib(source);

    // Same pairing the fixed `cmd_build` uses: single-pick OCCT default +
    // lazily-acquired OpenVDB (adds ONLY the openvdb adapter, leaves
    // default_kernel_name == "occt").
    let checker = reify_constraints::SimpleConstraintChecker;
    let mut engine = reify_eval::Engine::with_registered_kernel(Box::new(checker));
    let acquired = engine.ensure_openvdb_kernel();
    assert!(
        acquired,
        "ensure_openvdb_kernel() must return true under cfg(has_openvdb) \
         (the OpenVDB adapter must be present in the kernel registry)"
    );

    let _eval = engine.eval(&compiled);
    let build = engine.build(&compiled, ExportFormat::Stl);

    let error_diagnostics: Vec<String> = build
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .map(|d| d.message.clone())
        .collect();

    let snap = engine
        .snapshot()
        .expect("snapshot must be Some after a successful build()");
    let mut nodes: Vec<_> = snap
        .graph
        .realizations
        .iter()
        .filter(|(id, _)| id.entity == entity)
        .collect();
    assert!(
        !nodes.is_empty(),
        "expected at least 1 realization node for entity {entity}; got none"
    );
    nodes.sort_by_key(|(id, _)| id.index);
    let (terminal_id, _terminal_node) = *nodes.last().expect("nodes is non-empty (asserted above)");
    let terminal_path = terminal_id.to_string();

    let tess = engine.tessellate_realizations(&compiled);
    let terminal_mesh = tess
        .meshes
        .iter()
        .find(|m| m.entity_path == terminal_path)
        .unwrap_or_else(|| {
            panic!(
                "tessellate_realizations must surface a MeshSurface entry at \
                 entity_path {terminal_path:?} for entity {entity} (a \
                 no-error build's terminal realization always pushes one via \
                 surface_subtree's Ok(mesh) handling, even for an Ok(empty) \
                 mesh); got paths: {:?}",
                tess.meshes
                    .iter()
                    .map(|m| &m.entity_path)
                    .collect::<Vec<_>>()
            )
        });

    ShellStats {
        triangle_count: terminal_mesh.mesh.indices.len() / 3,
        bbox: bbox_of(&terminal_mesh.mesh.vertices),
        error_diagnostics,
    }
}

/// Asserts `stats` (from [`surface_shell_stats`]) carries no
/// `Severity::Error` build diagnostics, reusing the "silently degraded
/// build" framing `surface_shell_stats` used before it switched to
/// collecting (rather than asserting on) error diagnostics. Used by the
/// call sites that treat any build error as an unconditional failure;
/// `iso_option_out_of_band_surfaces_empty_mesh` deliberately does NOT use
/// this helper, asserting on `error_diagnostics` itself instead — see its
/// body for why.
#[cfg(has_openvdb)]
fn assert_no_build_errors(stats: &ShellStats, entity: &str) {
    assert!(
        stats.error_diagnostics.is_empty(),
        "build of entity {entity} must have no error-severity diagnostics \
         (an OpenVDB-registration or dispatch failure would silently \
         degrade the isosurface build and masquerade as an Ok(empty) \
         0-triangle result); got: {:?}",
        stats.error_diagnostics
    );
}

/// Two INLINE sources, byte-identical except the `iso:` literal, prove
/// `iso:` is a LIVE options-threading knob end-to-end: the DEFAULT in-band
/// value (`0mm` — what `isosurface(solid)` also yields with no `iso:`
/// argument at all) and a non-default in-band value (`3mm`, mirroring the
/// committed example fixture) must EACH surface a non-empty mesh, with a
/// DIFFERENT triangle count OR a different bounding box (see
/// [`ShellStats`] for why that's a disjunction, not two independent hard
/// asserts). Comparing two non-empty, in-band results (rather than
/// non-empty vs empty) is the stronger proof: since neither build can
/// collapse to empty, the difference can only be explained by `iso:`
/// genuinely moving the extracted isocontour, not by one build merely
/// falling outside the narrow band. It holds for ANY live `iso:` regardless
/// of the exact band width: agreement on BOTH signals would mean the `3mm`
/// build silently collapsed to the SAME default (`0mm`) surface as the
/// other build (D4 options-threading collapsed to default).
///
/// A third source OMITS `iso:` entirely (`isosurface(solid)`, the same form
/// `examples/multi_kernel/voxel_to_mesh.ri` and `voxel_to_mesh_e2e.rs`
/// build) and must match the explicit `iso: 0mm` build EXACTLY (same
/// triangle count, same bounding box) — pinning the "omitting `iso:` is
/// equivalent to passing `iso: 0mm`" claim this doc comment otherwise only
/// asserts in prose. That equivalence is never directly exercised
/// elsewhere: the sibling `voxel_to_mesh_e2e.rs` builds only the
/// no-argument default, never the explicit `iso: 0mm` form, so nothing else
/// in the test suite would catch a regression where the two diverge.
///
/// See `iso_option_out_of_band_surfaces_empty_mesh` below for the separate
/// `Ok(empty)` no-crossing contract guard.
#[cfg(has_openvdb)]
#[test]
fn iso_option_changes_surfaced_mesh() {
    if !occt_available_or_skip("iso_option_changes_surfaced_mesh") {
        return;
    }

    let source_zero = "structure IsoKnob { param size: Length = 20mm  let solid = box(size, size, size)  let shell = isosurface(solid, iso: 0mm) }";
    let source_inband = "structure IsoKnob { param size: Length = 20mm  let solid = box(size, size, size)  let shell = isosurface(solid, iso: 3mm) }";
    let source_omitted = "structure IsoKnob { param size: Length = 20mm  let solid = box(size, size, size)  let shell = isosurface(solid) }";

    let zero = surface_shell_stats(source_zero, "IsoKnob");
    let inband = surface_shell_stats(source_inband, "IsoKnob");
    let omitted = surface_shell_stats(source_omitted, "IsoKnob");
    assert_no_build_errors(&zero, "IsoKnob");
    assert_no_build_errors(&inband, "IsoKnob");
    assert_no_build_errors(&omitted, "IsoKnob");

    assert!(
        zero.triangle_count > 0,
        "iso: 0mm must surface a non-empty mesh (a closed 20mm box's \
         narrow-band SDF crosses iso=0 at its boundary, per δ/5002); got \
         {} triangles",
        zero.triangle_count
    );
    assert!(
        inband.triangle_count > 0,
        "iso: 3mm must also surface a non-empty mesh (in-band for a 20mm \
         box's ~0.01m narrow-band half-width, mirroring the committed \
         example fixture examples/multi_kernel/voxel_to_mesh_iso.ri); got \
         {} triangles",
        inband.triangle_count
    );
    // Pins the "omitting `iso:` == passing `iso: 0mm`" equivalence this
    // test's doc comment asserts in prose (see above): eval::geometry_ops
    // defaults BOTH the missing-`iso` and missing-`adaptive` named args to
    // the same values `iso: 0mm` (no `adaptive:`) resolves to, so these two
    // builds compute the IDENTICAL GeometryOp::Surface{iso_level: 0.0,
    // adaptive: false} — an exact match, not merely an expected one.
    assert_eq!(
        omitted.triangle_count, zero.triangle_count,
        "omitting `iso:` entirely (`isosurface(solid)`) must surface the \
         same triangle count as the explicit default `iso: 0mm` — the two \
         are documented to be equivalent (see this test's doc comment); \
         got {} (no `iso:` argument) vs {} (`iso: 0mm`)",
        omitted.triangle_count, zero.triangle_count
    );
    assert_eq!(
        omitted.bbox, zero.bbox,
        "omitting `iso:` entirely (`isosurface(solid)`) must surface the \
         same bounding box as the explicit default `iso: 0mm`; got {:?} \
         (no `iso:` argument) vs {:?} (`iso: 0mm`)",
        omitted.bbox, zero.bbox
    );
    // Disjunction, not two independent hard asserts — see ShellStats and the
    // doc comment above for why (a coincidental equal-triangle-count case
    // must still pass via the bbox signal instead of flaking).
    let triangle_counts_differ = zero.triangle_count != inband.triangle_count;
    let bboxes_differ = zero.bbox != inband.bbox;
    assert!(
        triangle_counts_differ || bboxes_differ,
        "iso: 0mm and iso: 3mm — both IN-BAND and both non-empty — must \
         surface a DIFFERENT mesh: either a different triangle count or a \
         different bounding box. This disjunction IS the options-threading \
         proof, driven by a genuine isocontour shift rather than by either \
         build collapsing to empty, and it holds for ANY live `iso:` \
         regardless of the exact narrow-band width; agreement on BOTH \
         signals (triangle counts {} == {}, bboxes {:?} == {:?}) means the \
         `iso:` option never reached marching cubes (D4 options-threading \
         collapsed to default)",
        zero.triangle_count, inband.triangle_count, zero.bbox, inband.bbox
    );
}

/// Separate regression guard for `realize_mesh_from_voxel_with_options`'s
/// documented `Ok(empty)` no-crossing contract (`kernel_real.rs`),
/// deliberately decoupled from the options-threading proof in
/// `iso_option_changes_surfaced_mesh` above: `iso: 10m` is >=500x the 20mm
/// box and >=1000x `MeshToVoxelOptions::honest_floor`'s narrow-band LOWER
/// BOUND (`narrow_band * h >= longest_extent/2`), so it is expected to
/// surface an EMPTY mesh. Keeping this claim in its own test means a future
/// legitimate change that widens the band fails only this contract guard,
/// not the options-threading proof.
///
/// The narrow band's width is a documented LOWER BOUND, not a ceiling, so
/// this exact-zero assertion is still implementation-coupled in principle —
/// the meters-scale margin above is deliberately large so only a drastic
/// band-width change could ever flip it. If it starts failing, check FIRST
/// whether `kernel_real.rs`'s narrow-band voxel count or `honest_floor`
/// voxel size `h` changed before treating it as an options-threading
/// regression. In that case the fix is to raise this test's `iso:` literal
/// further out of band, not to relax the equality.
///
/// Asserts on `ShellStats::error_diagnostics` directly (rather than going
/// through `assert_no_build_errors`) so that IF a future change makes an
/// out-of-band `iso:` emit an error diagnostic instead of the documented
/// `Ok(empty)` result, the failure message names that contract shift
/// specifically, instead of the generic "silently degraded build" wording
/// shared with the other tests in this file.
#[cfg(has_openvdb)]
#[test]
fn iso_option_out_of_band_surfaces_empty_mesh() {
    if !occt_available_or_skip("iso_option_out_of_band_surfaces_empty_mesh") {
        return;
    }

    let source_outband = "structure IsoKnob { param size: Length = 20mm  let solid = box(size, size, size)  let shell = isosurface(solid, iso: 10m) }";
    let outband = surface_shell_stats(source_outband, "IsoKnob");

    // Inspected directly here (not via assert_no_build_errors) so that if a
    // future change makes an out-of-band iso: emit an error diagnostic
    // instead of the documented Ok(empty) result, the failure names that
    // contract shift specifically rather than reporting a generic
    // "silently degraded build".
    assert!(
        outband.error_diagnostics.is_empty(),
        "iso: 10m produced error-severity diagnostics {:?} instead of the \
         documented Ok(empty) no-crossing result \
         (realize_mesh_from_voxel_with_options, kernel_real.rs) — the \
         no-crossing contract itself appears to have changed from \
         returning an empty mesh to emitting an error, which is the \
         regression this test guards against",
        outband.error_diagnostics
    );
    assert_eq!(
        outband.triangle_count, 0,
        "iso: 10m is >=500x the 20mm box and well outside its narrow \
         band's documented LOWER-BOUND width, so it is expected to surface \
         an EMPTY mesh via realize_mesh_from_voxel_with_options's `Ok(empty)` \
         no-crossing contract (kernel_real.rs); got {} triangles (with no \
         error diagnostics, so this is a genuine non-empty surface rather \
         than a degraded build). That bound is a FLOOR, not a ceiling — \
         before treating this failure as an options-threading regression, \
         check whether kernel_real.rs's narrow-band defaults (voxel count \
         or the honest_floor voxel size h) legitimately widened past 10m (a \
         >=500x jump from the 20mm box); if so, raise this test's iso: \
         literal further out of band instead of relaxing the equality",
        outband.triangle_count
    );
}

/// The committed example fixture `examples/multi_kernel/voxel_to_mesh_iso.ri`
/// (`iso: 3mm`, in-band for a 20mm box) must build and surface a non-empty
/// terminal mesh.
///
/// Reads the fixture at runtime via `std::fs::read_to_string`
/// (`concat!(env!("CARGO_MANIFEST_DIR"), ...)`), NOT `include_str!`, so that
/// if the fixture is ever missing or unreadable the test fails cleanly
/// instead of the whole test binary failing to compile.
///
/// This build's `iso: 3mm` overlaps with `iso_option_changes_surfaced_mesh`'s
/// `inband` leg (same literal, same box size), so each test pays for its
/// own full OCCT+OpenVDB engine build of an equivalent source. The overlap
/// is deliberate: this test's job is to validate the COMMITTED FIXTURE FILE
/// itself — a user-facing artifact that could drift from the inline source
/// above independently (e.g. a typo introduced editing the checked-in
/// `.ri`) — not merely the `iso: 3mm` value in the abstract. Keeping it
/// separate also means a fixture-specific regression fails with a message
/// that points straight at `voxel_to_mesh_iso.ri`, rather than being folded
/// into `iso_option_changes_surfaced_mesh`'s options-threading proof.
#[cfg(has_openvdb)]
#[test]
fn iso_example_fixture_surfaces_nonempty() {
    if !occt_available_or_skip("iso_example_fixture_surfaces_nonempty") {
        return;
    }

    // Runtime read (NOT include_str!): keeps a missing/unreadable fixture a
    // clean test failure rather than a compile error that would break the
    // whole test binary.
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/multi_kernel/voxel_to_mesh_iso.ri"
    ))
    .expect(
        "examples/multi_kernel/voxel_to_mesh_iso.ri must exist \
         (task 5003 step-2 creates this fixture)",
    );

    let stats = surface_shell_stats(&source, "VoxelToMeshIso");
    assert_no_build_errors(&stats, "VoxelToMeshIso");
    assert!(
        stats.triangle_count > 0,
        "examples/multi_kernel/voxel_to_mesh_iso.ri (iso: 3mm, in-band for \
         a 20mm box) must surface a non-empty terminal mesh; got {} triangles",
        stats.triangle_count
    );
}
