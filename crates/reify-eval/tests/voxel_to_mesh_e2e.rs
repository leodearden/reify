//! End-to-end honest-signal integration gate for Voxel→Mesh surfacing (task δ, 5002).
//!
//! Proves that a real OCCT + OpenVDB engine builds
//! `examples/multi_kernel/voxel_to_mesh.ri` end-to-end and that the mesh the
//! terminal `isosurface(...)` realization ("shell") emits is honestly sourced
//! from marching cubes on a Voxel grid — NOT from BRep→Mesh tessellation or
//! the `realize_solid_sdf` SampledField detour (PRD
//! docs/prds/v0_3/voxel-to-mesh-surfacing.md §D6/C-5).
//!
//! ## Reachability (two realizations, each single-source)
//!
//! `solid = box(size, size, size)` is the operand: `compute_demanded_reprs`
//! (β, task 5000) forces a `Surface`-op operand to demand `Voxel`, so `solid`
//! realizes `BRep→Mesh(tessellate on occt)→Voxel(ingest on openvdb)` —
//! `produced_repr == Voxel`.
//!
//! `shell = isosurface(solid)` is the terminal realization: the `Stl` export
//! sink demands `Mesh`, so `shell` realizes `Voxel→Mesh` (marching cubes on
//! openvdb, task 5001/γ) — `produced_repr == Mesh`.
//!
//! Each realization has exactly one mesh-source stage, so neither trips the
//! unsupported mixed `BRep→Mesh→Voxel→Mesh` single-realization chain
//! (deferred to task ρ; guard at `engine_build.rs:6812-6833`).
//!
//! ## Engine construction
//!
//! Mirrors the exact pairing the fixed `cmd_build`
//! (`crates/reify-cli/src/main.rs`) uses: `Engine::with_registered_kernel`
//! (single-pick OCCT default) + `ensure_openvdb_kernel()` (idempotently adds
//! OpenVDB, leaves `default_kernel_name` == OCCT). See
//! `crates/reify-eval/tests/ensure_openvdb_kernel.rs` for the engine-level
//! regression pin on that pairing.
//!
//! ## Reuse
//!
//! - Linker anchor + `OCCT_AVAILABLE` runtime gate + `eval→build` +
//!   `snapshot().graph.realizations` produced_repr inspection:
//!   `crates/reify-eval/tests/manifold_cross_kernel_real.rs`.
//! - `produced_repr == Mesh` terminal-node assertion style:
//!   `crates/reify-eval/tests/cross_kernel_handoff.rs:309-325`.
//! - `with_registered_kernel` + `ensure_openvdb_kernel()` pairing + OpenVDB
//!   extern-crate anchor: `crates/reify-eval/tests/ensure_openvdb_kernel.rs`.

// Anchor: force the linker to include the reify_kernel_occt rlib
// unconditionally so its `inventory::submit!` registration fires at binary
// startup, regardless of cfg(has_occt). Mirrors
// crates/reify-eval/tests/manifold_cross_kernel_real.rs and the OCCT/manifold
// anchors in crates/reify-cli/src/main.rs:14,20.
extern crate reify_kernel_occt as _;

// Anchor: same rationale, for reify_kernel_openvdb. Gated on `has_openvdb`
// because the whole test below only makes sense (and only compiles its
// OpenVDB-touching calls) under that cfg. Mirrors
// crates/reify-eval/tests/ensure_openvdb_kernel.rs:22-23.
#[cfg(has_openvdb)]
extern crate reify_kernel_openvdb as _;

