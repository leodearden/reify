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
//! - `iso_option_changes_surfaced_mesh` compares two IN-BAND non-default
//!   values (`0mm` vs `3mm`, the latter mirroring the committed example
//!   fixture) and asserts BOTH surface a non-empty mesh, AND that the pair
//!   differs in triangle count OR bounding box (either signal suffices).
//!   This is the core options-threading proof: since neither build collapses
//!   to empty, that disjunction can only be driven by a genuine isocontour
//!   shift, not by one build merely falling outside the narrow band. It
//!   holds for ANY live `iso:` regardless of the exact band width: agreement
//!   on BOTH signals would mean `iso:` was ignored (D4 options-threading
//!   collapsed to default). Triangle count alone is a PROXY for "the mesh
//!   changed" (two distinct isocontours could in principle share a triangle
//!   count while differing in vertex positions), so the bounding-box check —
//!   a signal that depends on vertex POSITIONS rather than triangulation
//!   structure — is a fallback that still catches that coincidental-equal-
//!   count case, rather than an independent assertion that could itself
//!   flake on it.
//! - `iso_option_out_of_band_surfaces_empty_mesh` is a SEPARATE regression
//!   guard locking in the documented `Ok(empty)` no-crossing contract
//!   (`crates/reify-kernel-openvdb/src/kernel_real.rs:340-344` — never
//!   panics): `iso: 100mm` (0.1m) is >=5x the 20mm box and well outside its
//!   narrow band's documented LOWER BOUND (`narrow_band * h >=
//!   longest_extent/2`,
//!   `crates/reify-kernel-openvdb/src/kernel_real.rs:623-628` — a floor,
//!   not a ceiling, on band width), so it is EXPECTED that marching cubes
//!   finds no crossing and returns `Ok(empty)`. Keeping this claim in its
//!   own test means a future legitimate change that widens the band (and
//!   makes `iso: 100mm` surface a still-distinct non-empty mesh) fails only
//!   this contract guard, not the options-threading proof above.
//!
//! `surface_shell_stats` additionally asserts each build has no
//! `Severity::Error` diagnostics, so neither test's triangle count can be
//! attributed to a silently degraded/errored build masquerading as a
//! genuine result; it also asserts a terminal `MeshSurface` entry EXISTS
//! (regardless of whether the mesh itself is empty), distinguishing the
//! documented `Ok(empty)` contract from an absent tessellation entry. The
//! `OCCT_AVAILABLE` skip preamble shared by all three `#[test]` functions
//! below is factored into `occt_available_or_skip`.
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
#[derive(Debug, Clone, Copy)]
struct ShellStats {
    triangle_count: usize,
    /// `(min, max)` corner of the mesh's vertex positions, or `None` for an
    /// empty mesh (no vertices to bound).
    bbox: Option<([f32; 3], [f32; 3])>,
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
/// Asserts the build has no `Severity::Error` diagnostics before returning,
/// mirroring `voxel_to_mesh_e2e.rs`'s error-diagnostics guard (lines
/// 112-122): this rules out a silently degraded build (e.g. an OpenVDB
/// dispatch/registration failure) masquerading as a 0-triangle `Ok(empty)`
/// result, and surfaces the underlying diagnostics in the panic message
/// instead of an opaque count.
///
/// Also asserts a terminal `MeshSurface` entry EXISTS at the terminal
/// realization's entity path: `crates/reify-eval/src/geometry_ops.rs:10440`
/// pushes a `MeshSurface` unconditionally on `Ok(mesh)` from
/// `kernel.tessellate` — never conditioned on the mesh being non-empty — so
/// a no-error build's terminal realization always has an entry here, EVEN
/// when the kernel's documented `Ok(empty)` no-crossing contract
/// (`kernel_real.rs:340-344`) applies. Asserting presence explicitly
/// distinguishes "terminal mesh present but empty" (the genuine `Ok(empty)`
/// contract) from "no mesh entry at all" (which would indicate a real
/// defect), rather than collapsing both to the same 0 count.
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

    let errors: Vec<_> = build
        .diagnostics
        .iter()
        .filter(|d| matches!(d.severity, Severity::Error))
        .collect();
    assert!(
        errors.is_empty(),
        "build of entity {entity} must have no error-severity diagnostics \
         (an OpenVDB-registration or dispatch failure would silently \
         degrade the isosurface build and masquerade as an Ok(empty) \
         0-triangle result); got: {errors:?}"
    );

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
                 no-error build's terminal realization always pushes one, \
                 even for an Ok(empty) mesh — see geometry_ops.rs:10440); \
                 got paths: {:?}",
                tess.meshes
                    .iter()
                    .map(|m| &m.entity_path)
                    .collect::<Vec<_>>()
            )
        });

    ShellStats {
        triangle_count: terminal_mesh.mesh.indices.len() / 3,
        bbox: bbox_of(&terminal_mesh.mesh.vertices),
    }
}

