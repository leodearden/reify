//! End-to-end options-threading proof for `isosurface(..., iso: X)` (task ε,
//! 5003).
//!
//! PRD `docs/prds/v0_3/voxel-to-mesh-surfacing.md` task ε (Phase-3
//! hardening): proves a NON-DEFAULT `iso:` on the `isosurface(...)` builtin
//! measurably changes the surfaced mesh END-TO-END, guarding PRD D4's
//! options-threading path against the C-10 "declared-but-unexercised" shape.
//!
//! Two builds of a narrow-band 20mm-box fixture with DISTINCT `iso:` values
//! (`0mm` vs `100mm`) must produce a measurably different outcome: `iso:
//! 0mm` crosses the SDF at the box boundary (non-empty, per δ/5002), while
//! `iso: 100mm` (0.1m) is >=5x the 20mm box and well outside the ~0.01m
//! narrow band (`narrow_band * h >= longest_extent/2`,
//! `crates/reify-kernel-openvdb/src/kernel_real.rs:623-628`), so marching
//! cubes finds no crossing and returns `Ok(empty)`
//! (`crates/reify-kernel-openvdb/src/kernel_real.rs:340-344` — never
//! panics). A BINARY `assert_ne!` on triangle counts is the proof: equal
//! counts would mean `iso:` was ignored (D4 options-threading collapsed to
//! default).
//!
//! ## Reuse
//!
//! - Linker anchors, `OCCT_AVAILABLE` runtime gate, `Engine::with_registered_kernel`
//!   + `ensure_openvdb_kernel()` pairing, `snapshot()` terminal-by-index +
//!   `tessellate_realizations()` terminal-mesh extraction:
//!   `crates/reify-eval/tests/voxel_to_mesh_e2e.rs`.
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

/// Builds `source` with a real OCCT + OpenVDB engine and returns the
/// terminal (highest realization index) realization's surfaced-mesh
/// triangle count for `entity` — 0 if the terminal `MeshSurface` is absent
/// or empty.
///
/// Mirrors the engine-construction and terminal-extraction sequence in
/// `voxel_to_mesh_e2e.rs::voxel_to_mesh_builds_honest_voxel_operand_and_mesh_terminal`:
/// a FRESH `Engine` per call eliminates cross-build snapshot/cache state
/// ambiguity, so the two triangle counts compared by the caller are each
/// independently honest.
#[cfg(has_openvdb)]
fn surface_shell_triangle_count(source: &str, entity: &str) -> usize {
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
    let _build = engine.build(&compiled, ExportFormat::Stl);

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
    let (terminal_id, _terminal_node) =
        *nodes.last().expect("nodes is non-empty (asserted above)");
    let terminal_path = terminal_id.to_string();

    let tess = engine.tessellate_realizations(&compiled);
    tess.meshes
        .iter()
        .find(|m| m.entity_path == terminal_path)
        .map(|m| m.mesh.indices.len() / 3)
        .unwrap_or(0)
}

/// Two INLINE sources, byte-identical except the `iso:` literal, prove
/// `iso:` is a LIVE options-threading knob end-to-end: `iso: 0mm` (baseline,
/// per δ) must surface a non-empty mesh, and `iso: 100mm` — provably outside
/// the ~0.01m narrow band of a 20mm box — must surface a DIFFERENT
/// (empty, per the kernel's `Ok(empty)` no-crossing contract) triangle
/// count. Equal counts would mean the isosurface `iso:` option never
/// reached marching cubes (D4 options-threading collapsed to default).
#[cfg(has_openvdb)]
#[test]
fn iso_option_changes_surfaced_mesh() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping iso_option_changes_surfaced_mesh: OCCT not available \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    let source_zero = "structure IsoKnob { param size: Length = 20mm  let solid = box(size, size, size)  let shell = isosurface(solid, iso: 0mm) }";
    let source_outband = "structure IsoKnob { param size: Length = 20mm  let solid = box(size, size, size)  let shell = isosurface(solid, iso: 100mm) }";

    let tris_zero = surface_shell_triangle_count(source_zero, "IsoKnob");
    let tris_outband = surface_shell_triangle_count(source_outband, "IsoKnob");

    assert!(
        tris_zero > 0,
        "iso: 0mm must surface a non-empty mesh (a closed 20mm box's \
         narrow-band SDF crosses iso=0 at its boundary, per δ/5002); got {tris_zero} triangles"
    );
    assert_ne!(
        tris_zero, tris_outband,
        "iso: 0mm and iso: 100mm must surface DIFFERENT triangle counts — \
         100mm (0.1m) is >=5x the 20mm box and provably outside its ~0.01m \
         narrow band, so marching cubes must find no crossing (Ok(empty)); \
         equal counts ({tris_zero} == {tris_outband}) mean the `iso:` \
         option never reached marching cubes (D4 options-threading \
         collapsed to default)"
    );
}

/// The committed example fixture `examples/multi_kernel/voxel_to_mesh_iso.ri`
/// (`iso: 3mm`, in-band for a 20mm box) must build and surface a non-empty
/// terminal mesh.
///
/// RED (before step-2 creates the example fixture): the runtime
/// `std::fs::read_to_string` of `examples/multi_kernel/voxel_to_mesh_iso.ri`
/// fails with a clean test panic (not a compile error, since the source is
/// read at runtime via `concat!(env!("CARGO_MANIFEST_DIR"), ...)`, not
/// `include_str!`).
#[cfg(has_openvdb)]
#[test]
fn iso_example_fixture_surfaces_nonempty() {
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping iso_example_fixture_surfaces_nonempty: OCCT not available \
             (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // Runtime read (NOT include_str!): a clean test-failure RED when the
    // example fixture does not exist yet (step-2 creates it), rather than a
    // compile error that would break the whole test binary.
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/multi_kernel/voxel_to_mesh_iso.ri"
    ))
    .expect(
        "examples/multi_kernel/voxel_to_mesh_iso.ri must exist \
         (task 5003 step-2 creates this fixture)",
    );

    let tris = surface_shell_triangle_count(&source, "VoxelToMeshIso");
    assert!(
        tris > 0,
        "examples/multi_kernel/voxel_to_mesh_iso.ri (iso: 3mm, in-band for \
         a 20mm box) must surface a non-empty terminal mesh; got {tris} triangles"
    );
}