/// Real OCCT + OpenVDB engine build of `voxel_to_mesh.ri`: asserts the
/// operand ("solid") realizes `Voxel` and the terminal ("shell") realizes
/// `Mesh` with a non-empty tessellated mesh.
///
/// RED (before step-2 creates the example fixture): the runtime
/// `std::fs::read_to_string` of `examples/multi_kernel/voxel_to_mesh.ri`
/// fails with a clean test panic (not a compile error, since the source is
/// read at runtime via `concat!(env!("CARGO_MANIFEST_DIR"), ...)`, not
/// `include_str!`).
#[cfg(has_openvdb)]
#[test]
fn voxel_to_mesh_builds_honest_voxel_operand_and_mesh_terminal() {
    use reify_constraints::SimpleConstraintChecker;
    use reify_core::Severity;
    use reify_ir::{ExportFormat, ReprKind};
    use reify_test_support::parse_and_compile_with_stdlib;

    if !reify_kernel_occt::OCCT_AVAILABLE {
        eprintln!(
            "skipping voxel_to_mesh_builds_honest_voxel_operand_and_mesh_terminal: \
             OCCT not available (cfg(has_occt) not set — stub-mode build)"
        );
        return;
    }

    // Runtime read (NOT include_str!): a clean test-failure RED when the
    // example fixture does not exist yet (step-2 creates it), rather than a
    // compile error that would break the whole test binary.
    let source = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/multi_kernel/voxel_to_mesh.ri"
    ))
    .expect(
        "examples/multi_kernel/voxel_to_mesh.ri must exist \
         (task 5002 step-2 creates this fixture)",
    );

    let compiled = parse_and_compile_with_stdlib(&source);

    // Same pairing the fixed `cmd_build` uses: single-pick OCCT default +
    // lazily-acquired OpenVDB (adds ONLY the openvdb adapter, leaves
    // default_kernel_name == "occt").
    let checker = SimpleConstraintChecker;
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
        "voxel_to_mesh.ri must build with no error-severity diagnostics \
         (an OpenVDB-registration or dispatch failure would degrade the \
         isosurface build to an Error diagnostic); got: {errors:?}"
    );

    // ── operand (solid) == Voxel, terminal (shell) == Mesh ────────────────
    let snap = engine
        .snapshot()
        .expect("snapshot must be Some after a successful build()");
    let mut nodes: Vec<_> = snap
        .graph
        .realizations
        .iter()
        .filter(|(id, _)| id.entity == "VoxelToMesh")
        .collect();
    assert!(
        nodes.len() >= 2,
        "expected at least 2 realization nodes (solid operand + shell \
         terminal) for entity VoxelToMesh; got {}: {:?}",
        nodes.len(),
        nodes
            .iter()
            .map(|(id, _)| id.to_string())
            .collect::<Vec<_>>()
    );
    nodes.sort_by_key(|(id, _)| id.index);

    let (terminal_id, terminal_node) =
        *nodes.last().expect("nodes is non-empty (asserted above)");
    assert_eq!(
        terminal_node.produced_repr,
        ReprKind::Mesh,
        "terminal realization (shell = isosurface(solid)) must record \
         produced_repr == Mesh (marching-cubes surfacing from the Voxel \
         grid); got {:?}",
        terminal_node.produced_repr
    );

    let operand_realizes_voxel = nodes[..nodes.len() - 1]
        .iter()
        .any(|(_, r)| r.produced_repr == ReprKind::Voxel);
    assert!(
        operand_realizes_voxel,
        "operand realization (solid = box(...)) must record produced_repr \
         == Voxel (β demand-seeding forces a Surface-op operand to Voxel); \
         got: {:?}",
        nodes[..nodes.len() - 1]
            .iter()
            .map(|(_, r)| r.produced_repr)
            .collect::<Vec<_>>()
    );

    // ── terminal Mesh has vertices>0 (binary — no numeric bound) ──────────
    let terminal_path = terminal_id.to_string();
    let tess = engine.tessellate_realizations(&compiled);
    let terminal_mesh = tess
        .meshes
        .iter()
        .find(|m| m.entity_path == terminal_path)
        .unwrap_or_else(|| {
            panic!(
                "tessellate_realizations must surface a MeshSurface at entity_path \
                 {terminal_path:?}; got paths: {:?}",
                tess.meshes
                    .iter()
                    .map(|m| &m.entity_path)
                    .collect::<Vec<_>>()
            )
        });
    assert!(
        !terminal_mesh.mesh.vertices.is_empty() && !terminal_mesh.mesh.indices.is_empty(),
        "terminal (shell) mesh must be non-empty (a closed 20mm box's narrow-band \
         SDF crosses iso=0 at its boundary, so marching cubes must yield a \
         non-empty mesh); got {} vertices, {} indices",
        terminal_mesh.mesh.vertices.len(),
        terminal_mesh.mesh.indices.len()
    );
}