/// Two INLINE sources, byte-identical except the `iso:` literal, prove
/// `iso:` is a LIVE options-threading knob end-to-end: two IN-BAND
/// non-default values (`0mm` vs `3mm`, the latter mirroring the committed
/// example fixture) must EACH surface a non-empty mesh, with a DIFFERENT
/// triangle count OR a different bounding box. Comparing two non-empty,
/// in-band results (rather than non-empty vs empty) is the stronger proof:
/// since neither build can collapse to empty, that disjunction can only be
/// explained by `iso:` genuinely moving the extracted isocontour, not by
/// one build merely falling outside the narrow band. It holds for ANY live
/// `iso:` regardless of the exact band width. Agreement on BOTH signals
/// would mean the isosurface `iso:` option never reached marching cubes (D4
/// options-threading collapsed to default).
///
/// Triangle count alone is a PROXY for "the mesh changed": two genuinely
/// distinct isocontours could in principle marching-cubes to the same
/// triangle count while differing in vertex positions. Asserting the two
/// signals as a DISJUNCTION (rather than two independent `assert_ne!`s)
/// means that coincidental-equal-count case still passes via the bbox
/// signal instead of failing the test on a geometry change that genuinely
/// occurred.
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

    let zero = surface_shell_stats(source_zero, "IsoKnob");
    let inband = surface_shell_stats(source_inband, "IsoKnob");

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
    // reviewer_comprehensive test_robustness (amend pass): assert the two
    // signals as a DISJUNCTION, not two independent hard asserts. Triangle
    // count is only a PROXY for "the mesh changed" — two genuinely distinct
    // isocontours could in principle marching-cubes to the same triangle
    // count while differing in vertex positions (a box's symmetric offset
    // shell keeps the same centroid but changes its extent, so in practice
    // the bboxes differ too). Asserting both inequalities independently
    // would make that coincidental-equal-count case a false-negative flake
    // instead of a pass; the bbox signal must be a FALLBACK that still
    // catches a genuine geometry change, not an additional requirement.
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

/// Separate regression guard for the kernel's documented `Ok(empty)`
/// no-crossing contract
/// (`crates/reify-kernel-openvdb/src/kernel_real.rs:340-344` — never
/// panics), deliberately decoupled from the options-threading proof in
/// `iso_option_changes_surfaced_mesh` above: `iso: 100mm` (0.1m) is >=5x
/// the 20mm box and well outside its narrow band's documented LOWER-BOUND
/// width (`narrow_band * h >= longest_extent/2`,
/// `crates/reify-kernel-openvdb/src/kernel_real.rs:623-628` — a floor, not
/// a ceiling, on band width), so it is expected to surface an EMPTY mesh.
/// Keeping this claim in its own test means a future legitimate change
/// that widens the band (and makes `iso: 100mm` surface a still-distinct
/// non-empty mesh) fails only this contract guard, not the
/// options-threading proof.
///
/// `narrow_band * h >= longest_extent/2` is a documented LOWER BOUND, not a
/// ceiling, so this exact-zero assertion is implementation-coupled: if it
/// starts failing, check FIRST whether `kernel_real.rs`'s narrow-band voxel
/// count or `honest_floor` voxel size `h` changed (which would legitimately
/// widen the band past 0.1m) before treating it as an options-threading
/// regression. In that case the fix is to raise this test's `iso:` literal
/// further out of band, not to relax the equality.
#[cfg(has_openvdb)]
#[test]
fn iso_option_out_of_band_surfaces_empty_mesh() {
    if !occt_available_or_skip("iso_option_out_of_band_surfaces_empty_mesh") {
        return;
    }

    let source_outband = "structure IsoKnob { param size: Length = 20mm  let solid = box(size, size, size)  let shell = isosurface(solid, iso: 100mm) }";
    let outband = surface_shell_stats(source_outband, "IsoKnob");

    assert_eq!(
        outband.triangle_count, 0,
        "iso: 100mm (0.1m) is >=5x the 20mm box and well outside its narrow \
         band's documented LOWER-BOUND width, so it is expected to surface \
         an EMPTY mesh via the kernel's `Ok(empty)` no-crossing contract \
         (crates/reify-kernel-openvdb/src/kernel_real.rs:340-344); got \
         {} triangles. That bound is a FLOOR, not a ceiling — before \
         treating this failure as an options-threading regression, check \
         whether kernel_real.rs's narrow-band defaults (voxel count or the \
         honest_floor voxel size h) legitimately widened past 0.1m; if so, \
         raise this test's iso: literal further out of band instead of \
         relaxing the equality",
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
    assert!(
        stats.triangle_count > 0,
        "examples/multi_kernel/voxel_to_mesh_iso.ri (iso: 3mm, in-band for \
         a 20mm box) must surface a non-empty terminal mesh; got {} triangles",
        stats.triangle_count
    );
}
