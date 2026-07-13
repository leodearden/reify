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
//! `iso: 100mm` (0.1m) is >=5x the 20mm box and well outside its narrow
//! band's documented LOWER BOUND (`narrow_band * h >= longest_extent/2`,
//! `crates/reify-kernel-openvdb/src/kernel_real.rs:623-628` — a floor, not a
//! ceiling, on band width), so it is EXPECTED that marching cubes finds no
//! crossing and returns `Ok(empty)`
//! (`crates/reify-kernel-openvdb/src/kernel_real.rs:340-344` — never
//! panics). The options-threading proof itself is the BINARY `assert_ne!`
//! on triangle counts, which holds for ANY live `iso:` regardless of the
//! exact band width: equal counts would mean `iso:` was ignored (D4
//! options-threading collapsed to default). A separate `assert_eq!` locks
//! in the documented `Ok(empty)` contract as its own regression guard, so a
//! future change that makes `iso: 100mm` surface a (still-distinct)
//! non-empty mesh is caught explicitly rather than only incidentally
//! passing `assert_ne!`. `surface_shell_triangle_count` additionally
//! asserts each build has no `Severity::Error` diagnostics, so the `iso:
//! 100mm` zero count is provably the documented `Ok(empty)` no-crossing
//! path rather than a silently degraded/errored build masquerading as an
//! empty mesh.
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

/// Builds `source` with a real OCCT + OpenVDB engine and returns the
/// terminal (highest realization index) realization's surfaced-mesh
/// triangle count for `entity` — 0 if the terminal `MeshSurface` is absent
/// or empty.
///
/// Asserts the build has no `Severity::Error` diagnostics before returning,
/// mirroring `voxel_to_mesh_e2e.rs`'s error-diagnostics guard (lines
/// 112-122): this rules out a silently degraded build (e.g. an OpenVDB
/// dispatch/registration failure) masquerading as a 0-triangle `Ok(empty)`
/// result, and surfaces the underlying diagnostics in the panic message
/// instead of an opaque count.
///
/// Mirrors the engine-construction and terminal-extraction sequence in
/// `voxel_to_mesh_e2e.rs::voxel_to_mesh_builds_honest_voxel_operand_and_mesh_terminal`:
/// a FRESH `Engine` per call eliminates cross-build snapshot/cache state
/// ambiguity, so the two triangle counts compared by the caller are each
/// independently honest.
#[cfg(has_openvdb)]
fn surface_shell_triangle_count(source: &str, entity: &str) -> usize {
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
    tess.meshes
        .iter()
        .find(|m| m.entity_path == terminal_path)
        .map(|m| m.mesh.indices.len() / 3)
        .unwrap_or(0)
}

/// Two INLINE sources, byte-identical except the `iso:` literal, prove
/// `iso:` is a LIVE options-threading knob end-to-end: `iso: 0mm` (baseline,
/// per δ) must surface a non-empty mesh, and `iso: 100mm` must surface a
/// DIFFERENT triangle count (the binary `assert_ne!`, which is the
/// options-threading proof and holds regardless of the exact narrow-band
/// width). `iso: 100mm` — well outside the narrow band's documented
/// LOWER-BOUND width for a 20mm box — is additionally expected to surface
/// an EMPTY mesh per the kernel's `Ok(empty)` no-crossing contract,
/// asserted explicitly below as its own regression guard. Equal counts
/// would mean the isosurface `iso:` option never reached marching cubes (D4
/// options-threading collapsed to default).
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
         this binary inequality IS the options-threading proof, and holds \
         for ANY live `iso:` regardless of the exact narrow-band width; \
         equal counts ({tris_zero} == {tris_outband}) mean the `iso:` \
         option never reached marching cubes (D4 options-threading \
         collapsed to default)"
    );
    // Separate regression guard for the documented Ok(empty) contract
    // itself (distinct from the assert_ne! options-threading proof above).
    // The cited narrow-band invariant (kernel_real.rs:623-628,
    // `narrow_band * h >= longest_extent/2`) is only a LOWER bound on band
    // width, not a ceiling, so it does not by itself guarantee 100mm lies
    // outside the band — this assert_eq! makes the expected emptiness an
    // explicit, independently-checked claim rather than an assumption baked
    // into the assert_ne! message, so a future change that makes `iso:
    // 100mm` surface a (still-distinct) non-empty mesh is caught here
    // rather than silently passing assert_ne! alone.
    assert_eq!(
        tris_outband, 0,
        "iso: 100mm (0.1m) is >=5x the 20mm box and well outside its narrow \
         band's documented LOWER-BOUND width, so it is expected to surface \
         an EMPTY mesh via the kernel's `Ok(empty)` no-crossing contract \
         (crates/reify-kernel-openvdb/src/kernel_real.rs:340-344); got \
         {tris_outband} triangles"
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
    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping iso_example_fixture_surfaces_nonempty: OCCT not available \
             (cfg(has_occt) not set — stub-mode build)"
        );
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

    let tris = surface_shell_triangle_count(&source, "VoxelToMeshIso");
    assert!(
        tris > 0,
        "examples/multi_kernel/voxel_to_mesh_iso.ri (iso: 3mm, in-band for \
         a 20mm box) must surface a non-empty terminal mesh; got {tris} triangles"
    );
}
